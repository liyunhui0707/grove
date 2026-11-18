use super::*;
use crate::{
    exports::files::{self, Dir},
    imaging, resources, sidecar,
};
use objc2::rc::{Retained, autoreleasepool};
use objc2_foundation::{NSFileManager, NSString, NSURL};
use rusqlite::params;
use std::{
    collections::HashSet,
    fs,
    io::Read,
    os::unix::fs::MetadataExt,
    sync::atomic::Ordering,
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const LIMIT: usize = 100;
#[derive(Clone)]
struct FileCheck {
    name: String,
    identity: String,
    hash: String,
    bytes: u64,
}
struct Item {
    id: String,
    revision: i64,
    path: PathBuf,
    parent_key: String,
    files: Vec<FileCheck>,
}
pub struct Plan {
    token: String,
    root: PathBuf,
    root_key: String,
    items: Vec<Item>,
    report: Report,
}
fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::SeqCst) {
        Err("已停止核对，未开始移到废纸篓".into())
    } else {
        Ok(())
    }
}
fn fingerprint(d: &Dir, name: &str, cancel: &AtomicBool) -> Result<FileCheck> {
    check_cancel(cancel)?;
    let f = d.read(name)?.ok_or("文件已离线，请重新扫描")?;
    let m = f.metadata().map_err(|e| e.to_string())?;
    if m.nlink() != 1 {
        return Err("暂不处理硬链接；未移到废纸篓".into());
    }
    let identity = imaging::identity_from_file(&f)?;
    let (hash, bytes) = files::digest(&f, cancel)?;
    let after = f.metadata().map_err(|e| e.to_string())?;
    if m.modified().ok() != after.modified().ok() || m.len() != after.len() {
        return Err("文件内容已变化，请重新核对".into());
    }
    Ok(FileCheck {
        name: name.into(),
        identity,
        hash,
        bytes,
    })
}
fn verify(d: &Dir, f: &FileCheck, cancel: &AtomicBool) -> Result<()> {
    let now = fingerprint(d, &f.name, cancel)?;
    if now.identity != f.identity || now.hash != f.hash || now.bytes != f.bytes {
        return Err("文件身份或内容已变化，未继续移到废纸篓".into());
    }
    Ok(())
}
fn binding(c: &Catalog, root: &Path, folder: i64, i: &Item) -> Result<()> {
    let (f, p, rev, key) = c.location(&i.id)?;
    let rating: Option<i32> =
        c.db.query_row("SELECT rating FROM assets WHERE id=?1", [&i.id], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())?;
    if f != folder || Path::new(&p) != i.path || rev != i.revision || rating != Some(0) {
        return Err("只允许所选的 0 分照片；位置或评分已变化，请重新核对".into());
    }
    if !files::equivalent_identity(root, &i.path, &i.files[0].identity, &key)? {
        return Err("照片身份已变化，请重新扫描".into());
    }
    let conflicts: i64 =
        c.db.query_row(
            "SELECT count(*) FROM xmp_jobs WHERE asset_id=?1 AND status IN ('prepared','conflict')",
            [&i.id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let duplicates: i64 =
        c.db.query_row(
            "SELECT count(*) FROM assets WHERE path=?1 AND id<>?2",
            params![p, i.id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if conflicts != 0 || duplicates != 0 {
        return Err("该照片有未完成同步或重复目录记录，请先核对".into());
    }
    Ok(())
}
fn group(c: &Catalog, root: &Path, i: &Item) -> Result<()> {
    resources::refuse_incomplete_original_operation(&i.path, "移到废纸篓")?;
    let (_, _, _, key) = c.location(&i.id)?;
    let obs = sidecar::capture(root, &i.path, &key);
    if let Some(e) = obs.error {
        return Err(e);
    }
    let saved: (String, Option<String>, Option<String>) =
        c.db.query_row(
            "SELECT target,baseline,error FROM xmp_state WHERE id=?1",
            [&i.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|_| "请重新扫描，核对 XMP 状态")?;
    if saved.0 != obs.target || saved.1 != obs.xml || saved.2.is_some() {
        return Err("XMP 已变化或有冲突，请先核对评分同步".into());
    }
    if obs.xml.is_some() != (i.files.len() == 2)
        || (i.files.len() == 2 && obs.target != i.files[1].name)
    {
        return Err("关联 XMP 已变化，未拆开资源组".into());
    }
    Ok(())
}
pub fn prepare(
    c: &Catalog,
    roots: &HashMap<i64, PathBuf>,
    folder: i64,
    targets: &[Target],
    cancel: &AtomicBool,
) -> Result<Plan> {
    if targets.is_empty() || targets.len() > LIMIT {
        return Err("请明确选择 1–100 张 0 分照片；不会自动删除全部淘汰候选".into());
    }
    let root = roots
        .get(&folder)
        .ok_or("请先重新打开并授权照片文件夹")?
        .clone();
    let dir = Dir::open(&root)?;
    let mut ids = HashSet::new();
    let mut owned = HashSet::new();
    let mut items = Vec::new();
    let mut rows = Vec::new();
    for t in targets {
        check_cancel(cancel)?;
        if !ids.insert(&t.id) {
            return Err("重复选择同一照片".into());
        }
        let (_, path, _, _) = c.location(&t.id)?;
        let path = PathBuf::from(path);
        let relative = path
            .strip_prefix(&root)
            .map_err(|_| "照片不在授权文件夹内")?
            .to_string_lossy()
            .into_owned();
        if relative.is_empty()
            || path.components().any(|p| {
                matches!(
                    p,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
        {
            return Err("照片路径无效".into());
        }
        if !path.extension().is_some_and(|e| {
            ["jpg", "jpeg", "png", "webp", "heic", "heif"]
                .contains(&e.to_string_lossy().to_lowercase().as_str())
        }) {
            return Err("只处理支持的静态照片".into());
        }
        let parent = Dir::open(path.parent().ok_or("照片路径无效")?)?;
        let name = path
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or("文件名无效")?;
        let mut checked = vec![fingerprint(&parent, name, cancel)?];
        let (_, _, _, key) = c.location(&t.id)?;
        let obs = sidecar::capture(&root, &path, &key);
        if let Some(e) = obs.error {
            return Err(e);
        }
        if obs.xml.is_some() {
            checked.push(fingerprint(&parent, &obs.target, cancel)?);
        }
        for f in &checked {
            if !owned.insert(parent.path.join(&f.name)) {
                return Err("所选照片共享 XMP，请分批核对".into());
            }
        }
        let i = Item {
            id: t.id.clone(),
            revision: t.revision,
            path,
            parent_key: parent.key()?,
            files: checked,
        };
        binding(c, &root, folder, &i)?;
        group(c, &root, &i)?;
        rows.push(Row {
            name: relative,
            status: "pending".into(),
            resources: i
                .files
                .iter()
                .map(|f| ResourceResult {
                    name: f.name.clone(),
                    status: "pending".into(),
                    destination: None,
                })
                .collect(),
        });
        items.push(i);
    }
    Ok(Plan {
        token: Uuid::new_v4().to_string(),
        root: root.clone(),
        root_key: dir.key()?,
        items,
        report: Report {
            id: Uuid::new_v4().to_string(),
            created: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_secs(),
            source_folder: folder,
            source: root.to_string_lossy().into_owned(),
            status: "ready".into(),
            completed: 0,
            items: rows,
            message: None,
        },
    })
}
pub fn review(p: &Plan) -> Review {
    Review {
        token: p.token.clone(),
        report: p.report.clone(),
    }
}

// File-reference URLs follow the checked file occurrence, not a later replacement
// at the same path. Resolve and recheck the pinned file before invoking Foundation.
struct TrashFile {
    reference: Retained<NSURL>,
}
impl TrashFile {
    fn open(path: &Path, expected: &FileCheck) -> Result<Self> {
        let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str().ok_or("文件路径无效")?));
        let reference = url
            .fileReferenceURL()
            .ok_or("该卷不支持稳定文件引用，未移到废纸篓")?;
        let this = Self { reference };
        let resolved = this.path()?;
        if resolved != path || imaging::identity(&resolved)? != expected.identity {
            return Err("核对期间文件已移动或被替换".into());
        }
        Ok(this)
    }
    fn path(&self) -> Result<PathBuf> {
        self.reference
            .filePathURL()
            .and_then(|u| u.path())
            .map(|p| PathBuf::from(p.to_string()))
            .ok_or_else(|| "文件引用已失效".into())
    }
    fn trash(&self) -> Result<Option<String>> {
        let mut destination = None;
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&self.reference, Some(&mut destination))
            .map_err(|e| format!("系统未完成移到废纸篓：{e}"))?;
        Ok(destination
            .and_then(|u| u.filePathURL())
            .and_then(|u| u.path())
            .map(|p| p.to_string()))
    }
}
fn journal(data: &Path) -> Result<Dir> {
    let path = data.join("trash");
    if !path.exists() {
        fs::create_dir(&path).map_err(|e| e.to_string())?;
    }
    Dir::open(&path)
}
fn record(dir: &Dir, step: &mut usize, report: &Report) -> Result<()> {
    dir.write_record(
        &format!("{step:04}.json"),
        &serde_json::to_vec(report).map_err(|e| e.to_string())?,
    )?;
    *step += 1;
    Ok(())
}
fn execute(
    c: &Catalog,
    roots: &HashMap<i64, PathBuf>,
    mut p: Plan,
    data: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(Report),
    mut trash: impl FnMut(&Path, &FileCheck) -> Result<Option<String>>,
) -> Result<Report> {
    check_cancel(cancel)?;
    if p.root.starts_with(data) || data.starts_with(&p.root) {
        return Err("不能在应用资料目录或其上级目录执行此操作，请选择独立照片文件夹".into());
    }
    if roots.get(&p.report.source_folder) != Some(&p.root)
        || !Dir::open(&p.root)?.matches_key(&p.root_key)?
    {
        return Err("源文件夹授权或身份已变化，未开始移到废纸篓".into());
    }
    for i in &p.items {
        binding(c, &p.root, p.report.source_folder, i)?;
        group(c, &p.root, i)?;
        let d = Dir::open(i.path.parent().ok_or("照片路径无效")?)?;
        if !d.matches_key(&i.parent_key)? {
            return Err("照片父目录已被替换".into());
        }
        for f in &i.files {
            verify(&d, f, cancel)?;
        }
    }
    // No catalog rows are deleted: Finder restoration + rescan retains ratings.
    crate::backups::create(c, &data.join("backups"))?;
    let log = journal(data)?.create(&p.report.id)?;
    let mut step = 0;
    p.report.status = "running".into();
    record(&log, &mut step, &p.report)?;
    progress(p.report.clone());
    let operation = (|| -> Result<()> {
        for (n, i) in p.items.iter().enumerate() {
            check_cancel(cancel)?;
            binding(c, &p.root, p.report.source_folder, i)?;
            group(c, &p.root, i)?;
            let d = Dir::open(i.path.parent().ok_or("照片路径无效")?)?;
            if !Dir::open(&p.root)?.matches_key(&p.root_key)? || !d.matches_key(&i.parent_key)? {
                return Err("文件夹身份已变化，已停止".into());
            }
            // Finish the photo/XMP group before honoring a stop request.
            let no_cancel = AtomicBool::new(false);
            for (k, f) in i.files.iter().enumerate() {
                d.unchanged()?;
                verify(&d, f, &no_cancel)?;
                p.report.items[n].status = "attention".into();
                p.report.items[n].resources[k].status = "requesting".into();
                record(&log, &mut step, &p.report)?;
                let destination = trash(&d.path.join(&f.name), f)?;
                let result = &mut p.report.items[n].resources[k];
                result.status = "trashed".into();
                result.destination = destination;
                record(&log, &mut step, &p.report)?;
            }
            p.report.items[n].status = "trashed".into();
            p.report.completed += 1;
            record(&log, &mut step, &p.report)?;
            progress(p.report.clone());
        }
        Ok(())
    })();
    match operation {
        Ok(()) => p.report.status = "completed".into(),
        Err(e) => {
            p.report.status = "stopped".into();
            p.report.message = Some(format!(
                "{e}。已保留记录；请核对原目录和废纸篓，不会自动重试。"
            ));
        }
    }
    record(&log, &mut step, &p.report)?;
    progress(p.report.clone());
    Ok(p.report)
}
pub fn run(
    c: &Catalog,
    roots: &HashMap<i64, PathBuf>,
    p: Plan,
    data: &Path,
    cancel: &AtomicBool,
    progress: impl FnMut(Report),
) -> Result<Report> {
    autoreleasepool(|_| {
        // Obtain every stable reference before any file is moved.
        let mut references = HashMap::new();
        for i in &p.items {
            for f in &i.files {
                let path = i.path.parent().ok_or("文件路径无效")?.join(&f.name);
                references.insert(path.clone(), TrashFile::open(&path, f)?);
            }
        }
        execute(c, roots, p, data, cancel, progress, |path, f| {
            let reference = references.get(path).ok_or("稳定引用缺失")?;
            if reference.path()? != path || imaging::identity(path)? != f.identity {
                return Err("文件已移动或被替换，已停止".into());
            }
            reference.trash()
        })
    })
}
pub fn history(data: &Path) -> Result<Vec<Report>> {
    let root = journal(data)?;
    let mut reports = Vec::new();
    for entry in fs::read_dir(&root.path)
        .map_err(|e| e.to_string())?
        .take(1000)
    {
        let entry = entry.map_err(|e| e.to_string())?;
        if Uuid::parse_str(&entry.file_name().to_string_lossy()).is_err() {
            continue;
        }
        let dir = Dir::open(&entry.path())?;
        let mut steps = fs::read_dir(&dir.path)
            .map_err(|e| e.to_string())?
            .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        steps.retain(|n| {
            n.len() == 9 && n.ends_with(".json") && n.as_bytes()[..4].iter().all(u8::is_ascii_digit)
        });
        steps.sort();
        if let Some(last) = steps.last() {
            let file = dir.read(last)?.ok_or("废纸篓记录缺失")?;
            let mut bytes = Vec::new();
            file.take(1_048_577)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > 1_048_576 {
                return Err("废纸篓记录过大，请保留现场".into());
            }
            let mut r: Report =
                serde_json::from_slice(&bytes).map_err(|_| "废纸篓记录损坏，请保留现场")?;
            if r.status == "running" {
                r.status = "interrupted".into();
                r.message = Some("上次操作中断。标为待核对的文件可能已移到废纸篓，请核对原目录与废纸篓；不会自动重试。".into());
            }
            reports.push(r);
        }
    }
    reports.sort_by_key(|r| std::cmp::Reverse(r.created));
    reports.truncate(20);
    Ok(reports)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
