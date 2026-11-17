use super::*;
use crate::{
    backups,
    exports::files::{self, Dir, digest},
    imaging, sidecar,
};
use rusqlite::{Connection, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    os::unix::fs::MetadataExt,
    sync::atomic::Ordering,
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;
const LIMIT: usize = 100;
const RECORD_LIMIT: usize = 1_048_576;
#[derive(Clone, Serialize, Deserialize)]
struct Resource {
    name: String,
    stage: String,
    identity: String,
    hash: String,
    bytes: u64,
}
#[derive(Clone, Serialize, Deserialize)]
struct Item {
    id: String,
    path: PathBuf,
    relative: String,
    parent_key: String,
    rating: Option<i32>,
    revision: i64,
    resources: Vec<Resource>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Job {
    format: u32,
    id: String,
    created: u64,
    root: PathBuf,
    root_key: String,
    folder: i64,
    parent: PathBuf,
    parent_key: String,
    directory: String,
    kind: String,
    items: Vec<Item>,
}
#[derive(Clone, Serialize, Deserialize)]
struct State {
    step: u64,
    direction: String,
    status: String,
    directory_key: Option<String>,
    snapshot: Option<String>,
    rows: Vec<(String, Option<String>)>,
    message: Option<String>,
}
pub struct Plan {
    job: Job,
    state: State,
    token: String,
    existing: bool,
    restore: bool,
}
pub struct Access {
    _lock: fs::File,
}
fn logs(data: &Path) -> Result<Dir> {
    let d = Dir::open(data)?;
    if !data.join("moves").exists() {
        match d.create("moves") {
            Ok(x) => return Ok(x),
            Err(_) if data.join("moves").exists() => {}
            Err(e) => return Err(e),
        }
    }
    Dir::open(&data.join("moves"))
}
fn record(d: &Dir, name: &str) -> Result<Vec<u8>> {
    use std::io::Read;
    let f = d.read(name)?.ok_or("安全整理记录缺失，请保留现场")?;
    if f.metadata().map_err(|e| e.to_string())?.len() > RECORD_LIMIT as u64 {
        return Err("安全整理记录超过上限".into());
    }
    let mut b = Vec::new();
    f.take(RECORD_LIMIT as u64 + 1)
        .read_to_end(&mut b)
        .map_err(|e| e.to_string())?;
    if b.len() > RECORD_LIMIT {
        return Err("安全整理记录超过上限".into());
    }
    Ok(b)
}
fn decode<T: serde::de::DeserializeOwned>(d: &Dir, name: &str) -> Result<T> {
    serde_json::from_slice(&record(d, name)?).map_err(|_| "安全整理记录损坏，请保留现场".into())
}
fn ids(data: &Path) -> Result<Vec<String>> {
    let d = logs(data)?;
    let mut out = Vec::new();
    for e in fs::read_dir(&d.path).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        let n = e.file_name().to_string_lossy().into_owned();
        if n == "operation.lock" || n.starts_with('.') {
            continue;
        }
        if Uuid::parse_str(&n).is_err() || !e.file_type().map_err(|e| e.to_string())?.is_dir() {
            return Err("安全整理日志目录异常，请保留现场".into());
        }
        if e.path().join("plan.json").exists() {
            out.push(n)
        }
        if out.len() > 1000 {
            return Err("安全整理历史超过首版上限，请先归档核对".into());
        }
    }
    Ok(out)
}
fn initial(j: &Job) -> State {
    State {
        step: 0,
        direction: "forward".into(),
        status: "planned".into(),
        directory_key: None,
        snapshot: None,
        rows: vec![("pending".into(), None); j.items.len()],
        message: None,
    }
}
fn latest(d: &Dir) -> Result<Option<String>> {
    let mut names = Vec::new();
    for e in fs::read_dir(&d.path).map_err(|e| e.to_string())? {
        let n = e
            .map_err(|e| e.to_string())?
            .file_name()
            .to_string_lossy()
            .into_owned();
        if n.len() == 19
            && n.starts_with("state-")
            && n.ends_with(".json")
            && n.as_bytes()[6..14].iter().all(u8::is_ascii_digit)
        {
            names.push(n)
        }
    }
    names.sort();
    Ok(names.pop())
}
fn leaf(name: &str) -> Result<()> {
    if name.is_empty()
        || Path::new(name).components().count() != 1
        || !matches!(
            Path::new(name).components().next(),
            Some(std::path::Component::Normal(_))
        )
    {
        Err("文件名无效".into())
    } else {
        Ok(())
    }
}
fn leaf_name(p: &Path) -> Result<String> {
    p.file_name()
        .and_then(|s| s.to_str())
        .map(str::to_owned)
        .ok_or("文件名无效".into())
}
fn validate_job(j: &Job, id: &str) -> Result<()> {
    if j.format != 1
        || j.id != id
        || !["move", "quarantine"].contains(&j.kind.as_str())
        || j.items.is_empty()
        || j.items.len() > LIMIT
        || !j.root.is_absolute()
        || !j.parent.is_absolute()
        || j.directory
            != format!(
                "Moment-Grove-{}-{}-{}",
                if j.kind == "quarantine" {
                    "Quarantine"
                } else {
                    "Move"
                },
                j.created,
                &j.id[..8]
            )
    {
        return Err("安全整理计划无效".into());
    }
    let mut seen = HashSet::new();
    let mut names = HashSet::new();
    let mut stems = HashSet::new();
    for (n, i) in j.items.iter().enumerate() {
        let rel = i
            .path
            .strip_prefix(&j.root)
            .map_err(|_| "安全整理路径无效")?;
        if rel
            .components()
            .any(|p| !matches!(p, std::path::Component::Normal(_)))
            || rel.to_string_lossy() != i.relative
            || i.resources.is_empty()
            || i.resources.len() > 2
            || i.resources[0].name != leaf_name(&i.path)?
            || !seen.insert(&i.id)
            || !stems.insert(
                i.path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase(),
            )
        {
            return Err("安全整理条目无效".into());
        }
        for (k, r) in i.resources.iter().enumerate() {
            leaf(&r.name)?;
            if !names.insert(r.name.to_lowercase())
                || r.stage != format!(".moment-grove-move-{}-{n}-{k}", j.id)
                || r.hash.len() != 64
                || !r.hash.bytes().all(|x| x.is_ascii_hexdigit())
            {
                return Err("安全整理资源无效".into());
            }
        }
    }
    Ok(())
}
fn load(data: &Path, id: &str) -> Result<(Job, State)> {
    if Uuid::parse_str(id).is_err() {
        return Err("安全整理标识无效".into());
    }
    let d = Dir::open(&logs(data)?.path.join(id))?;
    let j: Job = decode(&d, "plan.json")?;
    validate_job(&j, id)?;
    let s = match latest(&d)? {
        Some(n) => decode::<State>(&d, &n)?,
        None => initial(&j),
    };
    if s.rows.len() != j.items.len()
        || !["forward", "restore"].contains(&s.direction.as_str())
        || ![
            "planned",
            "running",
            "paused",
            "partial",
            "completed",
            "restored",
        ]
        .contains(&s.status.as_str())
        || s.step > 99_999_999
    {
        return Err("安全整理进度无效".into());
    }
    Ok((j, s))
}
fn save(d: &Dir, s: &mut State) -> Result<()> {
    s.step = s
        .step
        .checked_add(1)
        .filter(|n| *n <= 99_999_999)
        .ok_or("安全整理进度超过上限")?;
    let b = serde_json::to_vec(s).map_err(|e| e.to_string())?;
    if b.len() > RECORD_LIMIT {
        return Err("安全整理进度超过上限".into());
    }
    d.write_record(&format!("state-{:08}.json", s.step), &b)
}
fn settled(s: &State) -> bool {
    ["completed", "restored"].contains(&s.status.as_str())
}
// Frequent rating/scan entry points need only the durable status, not all immutable media plans.
fn status(data: &Path, id: &str) -> Result<String> {
    let d = Dir::open(&logs(data)?.path.join(id))?;
    let Some(name) = latest(&d)? else {
        return Ok("planned".into());
    };
    let state: State = decode(&d, &name)?;
    if state.rows.is_empty()
        || state.rows.len() > LIMIT
        || !["forward", "restore"].contains(&state.direction.as_str())
        || ![
            "planned",
            "running",
            "paused",
            "partial",
            "completed",
            "restored",
        ]
        .contains(&state.status.as_str())
    {
        return Err("安全整理状态损坏，请保留现场".into());
    }
    Ok(state.status)
}
fn ensure_idle(data: &Path, except: Option<&str>) -> Result<()> {
    for id in ids(data)? {
        if Some(id.as_str()) != except
            && !["completed", "restored"].contains(&status(data, &id)?.as_str())
        {
            return Err("存在未完成的安全整理，请先在“安全整理”中继续或撤回".into());
        }
    }
    Ok(())
}
pub fn access(data: &Path, allow_unfinished: bool) -> Result<Access> {
    let lock = logs(data)?
        .lock()
        .map_err(|_| "文件操作仍在处理，请稍后重试")?;
    if !allow_unfinished {
        ensure_idle(data, None)?
    }
    Ok(Access { _lock: lock })
}
pub fn restore_catalog_allowed(data: &Path) -> Result<()> {
    for id in ids(data)? {
        if status(data, &id)? != "restored" {
            return Err("请先撤回安全整理再恢复资料库；资料库快照不会搬回照片".into());
        }
    }
    Ok(())
}
pub fn catalog_restore_bindings(c: &Catalog, data: &Path) -> Result<Vec<(String, String, String)>> {
    restore_catalog_allowed(data)?;
    let mut seen = HashSet::new();
    let mut current = Vec::new();
    for id in ids(data)? {
        let (job, _) = load(data, &id)?;
        for item in job.items {
            if seen.insert(item.id.clone())
                && let Ok((_, path, _, identity)) = c.location(&item.id)
            {
                current.push((item.id, path, identity));
            }
        }
    }
    Ok(current)
}
fn report(j: &Job, s: &State, token: Option<String>) -> Report {
    Report {
        id: j.id.clone(),
        review_token: token,
        source: j.root.to_string_lossy().into_owned(),
        source_folder: j.folder,
        destination: j.parent.join(&j.directory).to_string_lossy().into_owned(),
        kind: j.kind.clone(),
        direction: s.direction.clone(),
        status: s.status.clone(),
        verified: s.rows.iter().filter(|(v, _)| v == "verified").count(),
        items: j
            .items
            .iter()
            .zip(&s.rows)
            .map(|(i, (status, error))| Row {
                name: i.relative.clone(),
                output: i.resources[0].name.clone(),
                rating: i.rating,
                sidecar: i.resources.len() == 2,
                status: status.clone(),
                error: error.clone(),
            })
            .collect(),
        safety_backup: s.snapshot.clone(),
        message: s.message.clone(),
    }
}
pub fn review(p: &Plan) -> Report {
    let mut s = p.state.clone();
    if p.restore {
        s.direction = "restore".into()
    }
    s.status = "ready".into();
    report(&p.job, &s, Some(p.token.clone()))
}
pub fn history(data: &Path) -> Result<Vec<Report>> {
    let mut jobs = ids(data)?
        .iter()
        .map(|id| load(data, id))
        .collect::<Result<Vec<_>>>()?;
    jobs.sort_by_key(|(j, _)| std::cmp::Reverse(j.created));
    Ok(jobs
        .into_iter()
        .enumerate()
        .filter(|(n, (_, s))| *n < 20 || !settled(s))
        .map(|(_, (j, s))| report(&j, &s, None))
        .collect())
}
pub fn locations(p: &Plan) -> (PathBuf, PathBuf) {
    (p.job.root.clone(), p.job.parent.clone())
}
fn fingerprint(dir: &Dir, name: &str, stage: String, cancel: &AtomicBool) -> Result<Resource> {
    let file = dir.read(name)?.ok_or("源文件已离线")?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if meta.nlink() != 1 {
        return Err("首版不移动硬链接，请使用独立测试副本".into());
    }
    let identity = imaging::identity_from_file(&file)?;
    let (hash, bytes) = digest(&file, cancel)?;
    let after = file.metadata().map_err(|e| e.to_string())?;
    if meta.modified().ok() != after.modified().ok() || meta.len() != after.len() {
        return Err("文件在核对时发生变化".into());
    }
    Ok(Resource {
        name: name.into(),
        stage,
        identity,
        hash,
        bytes,
    })
}
fn verify(dir: &Dir, name: &str, r: &Resource, cancel: &AtomicBool) -> Result<bool> {
    let Some(f) = dir.read(name)? else {
        return Ok(false);
    };
    let m = f.metadata().map_err(|e| e.to_string())?;
    if m.nlink() != 1 || !imaging::matches_file_identity(&f, &r.identity)? {
        return Err("文件身份已变化，未覆盖或继续移动".into());
    }
    let (h, b) = digest(&f, cancel)?;
    if h != r.hash
        || b != r.bytes
        || m.modified().ok() != f.metadata().map_err(|e| e.to_string())?.modified().ok()
    {
        return Err("照片或XMP内容已变化，保留现场".into());
    }
    Ok(true)
}
fn row_binding(db: &Connection, i: &Item, j: &Job, strict: bool) -> Result<String> {
    let (f, p, rev, key): (i64, String, i64, String) = db
        .query_row(
            "SELECT folder_id,path,revision,identity FROM assets WHERE id=?1",
            [&i.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(|_| "照片记录缺失")?;
    let dest = j.parent.join(&j.directory).join(&i.resources[0].name);
    if !((Path::new(&p) == i.path && f == j.folder) || Path::new(&p) == dest)
        || (strict && rev != i.revision)
    {
        return Err("照片位置或评分修订已变化，请重新核对".into());
    }
    if key != i.resources[0].identity {
        let staged = i
            .path
            .parent()
            .ok_or("照片路径无效")?
            .join(&i.resources[0].stage);
        let destination = j.parent.join(&j.directory);
        let matches = [
            (&j.root, &i.path),
            (&j.root, &staged),
            (&destination, &dest),
        ]
        .into_iter()
        .any(|(root, path)| {
            files::equivalent_identity(root, path, &i.resources[0].identity, &key).unwrap_or(false)
        });
        if !matches {
            return Err("照片身份已变化，未关联旧整理记录".into());
        }
    }
    let duplicates: i64 = db
        .query_row(
            "SELECT count(*) FROM assets WHERE path IN (?1,?2) AND id<>?3",
            params![i.path.to_str(), dest.to_str(), i.id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if duplicates != 0 {
        return Err("照片在其他目录视图中也有记录，暂不合并移动".into());
    }
    let active: i64 = db
        .query_row(
            "SELECT count(*) FROM xmp_jobs WHERE asset_id=?1 AND status IN ('prepared','conflict')",
            [&i.id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if active != 0 {
        return Err("该照片有未完成XMP任务，请先处理评分同步".into());
    }
    Ok(key)
}
fn parent_dirs(j: &Job, s: &State) -> Result<(Dir, Option<Dir>)> {
    let root = Dir::open(&j.root)?;
    let parent = Dir::open(&j.parent)?;
    if !root.matches_key(&j.root_key)? || !parent.matches_key(&j.parent_key)? {
        return Err("源或目标父目录身份已变化，请保留现场".into());
    }
    let dest = if let Some(key) = &s.directory_key {
        let d = Dir::open(&j.parent.join(&j.directory))?;
        if !d.matches_key(key)? {
            return Err("整理目录被替换，未移动文件".into());
        }
        Some(d)
    } else {
        None
    };
    Ok((parent, dest))
}
fn unplanned_xmp(i: &Item, d: &Dir) -> Result<()> {
    let full = format!("{}.xmp", i.resources[0].name);
    let stem = Path::new(&i.resources[0].name)
        .with_extension("xmp")
        .to_string_lossy()
        .into_owned();
    for name in [full, stem] {
        if !i.resources.iter().any(|r| r.name == name) && d.read(&name)?.is_some() {
            return Err("出现计划外XMP，请先核对，未拆开资源组".into());
        }
    }
    Ok(())
}
fn paired(p: &Path) -> Result<()> {
    crate::resources::refuse_incomplete_original_operation(p, "移动")
}
fn positions(j: &Job, s: &State, i: &Item) -> Result<(Dir, Option<Dir>, Vec<u8>)> {
    positions_with_cancel(j, s, i, &AtomicBool::new(false))
}
fn positions_with_cancel(
    j: &Job,
    s: &State,
    i: &Item,
    cancel: &AtomicBool,
) -> Result<(Dir, Option<Dir>, Vec<u8>)> {
    let (_, dest) = parent_dirs(j, s)?;
    let source = Dir::open(i.path.parent().ok_or("源父目录无效")?)?;
    if !source.matches_key(&i.parent_key)? {
        return Err("照片父目录被替换，未移动文件".into());
    }
    paired(&i.path)?;
    unplanned_xmp(i, &source)?;
    if let Some(d) = &dest {
        paired(&d.path.join(&i.resources[0].name))?;
        unplanned_xmp(i, d)?
    }
    let mut where_ = Vec::new();
    for r in &i.resources {
        let a = verify(&source, &r.name, r, cancel)?;
        let t = verify(&source, &r.stage, r, cancel)?;
        let b = dest
            .as_ref()
            .map(|d| verify(d, &r.name, r, cancel))
            .transpose()?
            .unwrap_or(false);
        if u8::from(a) + u8::from(t) + u8::from(b) != 1 {
            return Err("原名、暂存与目标位置有冲突或缺失；未覆盖".into());
        }
        where_.push(if a {
            0
        } else if t {
            1
        } else {
            2
        })
    }
    Ok((source, dest, where_))
}
fn preflight(db: &Connection, j: &Job, s: &State, strict: bool) -> Result<()> {
    preflight_with_progress(
        db,
        j,
        s,
        strict,
        &mut crate::preparation::Review::new(&AtomicBool::new(false), |_| {}),
    )
}
fn preflight_with_progress(
    db: &Connection,
    j: &Job,
    s: &State,
    strict: bool,
    review: &mut crate::preparation::Review<'_>,
) -> Result<()> {
    review.begin("rechecking", j.items.len())?;
    parent_dirs(j, s)?;
    for i in &j.items {
        review.item(&i.relative)?;
        row_binding(db, i, j, strict).map_err(|e| format!("{}：{e}", i.relative))?;
        positions_with_cancel(j, s, i, review.cancel)
            .map_err(|e| format!("{}：{e}", i.relative))?;
        review.done()?;
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
pub fn prepare_with_progress(
    c: &Catalog,
    roots: &HashMap<i64, PathBuf>,
    folder: i64,
    targets: &[Target],
    parent: &Path,
    data: &Path,
    kind: &str,
    review: &mut crate::preparation::Review<'_>,
) -> Result<Plan> {
    ensure_idle(data, None)?;
    if !["move", "quarantine"].contains(&kind) || targets.is_empty() || targets.len() > LIMIT {
        return Err("请选择1–100张测试照片".into());
    }
    review.begin("checking", targets.len())?;
    let root = roots.get(&folder).ok_or("请先打开源照片目录")?.clone();
    let root_dir = Dir::open(&root)?;
    let dest_parent = Dir::open(parent)?;
    let dest_device = dest_parent
        .file
        .metadata()
        .map_err(|e| e.to_string())?
        .dev();
    if root_dir.file.metadata().map_err(|e| e.to_string())?.dev() != dest_device {
        return Err("首版仅支持同卷移动，不会退回复制后删除".into());
    }
    let id = Uuid::new_v4().to_string();
    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let directory = format!(
        "Moment-Grove-{}-{created}-{}",
        if kind == "quarantine" {
            "Quarantine"
        } else {
            "Move"
        },
        &id[..8]
    );
    let destination = parent.join(&directory);
    let mut ancestor_keys = HashSet::new();
    for p in parent.ancestors() {
        ancestor_keys.insert(Dir::open(p)?.key()?);
    }
    let mut q =
        c.db.prepare("SELECT path FROM folders")
            .map_err(|e| e.to_string())?;
    for p in q
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
    {
        let p = PathBuf::from(p.map_err(|e| e.to_string())?);
        if destination.starts_with(&p)
            || p.starts_with(&destination)
            || Dir::open(&p)
                .and_then(|d| d.key())
                .is_ok_and(|k| ancestor_keys.contains(&k))
        {
            return Err("目标必须在已登记照片目录之外".into());
        }
    }
    if root.starts_with(data)
        || destination.starts_with(data)
        || data.starts_with(&destination)
        || ancestor_keys.contains(&Dir::open(data)?.key()?)
    {
        return Err("安全整理不能操作应用数据目录，请选独立测试副本".into());
    }
    let mut seen = HashSet::new();
    let mut output_names = HashSet::new();
    let mut stems = HashSet::new();
    let mut items = Vec::new();
    for (n, t) in targets.iter().enumerate() {
        if !seen.insert(t.id.clone()) {
            return Err("重复选择同一照片".into());
        }
        let (f, p, revision, key) = c.location(&t.id)?;
        if f != folder || revision != t.revision {
            return Err("选择或评分修订已变化".into());
        }
        let p = PathBuf::from(p);
        let relative = p.strip_prefix(&root).map_err(|_| "照片不在授权目录")?;
        if relative
            .components()
            .any(|p| !matches!(p, std::path::Component::Normal(_)))
        {
            return Err("照片路径无效".into());
        }
        let name = leaf_name(&p)?;
        review.item(&relative.to_string_lossy())?;
        if !stems.insert(
            p.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase(),
        ) {
            return Err("所选照片同名主干可能共享XMP，请分批整理；不会自动改名".into());
        }
        if !p.extension().is_some_and(|e| {
            ["jpg", "jpeg", "png", "webp", "heic", "heif"]
                .contains(&e.to_string_lossy().to_lowercase().as_str())
        }) {
            return Err("首版安全整理仅支持静态图片".into());
        }
        let rating: Option<i32> =
            c.db.query_row("SELECT rating FROM assets WHERE id=?1", [&t.id], |r| {
                r.get(0)
            })
            .map_err(|e| e.to_string())?;
        if kind == "quarantine" && !matches!(rating, Some(0 | 1)) {
            return Err("低分隔离只接收已确认的0或1分，未评分不会被隔离".into());
        }
        let parent_dir = Dir::open(p.parent().ok_or("照片路径无效")?)?;
        if parent_dir.file.metadata().map_err(|e| e.to_string())?.dev() != dest_device {
            return Err("照片所在目录不在目标同卷".into());
        }
        let mut resources = vec![fingerprint(
            &parent_dir,
            &name,
            format!(".moment-grove-move-{id}-{n}-0"),
            review.cancel,
        )?];
        if !files::equivalent_identity(&root, &p, &resources[0].identity, &key)? {
            return Err("照片身份已变化".into());
        }
        let obs = sidecar::capture(&root, &p, &key);
        if let Some(e) = obs.error {
            return Err(e);
        }
        let saved: (String, Option<String>, Option<String>) =
            c.db.query_row(
                "SELECT target,baseline,error FROM xmp_state WHERE id=?1",
                [&t.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|_| "请重新扫描源目录建立XMP基线")?;
        if saved.0 != obs.target || saved.1 != obs.xml || saved.2.is_some() {
            return Err("源XMP与已知基线有冲突，请先核对评分同步".into());
        }
        if obs.xml.is_some() {
            resources.push(fingerprint(
                &parent_dir,
                &obs.target,
                format!(".moment-grove-move-{id}-{n}-1"),
                review.cancel,
            )?)
        }
        for r in &resources {
            if !output_names.insert(r.name.to_lowercase()) {
                return Err("所选照片或XMP同名，请分批整理；不会自动改名".into());
            }
        }
        items.push(Item {
            id: t.id.clone(),
            path: p.clone(),
            relative: relative.to_string_lossy().into_owned(),
            parent_key: parent_dir.key()?,
            rating,
            revision,
            resources,
        });
        review.done()?;
    }
    let job = Job {
        format: 1,
        id,
        created,
        root,
        root_key: root_dir.key()?,
        folder,
        parent: parent.into(),
        parent_key: dest_parent.key()?,
        directory,
        kind: kind.into(),
        items,
    };
    let state = initial(&job);
    preflight_with_progress(&c.db, &job, &state, true, review)?;
    Ok(Plan {
        job,
        state,
        token: Uuid::new_v4().to_string(),
        existing: false,
        restore: false,
    })
}
pub fn resume_with_progress(
    c: &Catalog,
    data: &Path,
    id: &str,
    restore: bool,
    review: &mut crate::preparation::Review<'_>,
) -> Result<Plan> {
    review.check()?;
    ensure_idle(data, Some(id))?;
    let (job, state) = load(data, id)?;
    if state.status == "restored" {
        return Err("该操作已经撤回".into());
    }
    let restore = restore || state.direction == "restore";
    preflight_with_progress(&c.db, &job, &state, !restore, review)?;
    Ok(Plan {
        job,
        state,
        token: Uuid::new_v4().to_string(),
        existing: true,
        restore,
    })
}
fn rename(from: &Dir, a: &str, to: &Dir, b: &str) -> Result<()> {
    leaf(a)?;
    leaf(b)?;
    from.unchanged()?;
    to.unchanged()?;
    rustix::fs::renameat_with(
        &from.file,
        a,
        &to.file,
        b,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(|e| format!("移动暂停且未覆盖目标：{e}"))?;
    from.sync()?;
    to.sync()?;
    Ok(())
}
fn shift(
    j: &Job,
    s: &State,
    i: &Item,
    restore: bool,
    hook: &mut impl FnMut(&str) -> Result<()>,
) -> Result<()> {
    let cancel = AtomicBool::new(false); // Finish the current resource group before an execution stop.
    let (source, dest, where_) = positions(j, s, i)?;
    for (r, at) in i.resources.iter().zip(where_) {
        let target = if restore { 0 } else { 2 };
        if at == target {
            continue;
        }
        if at != 1 {
            let from = if at == 0 {
                &source
            } else {
                dest.as_ref().ok_or("缺少整理目录")?
            };
            if !verify(from, &r.name, r, &cancel)? {
                return Err("源文件已离线".into());
            }
            hook("before_claim")?;
            rename(from, &r.name, &source, &r.stage)?;
            hook("claimed")?;
        }
        if !verify(&source, &r.stage, r, &cancel)? {
            return Err("暂存文件缺失".into());
        }
        let to = if restore {
            &source
        } else {
            dest.as_ref().ok_or("缺少整理目录")?
        };
        rename(&source, &r.stage, to, &r.name)?;
        hook("published")?;
        if !verify(to, &r.name, r, &cancel)? {
            return Err("移动后核验失败，保留现场".into());
        }
    }
    Ok(())
}
fn bind(db: &Connection, j: &Job, i: &Item, restore: bool, destination_folder: i64) -> Result<()> {
    let key = row_binding(db, i, j, false)?;
    let dest = j.parent.join(&j.directory).join(&i.resources[0].name);
    let to = if restore { &i.path } else { &dest };
    let folder = if restore {
        j.folder
    } else {
        destination_folder
    };
    if db
        .execute(
            "UPDATE assets SET path=?1,folder_id=?2 WHERE id=?3 AND identity=?4",
            params![to.to_str(), folder, i.id, key],
        )
        .map_err(|e| e.to_string())?
        != 1
    {
        return Err("照片关联已变化".into());
    }
    db.execute(
        "UPDATE xmp_jobs SET source=?1 WHERE asset_id=?2 AND source IN (?3,?4)",
        params![to.to_str(), i.id, i.path.to_str(), dest.to_str()],
    )
    .map_err(|e| e.to_string())?;
    db.execute(
        "UPDATE xmp_state SET seen_token=NULL,seen_target=NULL,seen_xml=NULL WHERE id=?1",
        [&i.id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
pub fn run(
    c: &mut Catalog,
    mut p: Plan,
    data: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(Report),
    mut hook: impl FnMut(&str) -> Result<()>,
) -> Result<Report> {
    let _access = access(data, true)?;
    ensure_idle(data, if p.existing { Some(&p.job.id) } else { None })?;
    if p.existing {
        let (j, s) = load(data, &p.job.id)?;
        if s.step != p.state.step {
            return Err("安全整理清单已过期，请重新核对".into());
        }
        p.job = j;
        p.state = s;
    }
    preflight(&c.db, &p.job, &p.state, !p.restore)?;
    let backup = backups::create(c, &data.join("backups"))?;
    let all = logs(data)?;
    let journal = if p.existing {
        Dir::open(&all.path.join(&p.job.id))?
    } else {
        let d = all.create(&p.job.id)?;
        let b = serde_json::to_vec(&p.job).map_err(|e| e.to_string())?;
        if b.len() > RECORD_LIMIT {
            return Err("安全整理清单超过1MiB".into());
        }
        d.write_record("plan.json", &b)?;
        d
    };
    p.state.snapshot = Some(backup.id);
    p.state.direction = if p.restore { "restore" } else { "forward" }.into();
    p.state.status = "running".into();
    p.state.message = None;
    p.state.rows = vec![("pending".into(), None); p.job.items.len()];
    save(&journal, &mut p.state)?;
    hook("prepared")?;
    if !p.restore && p.state.directory_key.is_none() {
        let parent = Dir::open(&p.job.parent)?;
        let dest = parent.create(&p.job.directory)?;
        hook("directory_created")?;
        p.state.directory_key = Some(dest.key()?);
        save(&journal, &mut p.state)?;
    }
    let destination = p.job.parent.join(&p.job.directory);
    let tx =
        c.db.transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
    preflight(&tx, &p.job, &p.state, !p.restore)?;
    let destination_folder = if p.state.directory_key.is_some() {
        tx.execute(
            "INSERT OR IGNORE INTO folders(path) VALUES (?1)",
            [destination.to_str().ok_or("目录编码无效")?],
        )
        .map_err(|e| e.to_string())?;
        tx.query_row(
            "SELECT id FROM folders WHERE path=?1",
            [destination.to_str()],
            |r| r.get::<_, i64>(0),
        )
        .map_err(|e| e.to_string())?
    } else {
        p.job.folder
    };
    progress(report(&p.job, &p.state, None));
    let mut failed = false;
    for n in 0..p.job.items.len() {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        let i = &p.job.items[n];
        let result = shift(&p.job, &p.state, i, p.restore, &mut hook)
            .and_then(|_| bind(&tx, &p.job, i, p.restore, destination_folder));
        match result {
            Ok(()) => p.state.rows[n] = ("verified".into(), None),
            Err(e) => {
                p.state.rows[n] = ("error".into(), Some(e));
                failed = true;
            }
        }
        save(&journal, &mut p.state)?;
        progress(report(&p.job, &p.state, None));
        if failed {
            break;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    hook("catalog_committed")?;
    p.state.status = if p.state.rows.iter().all(|(s, _)| s == "verified") {
        if p.restore { "restored" } else { "completed" }
    } else if failed {
        "partial"
    } else {
        "paused"
    }
    .into();
    save(&journal, &mut p.state)?;
    let r = report(&p.job, &p.state, None);
    progress(r.clone());
    Ok(r)
}

#[cfg(test)]
pub fn prepare(
    c: &Catalog,
    roots: &HashMap<i64, PathBuf>,
    folder: i64,
    targets: &[Target],
    parent: &Path,
    data: &Path,
    kind: &str,
) -> Result<Plan> {
    prepare_with_progress(
        c,
        roots,
        folder,
        targets,
        parent,
        data,
        kind,
        &mut crate::preparation::Review::new(&AtomicBool::new(false), |_| {}),
    )
}
#[cfg(test)]
pub fn resume(c: &Catalog, data: &Path, id: &str, restore: bool) -> Result<Plan> {
    resume_with_progress(
        c,
        data,
        id,
        restore,
        &mut crate::preparation::Review::new(&AtomicBool::new(false), |_| {}),
    )
}
