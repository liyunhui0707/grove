//! Explicit catalog-only relinking of a whole, identity-preserving directory move.
use crate::{
    catalog::{Catalog, Result},
    imaging, sidecar,
};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use std::{
    collections::HashSet,
    fs,
    io::ErrorKind,
    path::{Component, Path, PathBuf},
};
use uuid::Uuid;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: i64,
    pub name: String,
    pub photos: i64,
    pub pending: i64,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub token: String,
    pub old_name: String,
    pub new_name: String,
    pub photos: usize,
    pub pending: i64,
}
pub struct Plan {
    pub review: Review,
    folder: i64,
    old: PathBuf,
    pub new: PathBuf,
    root_key: String,
    serial: u64,
    data_version: i64,
}
fn name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}
pub fn list(c: &Catalog) -> Result<Vec<Entry>> {
    let mut q=c.db.prepare("SELECT f.id,f.path,count(a.id),(SELECT count(*) FROM pending_xmp p JOIN assets b ON b.id=p.id WHERE b.folder_id=f.id) FROM folders f JOIN assets a ON a.folder_id=f.id GROUP BY f.id ORDER BY f.id DESC").map_err(|e|e.to_string())?;
    q.query_map([], |r| {
        Ok(Entry {
            id: r.get(0)?,
            name: name(Path::new(&r.get::<_, String>(1)?)),
            photos: r.get(2)?,
            pending: r.get(3)?,
        })
    })
    .map_err(|e| e.to_string())?
    .collect::<std::result::Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())
}
fn offline(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Ok(_) => Err("原目录仍存在，不能将独立副本合并为同一个目录".into()),
        Err(_) => Err("无法确认原目录离线，请先检查访问权限".into()),
    }
}
fn root_key(path: &Path) -> Result<String> {
    let m = fs::symlink_metadata(path).map_err(|_| "新目录不可读")?;
    if !m.is_dir()
        || m.file_type().is_symlink()
        || path.canonicalize().map_err(|_| "新目录不可读")? != path
    {
        return Err("新目录已变化或包含链接，请重新选择".into());
    }
    imaging::identity_from_metadata(&m)
}
fn bindings(
    c: &Catalog,
    folder: i64,
    old: &Path,
    new: &Path,
) -> Result<Vec<(String, String, String)>> {
    let mut q =
        c.db.prepare("SELECT id,path,identity FROM assets WHERE folder_id=?1 ORDER BY id")
            .map_err(|e| e.to_string())?;
    let rows = q
        .query_map([folder], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if rows.is_empty() || rows.len() > 100000 {
        return Err("目录为空或超过本次重新关联上限".into());
    }
    let mut paths = HashSet::new();
    let mut result = Vec::new();
    for (id, path, key) in rows {
        let relative = Path::new(&path)
            .strip_prefix(old)
            .map_err(|_| "既有记录不在原目录内")?;
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
            || !paths.insert(relative.to_path_buf())
        {
            return Err("既有路径存在歧义，未改动关联".into());
        }
        let target = new.join(relative);
        sidecar::verify_source(new, &target, &key).map_err(|_| {
            format!(
                "{}：缺失、链接或文件身份不同；跨卷副本暂不自动关联",
                relative.to_string_lossy()
            )
        })?;
        result.push((
            id,
            path,
            target.to_str().ok_or("暂不支持此路径编码")?.into(),
        ));
    }
    Ok(result)
}
pub fn prepare(c: &Catalog, folder: i64, selected: &Path) -> Result<Plan> {
    if !cfg!(unix) {
        return Err("此平台尚未验证目录重新关联".into());
    }
    let data_version =
        c.db.pragma_query_value(None, "data_version", |r| r.get::<_, i64>(0))
            .map_err(|e| e.to_string())?;
    let old: PathBuf =
        c.db.query_row("SELECT path FROM folders WHERE id=?1", [folder], |r| {
            r.get::<_, String>(0)
        })
        .map(PathBuf::from)
        .map_err(|_| "原目录记录不存在")?;
    offline(&old)?;
    let new = selected.canonicalize().map_err(|_| "新目录不可用")?;
    let key = root_key(&new)?;
    if c.db
        .query_row(
            "SELECT id FROM folders WHERE path=?1",
            [new.to_str().ok_or("路径编码无效")?],
            |r| r.get::<_, i64>(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .is_some()
    {
        return Err("新目录已经作为独立目录打开，未合并其记录".into());
    }
    let rows = bindings(c, folder, &old, &new)?;
    let pending =
        c.db.query_row(
            "SELECT count(*) FROM pending_xmp p JOIN assets a ON a.id=p.id WHERE a.folder_id=?1",
            [folder],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if c.db
        .pragma_query_value(None, "data_version", |r| r.get::<_, i64>(0))
        .map_err(|e| e.to_string())?
        != data_version
    {
        return Err("资料库在核对期间被其他实例更新，请重新核对".into());
    }
    Ok(Plan {
        review: Review {
            token: Uuid::new_v4().to_string(),
            old_name: name(&old),
            new_name: name(&new),
            photos: rows.len(),
            pending,
        },
        folder,
        old,
        new,
        root_key: key,
        serial: c.db.total_changes(),
        data_version,
    })
}
pub fn apply(
    c: &mut Catalog,
    plan: &Plan,
    backups: &Path,
) -> Result<(i64, crate::backups::Snapshot)> {
    if c.db.total_changes() != plan.serial
        || c.db
            .pragma_query_value(None, "data_version", |r| r.get::<_, i64>(0))
            .map_err(|e| e.to_string())?
            != plan.data_version
    {
        return Err("资料库在核对后已变化，请重新核对目录".into());
    }
    offline(&plan.old)?;
    if root_key(&plan.new)? != plan.root_key {
        return Err("新目录已被替换，请重新选择".into());
    }
    let rows = bindings(c, plan.folder, &plan.old, &plan.new)?;
    let snapshot = crate::backups::create(c, backups)?;
    let tx =
        c.db.transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
    // Lock out another writer before the final check and catalog-only transaction.
    if tx
        .pragma_query_value(None, "data_version", |r| r.get::<_, i64>(0))
        .map_err(|e| e.to_string())?
        != plan.data_version
    {
        return Err("资料库在确认期间被其他实例更新，请重新核对".into());
    }
    for (id, old, new) in rows {
        if tx
            .execute(
                "UPDATE assets SET path=?1 WHERE id=?2 AND path=?3",
                params![new, id, old],
            )
            .map_err(|e| e.to_string())?
            != 1
        {
            return Err("资料库关联已变化".into());
        }
        // Journal leaf names and XML baselines stay intact; a later explicit sync rechecks files.
        tx.execute(
            "UPDATE xmp_jobs SET source=?1 WHERE asset_id=?2 AND source=?3",
            params![new, id, old],
        )
        .map_err(|e| e.to_string())?;
    }
    if tx
        .execute(
            "UPDATE folders SET path=?1 WHERE id=?2 AND path=?3",
            params![plan.new.to_str(), plan.folder, plan.old.to_str()],
        )
        .map_err(|e| e.to_string())?
        != 1
    {
        return Err("原目录记录已变化".into());
    }
    tx.execute("UPDATE xmp_state SET seen_token=NULL,seen_target=NULL,seen_xml=NULL WHERE id IN (SELECT id FROM assets WHERE folder_id=?1)",[plan.folder]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok((plan.folder, snapshot))
}

#[cfg(all(test, unix))]
mod tests;
