//! App-owned catalog snapshots. Never copy an active SQLite file or touch source media.
use crate::catalog::{APPLICATION_ID, Catalog, Result, SCHEMA_VERSION};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension,
    backup::{Backup, StepResult},
};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, UNIX_EPOCH},
};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub id: String,
    pub created: u64,
    pub bytes: u64,
    pub photos: i64,
    pub reviewed: i64,
    pub rejected: i64,
}
#[derive(Serialize)]
pub struct SnapshotList {
    pub items: Vec<Snapshot>,
    pub unavailable: usize,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Restored {
    pub restored: Snapshot,
    pub safety_backup: Snapshot,
}

const MAX_BYTES: u64 = 512 * 1024 * 1024;
fn directory(root: &Path) -> Result<()> {
    if root.exists() {
        let meta = fs::symlink_metadata(root).map_err(|e| e.to_string())?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err("备份目录不可用或为链接".into());
        }
    } else {
        fs::create_dir(root).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root, fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
fn path_for(root: &Path, id: &str) -> Result<PathBuf> {
    let uuid = Uuid::parse_str(id).map_err(|_| "备份标识无效")?;
    if uuid.to_string() != id {
        return Err("备份标识无效".into());
    }
    Ok(root.join(format!("{id}.sqlite3")))
}
fn copy_database(source: &Connection, target: &mut Connection) -> Result<()> {
    let backup = Backup::new(source, target).map_err(|e| e.to_string())?;
    let started = Instant::now();
    loop {
        if started.elapsed() > Duration::from_secs(15) {
            return Err("资料库忙碌，备份或恢复已取消，请稍后重试".into());
        }
        match backup.step(128).map_err(|e| e.to_string())? {
            StepResult::Done => return Ok(()),
            StepResult::More => {}
            StepResult::Busy | StepResult::Locked => std::thread::sleep(Duration::from_millis(25)),
            _ => return Err("资料库备份状态异常".into()),
        }
    }
}
fn validate(db: &Connection) -> Result<(i64, i64, i64)> {
    let app_id: i32 = db
        .pragma_query_value(None, "application_id", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let version: i32 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if app_id != APPLICATION_ID || !(1..=SCHEMA_VERSION).contains(&version) {
        return Err("不是兼容的 Moment Grove 资料库快照".into());
    }
    // Reject triggers/views/altered constraints before reading data from the snapshot.
    if Catalog::schema(db)? != Catalog::expected_schema_for(version)? {
        return Err("备份结构不兼容，未执行恢复".into());
    }
    let integrity: String = db
        .query_row("PRAGMA integrity_check(1)", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if integrity != "ok" {
        return Err("备份完整性核验失败".into());
    }
    if db
        .prepare("PRAGMA foreign_key_check")
        .map_err(|e| e.to_string())?
        .exists([])
        .map_err(|e| e.to_string())?
    {
        return Err("备份关联数据不完整".into());
    }
    let invalid: bool = db.query_row("SELECT
        EXISTS(SELECT 1 FROM assets WHERE typeof(revision)<>'integer' OR revision<0 OR typeof(rating) NOT IN ('integer','null') OR rating NOT BETWEEN 0 AND 5) OR
        EXISTS(SELECT 1 FROM pending_xmp p JOIN assets a ON a.id=p.id WHERE typeof(p.revision)<>'integer' OR p.revision<0 OR p.revision<>a.revision) OR
        EXISTS(SELECT 1 FROM history h LEFT JOIN assets a ON a.id=h.id WHERE a.id IS NULL OR typeof(h.revision)<>'integer' OR h.revision<0 OR h.revision>a.revision OR typeof(h.before) NOT IN ('integer','null') OR typeof(h.after) NOT IN ('integer','null') OR h.before NOT BETWEEN 0 AND 5 OR h.after NOT BETWEEN 0 AND 5)",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if invalid {
        return Err("备份评分或修订数据无效".into());
    }
    if version == 2 {
        let invalid:bool=db.query_row("SELECT
          EXISTS(SELECT 1 FROM xmp_state WHERE typeof(baseline) NOT IN ('text','null') OR length(CAST(baseline AS BLOB))>1048576) OR
          EXISTS(SELECT 1 FROM xmp_jobs j JOIN assets a ON a.id=j.asset_id LEFT JOIN pending_xmp p ON p.id=j.asset_id WHERE
            typeof(j.revision)<>'integer' OR j.revision<0 OR j.revision>a.revision OR typeof(j.desired)<>'text' OR
            length(CAST(j.desired AS BLOB))>1048576 OR length(CAST(j.expected AS BLOB))>1048576 OR
            (j.status IN ('prepared','conflict') AND p.id IS NULL))",[],|r|r.get(0)).map_err(|e|e.to_string())?;
        if invalid {
            return Err("备份 XMP 待同步数据无效".into());
        }
    }
    db.query_row(
        "SELECT count(*),count(rating),count(CASE WHEN rating=0 THEN 1 END) FROM assets",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .map_err(|e| e.to_string())
}
fn open_snapshot(root: &Path, id: &str) -> Result<(Connection, Snapshot)> {
    directory(root)?;
    let path = path_for(root, id)?;
    let meta = fs::symlink_metadata(&path).map_err(|_| "备份文件不可用")?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > MAX_BYTES {
        return Err("备份文件无效、过大或为链接".into());
    }
    // macOS temporary roots commonly have /var -> /private/var aliases. Resolve that
    // trusted root before SQLite's NOFOLLOW check; the snapshot itself cannot be a link.
    let canonical = path.canonicalize().map_err(|e| e.to_string())?;
    if canonical.parent() != Some(root.canonicalize().map_err(|e| e.to_string())?.as_path()) {
        return Err("备份路径越界".into());
    }
    let db = Connection::open_with_flags(
        &canonical,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(|e| e.to_string())?;
    db.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON; BEGIN;")
        .map_err(|e| e.to_string())?;
    // Keep this read transaction open through restore, so validation and copy use one snapshot.
    let (photos, reviewed, rejected) = validate(&db)?;
    let created = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|t| t.as_millis() as u64)
        .unwrap_or(0);
    Ok((
        db,
        Snapshot {
            id: id.into(),
            created,
            bytes: meta.len(),
            photos,
            reviewed,
            rejected,
        },
    ))
}
fn verify_locations(db: &Connection, current: &[(String, String, String)]) -> Result<()> {
    for (asset, path, identity) in current {
        let historical = db
            .query_row(
                "SELECT path,identity FROM assets WHERE id=?1",
                [asset],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if historical.is_some_and(|location| location != (path.clone(), identity.clone())) {
            return Err(
                "此快照的照片位置与当前安全整理结果不一致；请选择位置一致的快照，照片不会被移动"
                    .into(),
            );
        }
    }
    Ok(())
}
pub fn create(catalog: &Catalog, root: &Path) -> Result<Snapshot> {
    directory(root)?;
    let pages: i64 = catalog
        .db
        .pragma_query_value(None, "page_count", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let page_size: i64 = catalog
        .db
        .pragma_query_value(None, "page_size", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if pages < 0 || page_size <= 0 || pages.saturating_mul(page_size) as u64 > MAX_BYTES {
        return Err("当前资料库超过本版快照大小上限".into());
    }
    let id = Uuid::new_v4().to_string();
    let partial = root.join(format!(".{id}.incomplete"));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&partial).map_err(|e| e.to_string())?;
    let mut destination = Connection::open(&partial).map_err(|e| e.to_string())?;
    copy_database(&catalog.db, &mut destination)?;
    // Standalone backup: no WAL file is needed to reopen it.
    destination
        .execute_batch("PRAGMA journal_mode=DELETE;")
        .map_err(|e| e.to_string())?;
    validate(&destination)?;
    drop(destination);
    file.sync_all().map_err(|e| e.to_string())?;
    // Publish without replacing any existing snapshot. Failed partials are never listed as usable.
    fs::hard_link(&partial, path_for(root, &id)?).map_err(|e| format!("快照未发布：{e}"))?;
    fs::remove_file(&partial).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    fs::File::open(root)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    open_snapshot(root, &id).map(|(_, info)| info)
}
pub fn list(root: &Path) -> Result<SnapshotList> {
    directory(root)?;
    let mut items = Vec::new();
    let mut unavailable = 0;
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("sqlite3") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        match open_snapshot(root, id) {
            Ok((_, info)) => items.push(info),
            Err(_) => unavailable += 1,
        }
    }
    items.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| a.id.cmp(&b.id)));
    Ok(SnapshotList { items, unavailable })
}
#[cfg(test)]
pub fn restore(catalog: &mut Catalog, root: &Path, id: &str) -> Result<Restored> {
    restore_with_locations(catalog, root, id, &[])
}
pub fn restore_with_locations(
    catalog: &mut Catalog,
    root: &Path,
    id: &str,
    current: &[(String, String, String)],
) -> Result<Restored> {
    let (source, restored) = open_snapshot(root, id)?;
    // Check and copy the very same read transaction, not a separately reopened snapshot file.
    verify_locations(&source, current)?;
    // Validate and upgrade old snapshots in a separate database before touching the live one.
    let mut upgraded = Connection::open_in_memory().map_err(|e| e.to_string())?;
    copy_database(&source, &mut upgraded)?;
    let upgraded = Catalog::from_connection(upgraded)?;
    let safety_backup = create(catalog, root)?;
    catalog.db.flush_prepared_statement_cache();
    copy_database(&upgraded.db, &mut catalog.db)?;
    Ok(Restored {
        restored,
        safety_backup,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    #[test]
    fn version_one_snapshot_restores_into_current_schema_without_changing_source() {
        let dir = tempfile::tempdir().unwrap();
        let db = Connection::open(dir.path().join("legacy.sqlite")).unwrap();
        db.execute_batch(crate::catalog::BASE_SCHEMA).unwrap();
        db.pragma_update(None, "application_id", APPLICATION_ID)
            .unwrap();
        db.pragma_update(None, "user_version", 1).unwrap();
        let mut legacy = Catalog { db };
        let folder = legacy.folder(&dir.path().join("generated")).unwrap();
        let id = legacy
            .upsert(
                folder,
                &dir.path().join("generated/a.jpg"),
                "legacy-file",
                None,
            )
            .unwrap()
            .0;
        legacy.rate(std::slice::from_ref(&id), Some(0)).unwrap();
        let root = dir.path().join("backups");
        let saved = create(&legacy, &root).unwrap();
        let before = fs::read(path_for(&root, &saved.id).unwrap()).unwrap();
        let mut current = Catalog::open(&dir.path().join("current.sqlite")).unwrap();
        restore(&mut current, &root, &saved.id).unwrap();
        assert_eq!(ratings(&current), vec![Some(0)]);
        assert_eq!(
            current
                .db
                .pragma_query_value(None, "user_version", |r| r.get::<_, i32>(0))
                .unwrap(),
            SCHEMA_VERSION
        );
        assert_eq!(
            fs::read(path_for(&root, &saved.id).unwrap()).unwrap(),
            before
        );
    }
    fn setup(root: &Path) -> (Catalog, Vec<String>) {
        let mut catalog = Catalog::open(&root.join("catalog.sqlite")).unwrap();
        let folder = catalog.folder(&root.join("generated-media")).unwrap();
        let ids: Vec<_> = (0..7)
            .map(|i| {
                catalog
                    .upsert(
                        folder,
                        &root.join(format!("{i}.jpg")),
                        &format!("file-{i}"),
                        None,
                    )
                    .unwrap()
                    .0
            })
            .collect();
        for (id, value) in
            ids.iter()
                .zip([None, Some(0), Some(1), Some(2), Some(3), Some(4), Some(5)])
        {
            catalog.rate(std::slice::from_ref(id), value).unwrap();
        }
        (catalog, ids)
    }
    fn ratings(c: &Catalog) -> Vec<Option<i32>> {
        c.db.prepare("SELECT rating FROM assets ORDER BY path")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }
    #[test]
    fn live_wal_snapshot_restores_all_seven_states_and_saves_current_state() {
        let dir = tempfile::tempdir().unwrap();
        let (mut c, ids) = setup(dir.path());
        c.db.execute_batch("PRAGMA wal_autocheckpoint=0;").unwrap();
        let before = ratings(&c);
        let root = dir.path().join("backups");
        let saved = create(&c, &root).unwrap();
        assert_eq!((saved.photos, saved.reviewed, saved.rejected), (7, 6, 1));
        c.rate(&ids, Some(5)).unwrap();
        let after = ratings(&c);
        let restored = restore(&mut c, &root, &saved.id).unwrap();
        assert_eq!(ratings(&c), before);
        assert_ne!(saved.id, restored.safety_backup.id);
        assert_eq!(list(&root).unwrap().items.len(), 2);
        restore(&mut c, &root, &restored.safety_backup.id).unwrap();
        assert_eq!(ratings(&c), after);
        drop(c);
        assert_eq!(
            ratings(&Catalog::open(&dir.path().join("catalog.sqlite")).unwrap()),
            after
        );
    }
    #[test]
    fn invalid_foreign_and_newer_snapshots_never_change_live_ratings() {
        let dir = tempfile::tempdir().unwrap();
        let (mut c, _) = setup(dir.path());
        let root = dir.path().join("backups");
        let before = ratings(&c);
        assert!(restore(&mut c, &root, "../catalog").is_err());
        for variant in 0..4 {
            let saved = create(&c, &root).unwrap();
            let path = path_for(&root, &saved.id).unwrap();
            if variant == 0 {
                fs::write(&path, b"broken snapshot").unwrap();
            } else {
                let bad = Connection::open(&path).unwrap();
                match variant {
                    1 => bad.pragma_update(None, "application_id", 0).unwrap(),
                    2 => bad
                        .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
                        .unwrap(),
                    _ => {
                        bad.execute("CREATE TRIGGER sqliteXsurprising AFTER UPDATE ON assets BEGIN UPDATE assets SET rating=0; END", []).unwrap();
                    }
                }
            }
            assert!(restore(&mut c, &root, &saved.id).is_err());
            assert_eq!(ratings(&c), before);
        }
        assert_eq!(list(&root).unwrap().unavailable, 4);
    }
    #[test]
    fn semantically_invalid_snapshot_is_rejected_before_any_restore() {
        let dir = tempfile::tempdir().unwrap();
        let (mut c, _) = setup(dir.path());
        let root = dir.path().join("backups");
        let before = ratings(&c);
        let saved = create(&c, &root).unwrap();
        let bad = Connection::open(path_for(&root, &saved.id).unwrap()).unwrap();
        bad.execute("UPDATE pending_xmp SET revision=?1", params![-1])
            .unwrap();
        drop(bad);
        assert!(restore(&mut c, &root, &saved.id).is_err());
        assert_eq!(ratings(&c), before);
    }
    #[cfg(unix)]
    #[test]
    fn linked_snapshot_and_linked_backup_directory_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let (mut c, _) = setup(dir.path());
        let root = dir.path().join("backups");
        let saved = create(&c, &root).unwrap();
        let alias = Uuid::new_v4().to_string();
        std::os::unix::fs::symlink(
            path_for(&root, &saved.id).unwrap(),
            root.join(format!("{alias}.sqlite3")),
        )
        .unwrap();
        assert!(restore(&mut c, &root, &alias).is_err());
        let link = dir.path().join("linked");
        std::os::unix::fs::symlink(&root, &link).unwrap();
        assert!(create(&c, &link).is_err());
    }
}
