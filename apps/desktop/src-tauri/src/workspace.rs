//! Small restart checkpoint, deliberately separate from the rating catalog/schema.
use crate::{AppState, Folder, catalog::Result, grant, imaging, moves};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub mode: String,
    pub active: String,
    pub filter: String,
    pub sort: String,
    pub search: String,
    pub size: u32,
    pub auto_advance: bool,
    pub theme: String,
    pub offset: f64,
}
impl View {
    fn validate(&self) -> Result<()> {
        if !["grid", "review"].contains(&self.mode.as_str())
            || ![
                "all",
                "unreviewed",
                "picks",
                "reject",
                "0",
                "1",
                "2",
                "3",
                "4",
                "5",
            ]
            .contains(&self.filter.as_str())
            || !["name", "date", "rating"].contains(&self.sort.as_str())
            || !["light", "dark"].contains(&self.theme.as_str())
            || !(200..=380).contains(&self.size)
            || self.active.len() > 128
            || self.search.len() > 4096
            || !self.offset.is_finite()
            || !(0.0..=1e9).contains(&self.offset)
        {
            return Err("审片位置格式无效，未保存".into());
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
struct Record {
    version: u32,
    folder_id: Option<i64>,
    path: Option<PathBuf>,
    identity: Option<String>,
    view: View,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    pub folder_id: Option<i64>,
    pub name: String,
    pub view: View,
}

fn read(state: &AppState) -> Result<Option<Record>> {
    let path = state.data.join("workspace.json");
    let meta = match fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("无法读取上次审片位置；可重新打开文件夹".into()),
    };
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 65536 {
        return Err("上次审片位置记录无效；可重新打开文件夹".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 65536 {
        return Err("审片位置记录过大".into());
    }
    let r: Record =
        serde_json::from_slice(&bytes).map_err(|_| "上次审片位置记录损坏；可重新打开文件夹")?;
    r.view.validate()?;
    if r.version != 1
        || r.folder_id.is_some() != r.path.is_some()
        || r.path.is_some() != r.identity.is_some()
    {
        return Err("上次审片位置版本无效；可重新打开文件夹".into());
    }
    Ok(Some(r))
}
pub fn load(state: &AppState) -> Result<Option<Saved>> {
    let _guard = state.catalog.lock().map_err(|_| "资料库忙碌")?;
    Ok(read(state)?.map(|r| Saved {
        folder_id: r.folder_id,
        name: r
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        view: r.view,
    }))
}
pub fn save(state: &AppState, folder_id: Option<i64>, view: View) -> Result<()> {
    view.validate()?;
    let _guard = state.catalog.lock().map_err(|_| "资料库忙碌")?;
    let root = folder_id
        .map(|id| {
            state
                .allowed
                .lock()
                .map_err(|_| "目录忙碌")?
                .get(&id)
                .cloned()
                .ok_or("目录尚未授权")
        })
        .transpose()?;
    let identity = root
        .as_ref()
        .map(|p| {
            let m = fs::symlink_metadata(p).map_err(|_| "目录已离线，审片位置暂未保存")?;
            if !m.is_dir() || m.file_type().is_symlink() {
                return Err("目录身份无效".into());
            }
            imaging::directory_identity(p)
        })
        .transpose()?;
    if let Some(id) = folder_id
        && state
            .granted_identities
            .lock()
            .map_err(|_| "目录忙碌")?
            .get(&id)
            != identity.as_ref()
    {
        return Err("目录身份已变化，审片位置未保存；请重新选择文件夹".into());
    }
    let path = state.data.join("workspace.json");
    if let Ok(m) = fs::symlink_metadata(&path)
        && (!m.is_file() || m.file_type().is_symlink())
    {
        return Err("审片位置记录不是普通文件，未覆盖".into());
    }
    let raw = serde_json::to_vec(&Record {
        version: 1,
        folder_id,
        path: root,
        identity,
        view,
    })
    .map_err(|e| e.to_string())?;
    let temp = state
        .data
        .join(format!("workspace-{}.tmp", uuid::Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|_| "无法保存审片位置")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
    }
    file.write_all(&raw)
        .and_then(|_| file.sync_all())
        .map_err(|_| "无法保存审片位置")?;
    fs::rename(temp, path).map_err(|_| "无法保存审片位置")?;
    Ok(())
}
pub fn forget(state: &AppState) -> Result<()> {
    // Retain the old checkpoint for diagnosis, but never auto-open it after a
    // catalog restore has deliberately revoked directory access.
    let path = state.data.join("workspace.json");
    match fs::symlink_metadata(&path) {
        Ok(_) => fs::rename(
            &path,
            state
                .data
                .join(format!("workspace-retained-{}.json", uuid::Uuid::new_v4())),
        )
        .map_err(|_| "无法停用旧审片位置")?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("无法停用旧审片位置".into()),
    }
    Ok(())
}
pub fn resume(state: &AppState) -> Result<Option<Folder>> {
    let _access = moves::access(&state.data, false)?;
    let Some(r) = read(state)? else {
        return Ok(None);
    };
    let Some(path) = r.path else {
        return Ok(None);
    };
    let unavailable = || "上次文件夹已离线、无读取权限或身份变化，请重新选择文件夹".to_string();
    if path.canonicalize().map_err(|_| unavailable())? != path {
        return Err(unavailable());
    }
    let meta = fs::symlink_metadata(&path).map_err(|_| unavailable())?;
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || !imaging::matches_directory_identity(
            &path,
            r.identity.as_deref().ok_or_else(unavailable)?,
        )?
    {
        return Err(unavailable());
    }
    fs::read_dir(&path).map_err(|_| unavailable())?;
    let recorded: String = state
        .catalog
        .lock()
        .map_err(|_| "资料库忙碌")?
        .db
        .query_row(
            "SELECT path FROM folders WHERE id=?1",
            [r.folder_id],
            |row| row.get(0),
        )
        .map_err(|_| unavailable())?;
    if std::path::Path::new(&recorded) != path {
        return Err(unavailable());
    }
    // Remembered access is limited to the same previously selected directory occurrence.
    grant(state, path).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety_tests::state;
    fn view() -> View {
        serde_json::from_value(serde_json::json!({"mode":"review","active":"photo-id","filter":"unreviewed","sort":"date","search":"旅程","size":300,"autoAdvance":false,"theme":"dark","offset":120.5})).unwrap()
    }
    #[test]
    fn restart_restores_only_same_directory_and_never_modifies_rating_catalog() {
        let t = tempfile::tempdir().unwrap();
        let base = t.path().canonicalize().unwrap();
        let data = base.join("data");
        let root = base.join("photos");
        fs::create_dir(&root).unwrap();
        let s = state(&data);
        let f = grant(&s, root.clone()).unwrap();
        save(&s, Some(f.id), view()).unwrap();
        drop(s);
        let s = state(&data);
        assert!(s.allowed.lock().unwrap().is_empty());
        let loaded = load(&s).unwrap().unwrap();
        assert_eq!(loaded.view.active, "photo-id");
        assert_eq!(loaded.view.search, "旅程");
        assert_eq!(resume(&s).unwrap().unwrap().id, f.id);
        fs::rename(&root, base.join("offline")).unwrap();
        assert!(resume(&s).is_err());
        fs::create_dir(&root).unwrap();
        assert!(resume(&s).is_err());
        let count: i64 = s
            .catalog
            .lock()
            .unwrap()
            .db
            .query_row("SELECT COUNT(*) FROM assets", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
    #[test]
    fn invalid_checkpoint_preserves_prior_state_and_corruption_does_not_touch_catalog() {
        let t = tempfile::tempdir().unwrap();
        let s = state(t.path());
        save(&s, None, view()).unwrap();
        let before = fs::read(t.path().join("workspace.json")).unwrap();
        let mut bad = view();
        bad.size = 900;
        assert!(save(&s, None, bad).is_err());
        assert_eq!(fs::read(t.path().join("workspace.json")).unwrap(), before);
        fs::write(t.path().join("workspace.json"), b"broken").unwrap();
        assert!(load(&s).is_err());
        assert!(
            s.catalog
                .lock()
                .unwrap()
                .db
                .execute_batch("PRAGMA integrity_check;")
                .is_ok()
        );
    }
    #[test]
    fn replaced_directory_cannot_be_saved_as_the_original_grant_and_forget_revokes_resume() {
        let t = tempfile::tempdir().unwrap();
        let base = t.path().canonicalize().unwrap();
        let s = state(&base.join("runtime"));
        let root = base.join("photos");
        fs::create_dir(&root).unwrap();
        let folder = grant(&s, root.clone()).unwrap();
        save(&s, Some(folder.id), view()).unwrap();
        let before = fs::read(s.data.join("workspace.json")).unwrap();
        fs::rename(&root, base.join("kept-photos")).unwrap();
        fs::create_dir(&root).unwrap();
        assert!(save(&s, Some(folder.id), view()).is_err());
        assert_eq!(fs::read(s.data.join("workspace.json")).unwrap(), before);
        forget(&s).unwrap();
        assert!(load(&s).unwrap().is_none());
        assert!(resume(&s).unwrap().is_none());
    }
    #[cfg(unix)]
    #[test]
    fn symlink_checkpoint_and_unreadable_source_are_not_resumed() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let t = tempfile::tempdir().unwrap();
        let s = state(t.path());
        let target = t.path().join("untouched");
        fs::write(&target, b"unchanged").unwrap();
        symlink(&target, t.path().join("workspace.json")).unwrap();
        assert!(save(&s, None, view()).is_err());
        assert!(load(&s).is_err());
        assert_eq!(fs::read(target).unwrap(), b"unchanged");
        let t = tempfile::tempdir().unwrap();
        let s = state(t.path());
        let root = t.path().join("photos");
        fs::create_dir(&root).unwrap();
        let f = grant(&s, root.clone()).unwrap();
        save(&s, Some(f.id), view()).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o000)).unwrap();
        let result = resume(&s);
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
    }
}
