use super::{Report, Row, Target};
use crate::{
    catalog::{Catalog, Result},
    sidecar, watermark, xmp,
};
#[path = "files.rs"]
pub(crate) mod files;
use files::{Dir, digest, hash_bytes};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const LIMIT: usize = 100;
const MAX_PLAN_BYTES: usize = 8 * 1024 * 1024;
#[derive(Clone, Serialize, Deserialize)]
struct Item {
    id: String,
    path: PathBuf,
    identity: String,
    relative: String,
    output: String,
    rating: Option<i32>,
    revision: i64,
    bytes: u64,
    sha256: String,
    xml: String,
    source_xmp_hash: Option<String>,
    source_xmp_target: String,
    status: String,
    error: Option<String>,
    #[serde(default)]
    info: Option<watermark::Info>,
    #[serde(skip)]
    derivative: Option<Derivative>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Derivative {
    sha256: String,
    bytes: u64,
    info: watermark::RenderInfo,
}
#[derive(Clone, Serialize, Deserialize)]
struct Job {
    format: u32,
    id: String,
    folder: i64,
    root: PathBuf,
    parent: PathBuf,
    parent_key: String,
    directory: String,
    directory_key: Option<String>,
    status: String,
    items: Vec<Item>,
    step: u64,
    message: Option<String>,
    #[serde(default)]
    watermark: Option<watermark::Options>,
}
pub struct Plan {
    job: Job,
    existing: bool,
}
#[derive(Serialize, Deserialize)]
struct Checkpoint {
    id: String,
    step: u64,
    directory_key: Option<String>,
    status: String,
    message: Option<String>,
    items: Vec<(String, Option<String>)>,
    #[serde(default)]
    derivatives: Vec<Option<Derivative>>,
}
pub fn review(p: &Plan) -> Report {
    report(&p.job)
}
fn report(j: &Job) -> Report {
    Report {
        id: j.id.clone(),
        destination: j.parent.join(&j.directory).to_string_lossy().into(),
        status: j.status.clone(),
        bytes: j.items.iter().map(|i| i.bytes + i.xml.len() as u64).sum(),
        verified: j.items.iter().filter(|i| i.status == "verified").count(),
        message: j.message.clone(),
        kind: if j.watermark.is_some() {
            "watermark"
        } else {
            "copy"
        }
        .into(),
        watermark: j.watermark.clone(),
        items: j
            .items
            .iter()
            .map(|i| Row {
                name: i.relative.clone(),
                output: i.output.clone(),
                rating: i.rating,
                bytes: i.derivative.as_ref().map(|d| d.bytes).unwrap_or(i.bytes),
                status: i.status.clone(),
                error: i.error.clone(),
                caption: i.info.as_ref().map(|info| info.text.clone()),
            })
            .collect(),
    }
}
fn safe_name(i: usize, path: &Path) -> String {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let mut safe = String::new();
    for c in name.chars() {
        let c = if c.is_control() || "\\/:<>*?\"|".contains(c) {
            '_'
        } else {
            c
        };
        if safe.len() + c.len_utf8() > 180 {
            break;
        }
        safe.push(c);
    }
    format!("{:03}-{}", i + 1, safe.trim_end_matches(['.', ' ']))
}
fn current(c: &Catalog, roots: &HashMap<i64, PathBuf>, j: &Job) -> Result<()> {
    current_with_progress(
        c,
        roots,
        j,
        &mut crate::preparation::Review::new(&AtomicBool::new(false), |_| {}),
    )
}
fn current_with_progress(
    c: &Catalog,
    roots: &HashMap<i64, PathBuf>,
    j: &Job,
    review: &mut crate::preparation::Review<'_>,
) -> Result<()> {
    review.begin("rechecking", j.items.len())?;
    if roots.get(&j.folder) != Some(&j.root) {
        return Err("请先重新打开原照片目录，授予读取权限".into());
    }
    for i in &j.items {
        review.item(&i.relative)?;
        let (f, path, rev, key) = c.location(&i.id)?;
        if f != j.folder
            || Path::new(&path) != i.path
            || rev != i.revision
            || !files::equivalent_identity(&j.root, &i.path, &i.identity, &key)?
        {
            return Err("照片位置或评分修订已变化，请重新创建导出清单".into());
        }
        review.done()?;
    }
    Ok(())
}
fn space(dir: &Dir, bytes: u64) -> Result<()> {
    if dir.available()? < bytes.saturating_add(1024 * 1024) {
        return Err("目标可用空间不足（含预留余量）".into());
    }
    Ok(())
}
#[cfg(test)]
pub fn prepare(
    c: &Catalog,
    roots: &HashMap<i64, PathBuf>,
    folder: i64,
    targets: &[Target],
    parent: &Path,
    data: &Path,
) -> Result<Plan> {
    prepare_with_progress(
        c,
        roots,
        folder,
        targets,
        parent,
        data,
        None,
        &mut crate::preparation::Review::new(&AtomicBool::new(false), |_| {}),
    )
}
#[cfg(test)]
pub fn prepare_watermark(
    c: &Catalog,
    roots: &HashMap<i64, PathBuf>,
    folder: i64,
    targets: &[Target],
    parent: &Path,
    data: &Path,
    options: &watermark::Options,
) -> Result<Plan> {
    prepare_with_progress(
        c,
        roots,
        folder,
        targets,
        parent,
        data,
        Some(options),
        &mut crate::preparation::Review::new(&AtomicBool::new(false), |_| {}),
    )
}
pub fn preview_watermark(
    c: &Catalog,
    roots: &HashMap<i64, PathBuf>,
    id: &str,
    options: &watermark::Options,
) -> Result<watermark::Preview> {
    use base64::Engine;
    let (folder, path, _, key) = c.location(id)?;
    let root = roots.get(&folder).ok_or("请先打开照片目录")?;
    let bytes = watermark::read_input(files::source(root, Path::new(&path), &key)?)?;
    let rendered = watermark::render(&bytes, options, Some(1280))?;
    let (export_width, export_height) = watermark::output_dimensions(
        rendered.info.source.width,
        rendered.info.source.height,
        options.output_max_edge,
    );
    Ok(watermark::Preview {
        export_width,
        export_height,
        can_export: rendered.info.source.date.is_some(),
        data_url: format!(
            "data:image/jpeg;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&rendered.jpeg)
        ),
        info: rendered.info,
    })
}
#[allow(clippy::too_many_arguments)]
fn prepare_kind(
    c: &Catalog,
    roots: &HashMap<i64, PathBuf>,
    folder: i64,
    targets: &[Target],
    parent: &Path,
    data: &Path,
    options: Option<watermark::Options>,
    review: &mut crate::preparation::Review<'_>,
) -> Result<Plan> {
    if !cfg!(target_os = "macos") {
        return Err("此平台复制导出尚未验证".into());
    }
    if targets.is_empty() || targets.len() > LIMIT {
        return Err("请选择1–100张照片，较大精选请分批导出".into());
    }
    review.begin("checking", targets.len())?;
    let root = roots.get(&folder).ok_or("照片目录尚未授权")?.clone();
    let parent = parent.canonicalize().map_err(|_| "目标父目录不可用")?;
    let dir = Dir::open(&parent)?;
    let id = uuid::Uuid::new_v4().to_string();
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let directory = format!(
        "Moment-Grove-{}{seconds}-{}",
        if options.is_some() { "Watermark-" } else { "" },
        &id[..8]
    );
    let destination = parent.join(&directory);
    // Directory identities also cover case aliases on case-insensitive volumes.
    let mut ancestor_keys = HashSet::new();
    for ancestor in parent.ancestors() {
        ancestor_keys.insert(Dir::open(ancestor)?.key()?);
    }
    let mut q =
        c.db.prepare("SELECT path FROM folders")
            .map_err(|e| e.to_string())?;
    let registered = q
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    for p in registered {
        let p = PathBuf::from(p.map_err(|e| e.to_string())?);
        let physical_match = Dir::open(&p)
            .and_then(|d| d.key())
            .is_ok_and(|key| ancestor_keys.contains(&key));
        if destination.starts_with(&p) || p.starts_with(&destination) || physical_match {
            return Err("请选择已登记照片目录之外的导出位置".into());
        }
    }
    if destination.starts_with(data)
        || data.starts_with(&destination)
        || ancestor_keys.contains(&Dir::open(data)?.key()?)
    {
        return Err("导出位置不能位于应用数据目录".into());
    }
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for (index, t) in targets.iter().enumerate() {
        if !seen.insert(&t.id) {
            return Err("导出选择含重复照片".into());
        }
        let (f, path, revision, identity) = c.location(&t.id)?;
        if f != folder || revision != t.revision {
            return Err("选择或评分修订已变化，请重新核对".into());
        }
        let path = PathBuf::from(path);
        review.item(&path.strip_prefix(&root).unwrap_or(&path).to_string_lossy())?;
        let source = files::source(&root, &path, &identity)?;
        if !path.extension().is_some_and(|e| {
            ["jpg", "jpeg", "png", "webp", "heic", "heif"]
                .contains(&e.to_string_lossy().to_lowercase().as_str())
        }) {
            return Err("此格式暂不支持复制导出".into());
        }
        if options.is_none() {
            crate::resources::refuse_incomplete_original_operation(&path, "复制导出")?;
        }
        let (sha256, bytes, info) = if let Some(o) = &options {
            let input = watermark::read_input(source)?;
            let info = watermark::inspect(&input, o)?;
            if info.date.is_none() {
                return Err(format!(
                    "{}：请补充备用日期后重新核对",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ));
            }
            (hash_bytes(&input), input.len() as u64, Some(info))
        } else {
            let (s, b) = digest(source, review.cancel)?;
            (s, b, None)
        };
        let rating =
            c.db.query_row("SELECT rating FROM assets WHERE id=?1", [&t.id], |r| {
                r.get::<_, Option<i32>>(0)
            })
            .map_err(|e| e.to_string())?;
        let captured = sidecar::capture(&root, &path, &identity);
        if let Some(e) = captured.error {
            return Err(e);
        }
        let xml = xmp::merge(
            if options.is_some() {
                None
            } else {
                captured.xml.as_deref()
            },
            rating,
        )?;
        let source_xmp_hash = captured.xml.as_ref().map(|s| hash_bytes(s.as_bytes()));
        let relative = path
            .strip_prefix(&root)
            .map_err(|_| "照片不在授权目录")?
            .to_str()
            .ok_or("文件名编码无效")?
            .to_string();
        items.push(Item {
            id: t.id.clone(),
            output: safe_name(
                index,
                &if options.is_some() {
                    path.with_extension("jpg")
                } else {
                    path.clone()
                },
            ),
            path,
            identity,
            relative,
            rating,
            revision,
            bytes,
            sha256,
            xml,
            source_xmp_hash,
            source_xmp_target: captured.target,
            status: "pending".into(),
            error: None,
            info,
            derivative: None,
        });
        review.done()?;
    }
    review.check()?;
    let job = Job {
        // Older apps must reject a resized/different-quality plan rather than
        // silently regenerate it using their fixed original-size settings.
        format: if options
            .as_ref()
            .is_some_and(|o| o.output_max_edge.is_some() || o.jpeg_quality != 92)
        {
            2
        } else {
            1
        },
        id,
        folder,
        root,
        parent,
        parent_key: dir.key()?,
        directory,
        directory_key: None,
        status: "ready".into(),
        items,
        step: 0,
        message: None,
        watermark: options,
    };
    if serde_json::to_vec(&job).map_err(|e| e.to_string())?.len() > MAX_PLAN_BYTES {
        return Err("这批XMP元数据较多，请减少照片数量分批导出".into());
    }
    space(
        &dir,
        if job.watermark.is_some() {
            job.items.len() as u64 * (watermark::MAX_INPUT_BYTES + 1_048_576)
        } else {
            report(&job).bytes
        },
    )?;
    Ok(Plan {
        job,
        existing: false,
    })
}
fn logs(data: &Path) -> Result<Dir> {
    let data = Dir::open(data)?;
    match data.create("exports") {
        Ok(d) => Ok(d),
        Err(e) => {
            let path = data.path.join("exports");
            if !path.exists() {
                return Err(e);
            }
            Dir::open(&path)
        }
    }
}
fn save(dir: &Dir, j: &mut Job) -> Result<()> {
    if j.step == 0 {
        dir.write_record(
            "plan.json",
            &serde_json::to_vec(j).map_err(|e| e.to_string())?,
        )?;
    }
    j.step += 1;
    let checkpoint = Checkpoint {
        id: j.id.clone(),
        step: j.step,
        directory_key: j.directory_key.clone(),
        status: j.status.clone(),
        message: j.message.clone(),
        items: j
            .items
            .iter()
            .map(|i| (i.status.clone(), i.error.clone()))
            .collect(),
        derivatives: j.items.iter().map(|i| i.derivative.clone()).collect(),
    };
    let bytes = serde_json::to_vec(&checkpoint).map_err(|e| e.to_string())?;
    dir.write_record(&format!("state-{:08}.json", j.step), &bytes)
}
fn record(dir: &Dir, name: &str, max: usize) -> Result<Vec<u8>> {
    let file = dir.read(name)?.ok_or("导出记录缺失")?;
    let mut bytes = Vec::new();
    file.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > max {
        return Err("导出记录超过上限".into());
    }
    Ok(bytes)
}
fn load(data: &Path, id: &str) -> Result<Job> {
    if uuid::Uuid::parse_str(id)
        .map_err(|_| "导出标识无效")?
        .to_string()
        != id
    {
        return Err("导出标识无效".into());
    }
    let dir = Dir::open(&data.join("exports").join(id))?;
    let mut names = fs::read_dir(&dir.path)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| {
            n.len() == 19
                && n.starts_with("state-")
                && n.ends_with(".json")
                && n.as_bytes()[6..14].iter().all(|b| b.is_ascii_digit())
        })
        .collect::<Vec<_>>();
    names.sort();
    let name = names.last().ok_or("没有完整的导出记录，保留现场")?;
    let mut j: Job = serde_json::from_slice(&record(&dir, "plan.json", MAX_PLAN_BYTES)?)
        .map_err(|_| "导出清单损坏，已停止")?;
    let checkpoint: Checkpoint = serde_json::from_slice(&record(&dir, name, 1_048_576)?)
        .map_err(|_| "导出进度损坏，已停止")?;
    if checkpoint.id != id || checkpoint.items.len() != j.items.len() {
        return Err("导出进度与清单不符".into());
    }
    j.step = checkpoint.step;
    j.directory_key = checkpoint.directory_key;
    j.status = checkpoint.status;
    j.message = checkpoint.message;
    for (item, (status, error)) in j.items.iter_mut().zip(checkpoint.items) {
        item.status = status;
        item.error = error;
    }
    if !checkpoint.derivatives.is_empty() {
        if checkpoint.derivatives.len() != j.items.len() {
            return Err("水印进度与清单不符".into());
        }
        for (item, d) in j.items.iter_mut().zip(checkpoint.derivatives) {
            item.derivative = d;
        }
    }
    if ![1, 2].contains(&j.format)
        || j.id != id
        || j.items.is_empty()
        || j.items.len() > LIMIT
        || j.directory
            != Path::new(&j.directory)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        || !j.directory.starts_with("Moment-Grove-")
    {
        return Err("导出记录无效".into());
    }
    for (n, i) in j.items.iter().enumerate() {
        if i.output
            != safe_name(
                n,
                &if j.watermark.is_some() {
                    i.path.with_extension("jpg")
                } else {
                    i.path.clone()
                },
            )
            || i.xml.len() > 1_048_576
            || i.sha256.len() != 64
            || !i.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || i.bytes > 1_099_511_627_776
        {
            return Err("导出条目无效".into());
        }
        if j.watermark.is_some()
            && i.info.as_ref().is_none_or(|info| {
                info.date.is_none()
                    || !watermark::valid_date(info.date.as_deref().unwrap_or_default())
            })
        {
            return Err("水印清单日期无效".into());
        }
        if i.derivative.as_ref().is_some_and(|d| {
            d.sha256.len() != 64
                || !d.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                || d.bytes > watermark::MAX_INPUT_BYTES
        }) {
            return Err("水印进度校验无效".into());
        }
    }
    if let Some(o) = &j.watermark
        && *o != o.normalized()?
    {
        return Err("水印设置无效".into());
    }
    if j.format == 1
        && j.watermark
            .as_ref()
            .is_some_and(|o| o.output_max_edge.is_some() || o.jpeg_quality != 92)
    {
        return Err("导出设置与清单版本不符，未继续".into());
    }
    Ok(j)
}
pub fn history(data: &Path, folder: i64) -> Result<Vec<Report>> {
    let dir = logs(data)?;
    let mut reports = Vec::new();
    for e in fs::read_dir(&dir.path).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        let Some(id) = e.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if uuid::Uuid::parse_str(&id).is_err() {
            continue;
        }
        let j = load(data, &id)?;
        if j.folder == folder {
            reports.push(report(&j));
        }
    }
    reports.sort_by(|a, b| b.destination.cmp(&a.destination));
    reports.truncate(10);
    Ok(reports)
}
#[cfg(test)]
pub fn resume(
    c: &Catalog,
    roots: &HashMap<i64, PathBuf>,
    folder: i64,
    id: &str,
    parent: &Path,
    data: &Path,
) -> Result<Plan> {
    resume_with_progress(
        c,
        roots,
        folder,
        id,
        parent,
        data,
        &mut crate::preparation::Review::new(&AtomicBool::new(false), |_| {}),
    )
}
pub fn validate(c: &Catalog, roots: &HashMap<i64, PathBuf>, p: &Plan) -> Result<()> {
    current(c, roots, &p.job)
}
fn publish_photo(
    j: &mut Job,
    n: usize,
    input: fs::File,
    dir: &Dir,
    journal: &Dir,
    cancel: &AtomicBool,
) -> Result<()> {
    let i = j.items[n].clone();
    let Some(options) = &j.watermark else {
        return dir.publish(&i.output, input, &i.sha256, cancel);
    };
    let bytes = watermark::read_input(input)?;
    if hash_bytes(&bytes) != i.sha256 {
        return Err("源内容已变化，请重新创建水印导出".into());
    }
    files::cancelled(cancel)?;
    if let Some(d) = &i.derivative
        && let Some(existing) = dir.read(&i.output)?
    {
        if digest(existing, cancel)?.0 != d.sha256 {
            return Err("已有水印副本被外部修改，未覆盖".into());
        }
        return Ok(());
    }
    let rendered = watermark::render(&bytes, options, None)?;
    if i.info.as_ref() != Some(&rendered.info.source) {
        return Err("拍摄日期或图像属性与清单不符".into());
    }
    let hash = hash_bytes(&rendered.jpeg);
    if i.derivative.as_ref().is_some_and(|d| d.sha256 != hash) {
        return Err("渲染结果与先前进度不同，请新建导出；已有文件不覆盖".into());
    }
    j.items[n].derivative = Some(Derivative {
        sha256: hash.clone(),
        bytes: rendered.jpeg.len() as u64,
        info: rendered.info,
    });
    // Durable expected derivative bytes precede publication, so crash recovery never adopts unknown files.
    save(journal, j)?;
    files::cancelled(cancel)?;
    dir.publish(&i.output, rendered.jpeg.as_slice(), &hash, cancel)
}
pub fn run(
    mut p: Plan,
    data: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(Report),
    mut after_photo: impl FnMut() -> Result<()>,
) -> Result<Report> {
    let parent = Dir::open(&p.job.parent)?;
    if !parent.matches_key(&p.job.parent_key)? {
        return Err("目标父目录已变化".into());
    }
    let all_logs = logs(data)?;
    let journal = if p.existing {
        Dir::open(&all_logs.path.join(&p.job.id))?
    } else {
        all_logs.create(&p.job.id)?
    };
    let _lock = journal.lock()?;
    if p.existing {
        p.job = load(data, &p.job.id)?;
    } else {
        save(&journal, &mut p.job)?;
    }
    let dir = if let Some(key) = &p.job.directory_key {
        let dir = Dir::open(&parent.path.join(&p.job.directory))?;
        if !dir.matches_key(key)? {
            return Err("导出子目录身份已变化，未写入".into());
        }
        dir
    } else {
        let dir = parent.create(&p.job.directory)?;
        p.job.directory_key = Some(dir.key()?);
        save(&journal, &mut p.job)?;
        dir
    };
    p.job.status = "running".into();
    p.job.message = None;
    save(&journal, &mut p.job)?;
    progress(report(&p.job));
    for n in 0..p.job.items.len() {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        let i = p.job.items[n].clone();
        let result = (|| {
            parent.unchanged()?;
            dir.unchanged()?;
            let input = files::source(&p.job.root, &i.path, &i.identity)?;
            if p.job.watermark.is_none() {
                crate::resources::refuse_incomplete_original_operation(&i.path, "复制导出")?;
            }
            let observation = sidecar::capture(&p.job.root, &i.path, &i.identity);
            if let Some(e) = observation.error {
                return Err(e);
            }
            if observation.target != i.source_xmp_target
                || observation.xml.as_ref().map(|s| hash_bytes(s.as_bytes())) != i.source_xmp_hash
                || xmp::merge(
                    if p.job.watermark.is_some() {
                        None
                    } else {
                        observation.xml.as_deref()
                    },
                    i.rating,
                )? != i.xml
            {
                return Err("源 XMP 已变化，请新建导出".into());
            }
            space(
                &dir,
                if p.job.watermark.is_some() {
                    watermark::MAX_INPUT_BYTES
                } else {
                    i.bytes
                } + i.xml.len() as u64,
            )?;
            publish_photo(&mut p.job, n, input, &dir, &journal, cancel)?;
            after_photo()?;
            dir.publish(
                &format!("{}.xmp", i.output),
                i.xml.as_bytes(),
                &hash_bytes(i.xml.as_bytes()),
                cancel,
            )?;
            Ok(())
        })();
        let item = &mut p.job.items[n];
        match result {
            Ok(()) => {
                item.status = "verified".into();
                item.error = None
            }
            Err(e) => {
                item.status = if cancel.load(Ordering::SeqCst) {
                    "pending"
                } else {
                    "error"
                }
                .into();
                item.error = Some(e)
            }
        }
        save(&journal, &mut p.job)?;
        progress(report(&p.job));
    }
    p.job.status = if cancel.load(Ordering::SeqCst) {
        "cancelled"
    } else if p.job.items.iter().all(|i| i.status == "verified") {
        "completed"
    } else {
        "partial"
    }
    .into();
    if p.job.status == "completed" {
        let mut portable = serde_json::json!({"app":"Moment Grove","format":1,"files":p.job.items.iter().map(|i|serde_json::json!({"sourceRelative":i.relative,"name":i.output,"rating":i.rating,"bytes":i.derivative.as_ref().map(|d|d.bytes).unwrap_or(i.bytes),"sha256":i.derivative.as_ref().map(|d|d.sha256.as_str()).unwrap_or(&i.sha256),"sidecarSha256":hash_bytes(i.xml.as_bytes())})).collect::<Vec<_>>()});
        if let Some(options) = &p.job.watermark {
            portable["watermark"] = serde_json::to_value(options).map_err(|e| e.to_string())?;
            portable["rendered"] = serde_json::to_value(
                p.job
                    .items
                    .iter()
                    .map(|i| i.derivative.as_ref().map(|d| &d.info))
                    .collect::<Vec<_>>(),
            )
            .map_err(|e| e.to_string())?;
        }
        let bytes = serde_json::to_vec_pretty(&portable).map_err(|e| e.to_string())?;
        if let Err(e) = dir.write_record("moment-grove-export.json", &bytes) {
            p.job.status = "partial".into();
            p.job.message = Some(e);
        }
    }
    save(&journal, &mut p.job)?;
    let result = report(&p.job);
    progress(result.clone());
    Ok(result)
}
#[cfg(test)]
pub fn stage_for_crash_test(p: &Plan, data: &Path) -> Result<()> {
    let logs = logs(data)?;
    let journal = logs.create(&p.job.id)?;
    let mut j = p.job.clone();
    save(&journal, &mut j)
}

#[allow(clippy::too_many_arguments)]
pub fn prepare_with_progress(
    c: &Catalog,
    roots: &HashMap<i64, PathBuf>,
    folder: i64,
    targets: &[Target],
    parent: &Path,
    data: &Path,
    options: Option<&watermark::Options>,
    review: &mut crate::preparation::Review<'_>,
) -> Result<Plan> {
    prepare_kind(
        c,
        roots,
        folder,
        targets,
        parent,
        data,
        options.map(watermark::Options::normalized).transpose()?,
        review,
    )
}
#[allow(clippy::too_many_arguments)]
pub fn resume_with_progress(
    c: &Catalog,
    roots: &HashMap<i64, PathBuf>,
    folder: i64,
    id: &str,
    parent: &Path,
    data: &Path,
    review: &mut crate::preparation::Review<'_>,
) -> Result<Plan> {
    review.check()?;
    let j = load(data, id)?;
    if j.folder != folder
        || parent.canonicalize().map_err(|_| "目标目录不可用")? != j.parent
        || !Dir::open(&j.parent)?.matches_key(&j.parent_key)?
    {
        return Err("请选择这次导出的原目标父目录".into());
    }
    current_with_progress(c, roots, &j, review)?;
    Ok(Plan {
        job: j,
        existing: true,
    })
}
