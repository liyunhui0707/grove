use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

pub type Result<T> = std::result::Result<T, String>;
pub const APPLICATION_ID: i32 = 0x4d475256;
// v3 changes the identity contract, not the table layout. Older apps must
// refuse it instead of creating duplicate unrated occurrences after a scan.
pub const SCHEMA_VERSION: i32 = 3;
pub(crate) const BASE_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS folders(id INTEGER PRIMARY KEY,path TEXT UNIQUE NOT NULL);
          CREATE TABLE IF NOT EXISTS assets(id TEXT PRIMARY KEY,folder_id INTEGER NOT NULL REFERENCES folders(id),
            path TEXT NOT NULL,identity TEXT NOT NULL,rating INTEGER CHECK(rating BETWEEN 0 AND 5),revision INTEGER NOT NULL DEFAULT 0);
          CREATE INDEX IF NOT EXISTS asset_paths ON assets(folder_id,path);
          CREATE INDEX IF NOT EXISTS asset_identity ON assets(folder_id,identity);
          CREATE TABLE IF NOT EXISTS history(token TEXT NOT NULL,id TEXT NOT NULL,before INTEGER,after INTEGER,revision INTEGER NOT NULL);
          CREATE TABLE IF NOT EXISTS pending_xmp(id TEXT PRIMARY KEY REFERENCES assets(id),revision INTEGER NOT NULL);";
const SYNC_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS xmp_state(id TEXT PRIMARY KEY REFERENCES assets(id),target TEXT NOT NULL,baseline TEXT,error TEXT,seen_token TEXT,seen_target TEXT,seen_xml TEXT);
  CREATE TABLE IF NOT EXISTS xmp_jobs(token TEXT PRIMARY KEY,asset_id TEXT NOT NULL REFERENCES assets(id),revision INTEGER NOT NULL,
    source TEXT NOT NULL,target TEXT NOT NULL,staged TEXT NOT NULL,previous TEXT NOT NULL,expected TEXT,desired TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('prepared','done','conflict','retained')),error TEXT);
  CREATE UNIQUE INDEX IF NOT EXISTS active_xmp_job ON xmp_jobs(asset_id) WHERE status IN ('prepared','conflict');";
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: String,
    pub folder_id: i64,
    pub name: String,
    pub relative: String,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub modified: u64,
    pub captured: Option<String>,
    pub rating: Option<i32>,
    pub revision: i64,
    pub format: String,
    #[serde(default)]
    pub companions: crate::resources::Companions,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub id: String,
    pub before: Option<i32>,
    pub after: Option<i32>,
    pub revision: i64,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Batch {
    pub token: String,
    pub changes: Vec<Change>,
}

pub fn validate_rating(rating: Option<i32>) -> Result<()> {
    if rating.is_some_and(|r| !(0..=5).contains(&r)) {
        return Err("评分须为未评分或 0–5".into());
    }
    Ok(())
}

pub struct Catalog {
    pub db: Connection,
}
impl Catalog {
    pub fn open(path: &Path) -> Result<Self> {
        let db = Connection::open(path).map_err(|e| e.to_string())?;
        let version: i32 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if version == 2 {
            let old = Self { db };
            crate::backups::create(&old, &path.parent().ok_or("资料目录无效")?.join("backups"))
                .map_err(|_| "升级前安全快照未完成，未升级资料库；请检查空间与写入权限")?;
            return Self::from_connection(old.db);
        }
        Self::from_connection(db)
    }
    pub fn from_connection(mut db: Connection) -> Result<Self> {
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        let app_id: i32 = db
            .pragma_query_value(None, "application_id", |r| r.get(0))
            .map_err(|e| e.to_string())?;
        let version: i32 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if ![0, APPLICATION_ID].contains(&app_id) || !(0..=SCHEMA_VERSION).contains(&version) {
            return Err("资料库版本不兼容，未改写数据库".into());
        }
        let schema = Self::schema(&db)?;
        if (!schema.is_empty() && schema != Self::expected_schema_for(version)?)
            || (schema.is_empty() && version != 0)
        {
            return Err("资料库结构不兼容，未改写数据库".into());
        }
        db.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA trusted_schema=OFF;",
        )
        .map_err(|e| e.to_string())?;
        let tx = db.transaction().map_err(|e| e.to_string())?;
        tx.execute_batch(BASE_SCHEMA).map_err(|e| e.to_string())?;
        tx.execute_batch(SYNC_SCHEMA).map_err(|e| e.to_string())?;
        tx.pragma_update(None, "application_id", APPLICATION_ID)
            .map_err(|e| e.to_string())?;
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(Self { db })
    }
    pub fn expected_schema_for(version: i32) -> Result<Vec<(String, String, String)>> {
        let db = Connection::open_in_memory().map_err(|e| e.to_string())?;
        db.execute_batch(BASE_SCHEMA).map_err(|e| e.to_string())?;
        if version >= 2 {
            db.execute_batch(SYNC_SCHEMA).map_err(|e| e.to_string())?;
        }
        Self::schema(&db)
    }
    pub fn schema(db: &Connection) -> Result<Vec<(String, String, String)>> {
        let mut statement = db.prepare("SELECT type,name,sql FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' ORDER BY type,name").map_err(|e|e.to_string())?;
        statement
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(|e| e.to_string())?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }
    pub fn folder(&self, path: &Path) -> Result<i64> {
        let path = path.to_str().ok_or("暂不支持非 UTF-8 目录")?;
        self.db
            .execute("INSERT OR IGNORE INTO folders(path) VALUES (?1)", [path])
            .map_err(|e| e.to_string())?;
        self.db
            .query_row("SELECT id FROM folders WHERE path=?1", [path], |r| r.get(0))
            .map_err(|e| e.to_string())
    }
    pub fn upsert(
        &self,
        folder: i64,
        path: &Path,
        identity: &str,
        initial: Option<i32>,
    ) -> Result<(String, Option<i32>, i64)> {
        validate_rating(initial)?;
        let path_text = path.to_str().ok_or("暂不支持非 UTF-8 文件名")?;
        // An online legacy occurrence can be upgraded only using its exact
        // current device/inode/birth identity, never its bytes or path alone.
        let legacy = if identity.starts_with("volume:") {
            if crate::imaging::identity(path)? != identity {
                return Err("核对期间照片身份已变化，未关联旧记录".into());
            }
            crate::imaging::identity_from_metadata(
                &std::fs::symlink_metadata(path).map_err(|_| "照片不可读")?,
            )?
        } else {
            identity.into()
        };
        let existing = self.db.query_row("SELECT id,rating,revision FROM assets WHERE folder_id=?1 AND path=?2 AND identity IN (?3,?4) ORDER BY identity=?3 DESC LIMIT 1",
            params![folder,path_text,identity,legacy], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(|e|e.to_string())?;
        if let Some(found) = existing {
            self.db
                .execute(
                    "UPDATE assets SET identity=?1 WHERE id=?2 AND identity=?3 AND identity<>?1",
                    params![identity, &found.0, legacy],
                )
                .map_err(|e| e.to_string())?;
            return Ok(found);
        }
        // A rename is relinked only when the previous occurrence is missing and unambiguous.
        let mut statement = self
            .db
            .prepare(
                "SELECT id,path,rating,revision FROM assets WHERE folder_id=?1 AND identity IN (?2,?3)",
            )
            .map_err(|e| e.to_string())?;
        let candidates = statement
            .query_map(params![folder, identity, legacy], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<i32>>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let missing: Vec<_> = candidates
            .iter()
            .filter(|c| !Path::new(&c.1).exists())
            .collect();
        if candidates.len() == 1 && missing.len() == 1 {
            let c = missing[0];
            self.db
                .execute(
                    "UPDATE assets SET path=?1,identity=?3 WHERE id=?2",
                    params![path_text, c.0, identity],
                )
                .map_err(|e| e.to_string())?;
            return Ok((c.0.clone(), c.2, c.3));
        }
        let id = Uuid::new_v4().to_string();
        self.db
            .execute(
                "INSERT INTO assets(id,folder_id,path,identity,rating) VALUES (?1,?2,?3,?4,?5)",
                params![id, folder, path_text, identity, initial],
            )
            .map_err(|e| e.to_string())?;
        Ok((id, initial, 0))
    }
    pub fn location(&self, id: &str) -> Result<(i64, String, i64, String)> {
        self.db
            .query_row(
                "SELECT folder_id,path,revision,identity FROM assets WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .map_err(|_| "照片记录不存在".into())
    }
    pub fn rate(&mut self, ids: &[String], rating: Option<i32>) -> Result<Batch> {
        self.rate_inner(ids, rating, None)
    }
    pub fn adopt_xmp(
        &mut self,
        id: &str,
        rating: Option<i32>,
        target: &str,
        xml: Option<&str>,
    ) -> Result<Batch> {
        self.rate_inner(&[id.to_string()], rating, Some((id, target, xml)))
    }
    fn rate_inner(
        &mut self,
        ids: &[String],
        rating: Option<i32>,
        observed: Option<(&str, &str, Option<&str>)>,
    ) -> Result<Batch> {
        validate_rating(rating)?;
        if ids.is_empty() || ids.len() > 10000 {
            return Err("请选择 1–10000 张照片".into());
        }
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        if unique.len() != ids.len() {
            return Err("评分目标重复".into());
        }
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        let token = Uuid::new_v4().to_string();
        let mut changes = Vec::new();
        for id in ids {
            let (before, rev): (Option<i32>, i64) = tx
                .query_row(
                    "SELECT rating,revision FROM assets WHERE id=?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|e| e.to_string())?;
            tx.execute(
                "UPDATE assets SET rating=?1,revision=revision+1 WHERE id=?2",
                params![rating, id],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT INTO history VALUES (?1,?2,?3,?4,?5)",
                params![token, id, before, rating, rev + 1],
            )
            .map_err(|e| e.to_string())?;
            tx.execute("INSERT INTO pending_xmp VALUES (?1,?2) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision",params![id,rev+1]).map_err(|e|e.to_string())?;
            changes.push(Change {
                id: id.clone(),
                before,
                after: rating,
                revision: rev + 1,
            });
        }
        if let Some((id, target, xml)) = observed {
            tx.execute("UPDATE xmp_state SET target=?2,baseline=?3,error=NULL,seen_token=NULL,seen_target=NULL,seen_xml=NULL WHERE id=?1",params![id,target,xml]).map_err(|e|e.to_string())?;
            tx.execute("DELETE FROM pending_xmp WHERE id=?1", [id])
                .map_err(|e| e.to_string())?;
            tx.execute(
                "UPDATE xmp_jobs SET status='retained' WHERE asset_id=?1 AND status='conflict'",
                [id],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(Batch { token, changes })
    }
    pub fn undo(&mut self, token: &str) -> Result<Batch> {
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        let changes = {
            let mut statement = tx
                .prepare("SELECT id,before,after,revision FROM history WHERE token=?1")
                .map_err(|e| e.to_string())?;
            statement
                .query_map([token], |r| {
                    Ok(Change {
                        id: r.get(0)?,
                        before: r.get(1)?,
                        after: r.get(2)?,
                        revision: r.get(3)?,
                    })
                })
                .map_err(|e| e.to_string())?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
        };
        if changes.is_empty() {
            return Err("没有可撤销的评分".into());
        }
        let mut restored = Vec::new();
        for c in changes {
            let changed = tx
                .execute(
                    "UPDATE assets SET rating=?1,revision=revision+1 WHERE id=?2 AND revision=?3",
                    params![c.before, c.id, c.revision],
                )
                .map_err(|e| e.to_string())?;
            if changed != 1 {
                return Err("照片评分已变化，未覆盖新评分".into());
            }
            tx.execute("INSERT INTO pending_xmp VALUES (?1,?2) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision",params![c.id,c.revision+1]).map_err(|e|e.to_string())?;
            // Rebase earlier history for repeated undo, but only for the state just restored.
            tx.execute("UPDATE history SET revision=?1 WHERE rowid=(SELECT rowid FROM history WHERE id=?2 AND token<>?3 ORDER BY rowid DESC LIMIT 1)",params![c.revision+1,c.id,token]).map_err(|e|e.to_string())?;
            restored.push(Change {
                id: c.id,
                before: c.after,
                after: c.before,
                revision: c.revision + 1,
            });
        }
        tx.execute("DELETE FROM history WHERE token=?1", [token])
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(Batch {
            token: token.into(),
            changes: restored,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unmarked_spike_catalog_is_upgraded_without_losing_ratings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("catalog.sqlite");
        let db = Connection::open(&path).unwrap();
        db.execute_batch(BASE_SCHEMA).unwrap();
        let mut c = Catalog { db };
        let id = asset(&c, &dir.path().join("a.jpg"), "file-a");
        c.rate(std::slice::from_ref(&id), Some(0)).unwrap();
        c.db.execute_batch("PRAGMA application_id=0; PRAGMA user_version=0;")
            .unwrap();
        drop(c);
        let c = Catalog::open(&path).unwrap();
        let rating: Option<i32> =
            c.db.query_row("SELECT rating FROM assets WHERE id=?1", [id], |r| r.get(0))
                .unwrap();
        assert_eq!(rating, Some(0));
        assert_eq!(
            c.db.pragma_query_value(None, "application_id", |r| r.get::<_, i32>(0))
                .unwrap(),
            APPLICATION_ID
        );
    }
    #[test]
    fn foreign_or_newer_database_is_rejected_without_modifying_its_bytes() {
        let dir = tempfile::tempdir().unwrap();
        for (name, sql) in [
            ("foreign", "CREATE TABLE private_notes(value TEXT);"),
            ("newer", "PRAGMA user_version=4;"),
        ] {
            let path = dir.path().join(name);
            let db = Connection::open(&path).unwrap();
            db.execute_batch(sql).unwrap();
            drop(db);
            let before = std::fs::read(&path).unwrap();
            assert!(Catalog::open(&path).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
    }
    #[test]
    fn v2_upgrade_keeps_a_coherent_old_version_snapshot_before_identity_migration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("catalog.sqlite");
        let mut c = Catalog::open(&path).unwrap();
        let folder = c.folder(dir.path()).unwrap();
        let id = c
            .upsert(
                folder,
                &dir.path().join("legacy.jpg"),
                "legacy-test-identity",
                None,
            )
            .unwrap()
            .0;
        c.rate(std::slice::from_ref(&id), Some(4)).unwrap();
        c.db.pragma_update(None, "user_version", 2).unwrap();
        drop(c);
        let c = Catalog::open(&path).unwrap();
        assert_eq!(
            c.db.pragma_query_value(None, "user_version", |r| r.get::<_, i32>(0))
                .unwrap(),
            3
        );
        let snapshots = crate::backups::list(&dir.path().join("backups")).unwrap();
        assert_eq!(snapshots.items.len(), 1);
        let snapshot = dir
            .path()
            .join("backups")
            .join(format!("{}.sqlite3", snapshots.items[0].id));
        let old = Connection::open(&snapshot).unwrap();
        let retained: (String, String, i32, i64) = old
            .query_row("SELECT id,identity,rating,revision FROM assets", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .unwrap();
        assert_eq!(retained, (id, "legacy-test-identity".into(), 4, 1));
        assert_eq!(
            old.pragma_query_value(None, "user_version", |r| r.get::<_, i32>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            old.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
        drop(c);
        let _ = Catalog::open(&path).unwrap();
        assert_eq!(
            crate::backups::list(&dir.path().join("backups"))
                .unwrap()
                .items
                .len(),
            1
        );
    }
    fn asset(c: &Catalog, p: &Path, key: &str) -> String {
        let root = c.folder(p.parent().unwrap()).unwrap();
        c.upsert(root, p, key, None).unwrap().0
    }
    #[test]
    fn seven_states_survive_reopen_and_undo() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("catalog.db");
        let mut c = Catalog::open(&db).unwrap();
        let id = asset(&c, &dir.path().join("a.jpg"), "file-a");
        for rating in [None, Some(0), Some(1), Some(2), Some(3), Some(4), Some(5)] {
            c.rate(std::slice::from_ref(&id), rating).unwrap();
            let fresh = Catalog::open(&db).unwrap();
            let value: Option<i32> = fresh
                .db
                .query_row("SELECT rating FROM assets WHERE id=?1", [&id], |r| r.get(0))
                .unwrap();
            assert_eq!(value, rating);
        }
        let b = c.rate(std::slice::from_ref(&id), None).unwrap();
        assert_eq!(c.undo(&b.token).unwrap().changes[0].after, Some(5));
    }
    #[test]
    fn transaction_rolls_back_and_does_not_clobber_newer_ratings() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = Catalog::open(&dir.path().join("db")).unwrap();
        let id = asset(&c, &dir.path().join("a.jpg"), "a");
        assert!(c.rate(&[id.clone(), "missing".into()], Some(5)).is_err());
        let b = c.rate(std::slice::from_ref(&id), Some(0)).unwrap();
        assert_eq!(b.changes[0].before, None);
        c.rate(std::slice::from_ref(&id), Some(4)).unwrap();
        assert!(c.undo(&b.token).is_err());
        assert!(c.rate(&[id], Some(-1)).is_err());
    }
    #[test]
    fn copies_are_distinct_and_replaced_path_does_not_inherit_rating() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = Catalog::open(&dir.path().join("db")).unwrap();
        let a = asset(&c, &dir.path().join("a.jpg"), "a");
        c.rate(std::slice::from_ref(&a), Some(5)).unwrap();
        let b = asset(&c, &dir.path().join("b.jpg"), "b");
        assert_ne!(a, b);
        let replacement = asset(&c, &dir.path().join("a.jpg"), "replacement");
        assert_ne!(a, replacement);
    }
    #[test]
    fn consecutive_undo_restores_null_and_zero_separately() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = Catalog::open(&dir.path().join("db")).unwrap();
        let a = asset(&c, &dir.path().join("a.jpg"), "a");
        let b1 = c.rate(std::slice::from_ref(&a), Some(0)).unwrap();
        let b2 = c.rate(std::slice::from_ref(&a), Some(5)).unwrap();
        let b3 = c.rate(std::slice::from_ref(&a), Some(2)).unwrap();
        assert_eq!(c.undo(&b3.token).unwrap().changes[0].after, Some(5));
        assert_eq!(c.undo(&b2.token).unwrap().changes[0].after, Some(0));
        assert_eq!(c.undo(&b1.token).unwrap().changes[0].after, None);
    }
}
