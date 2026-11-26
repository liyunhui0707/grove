//! Explicit, journaled sidecar sync. Source images are opened read-only.
mod files;
use crate::{
    catalog::{Batch, Catalog, Result},
    xmp,
};
use files::{Files, MAX_XML};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub struct Observation {
    pub target: String,
    pub xml: Option<String>,
    pub error: Option<String>,
}
impl Observation {
    pub fn rating(&self) -> Result<Option<i32>> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        self.xml.as_deref().map(xmp::read).unwrap_or(Ok(None))
    }
}
pub fn capture(root: &Path, source: &Path, key: &str) -> Observation {
    match Files::open(root, source, key).and_then(|f| f.resolve()) {
        Ok((target, xml)) => {
            let error = xml.as_deref().and_then(|s| xmp::read(s).err());
            Observation { target, xml, error }
        }
        Err(error) => Observation {
            target: files::default_target(source),
            xml: None,
            error: Some(error),
        },
    }
}
pub(crate) fn verify_source(root: &Path, source: &Path, key: &str) -> Result<()> {
    Files::open(root, source, key).map(|_| ())
}
pub fn remember(catalog: &Catalog, id: &str, observed: &Observation) -> Result<()> {
    let inserted = catalog
        .db
        .execute(
            "INSERT OR IGNORE INTO xmp_state(id,target,baseline,error) VALUES (?1,?2,?3,?4)",
            params![id, observed.target, observed.xml, observed.error],
        )
        .map_err(|e| e.to_string())?;
    // An already identified occurrence may have been renamed outside the app.
    // Rebind its sidecar name only if all known XML bytes still match and no write is active.
    if observed.error.is_none() {
        catalog.db.execute("UPDATE xmp_state SET target=?2,seen_token=NULL,seen_target=NULL,seen_xml=NULL WHERE id=?1 AND target<>?2 AND baseline IS ?3 AND error IS NULL AND NOT EXISTS(SELECT 1 FROM xmp_jobs WHERE asset_id=?1 AND status IN ('prepared','conflict'))",params![id,observed.target,observed.xml]).map_err(|e|e.to_string())?;
    }
    if inserted == 0 {
        catalog.db.execute("INSERT INTO pending_xmp(id,revision) SELECT a.id,a.revision FROM assets a JOIN xmp_state s ON s.id=a.id WHERE a.id=?1 AND (s.target<>?2 OR s.baseline IS NOT ?3 OR s.error IS NOT ?4) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision",params![id,observed.target,observed.xml,observed.error]).map_err(|e|e.to_string())?;
    } else if let Ok(rating) = observed.rating() {
        // Old catalogs have no trustworthy pre-existing XML baseline. Require a review
        // when their saved rating differs from the first observed file rating.
        catalog.db.execute("UPDATE xmp_state SET error='既有本机评分与首次观察的文件评分不同，需核对' WHERE id=?1 AND EXISTS(SELECT 1 FROM assets WHERE id=?1 AND rating IS NOT ?2)",params![id,rating]).map_err(|e|e.to_string())?;
        catalog.db.execute("INSERT INTO pending_xmp(id,revision) SELECT id,revision FROM assets WHERE id=?1 AND rating IS NOT ?2 ON CONFLICT(id) DO UPDATE SET revision=excluded.revision",params![id,rating]).map_err(|e|e.to_string())?;
    }
    Ok(())
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub id: String,
    pub name: String,
    pub relative: String,
    pub rating: Option<i32>,
    pub revision: i64,
    pub disk_rating: Option<i32>,
    pub disk_known: bool,
    pub state: String,
    pub message: String,
    pub review_token: Option<String>,
    pub can_repair: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub pending: i64,
    pub recoveries: i64,
    pub supported: bool,
    pub offset: u32,
    pub items: Vec<Row>,
}
#[derive(Serialize)]
pub struct Outcome {
    pub completed: usize,
    pub failed: usize,
    pub report: Report,
}
#[derive(Deserialize)]
pub struct Target {
    pub id: String,
    pub revision: i64,
}
struct Local {
    id: String,
    folder: i64,
    source: String,
    key: String,
    rating: Option<i32>,
    revision: i64,
}
fn local(c: &Catalog, id: &str) -> Result<Local> {
    c.db.query_row(
        "SELECT id,folder_id,path,identity,rating,revision FROM assets WHERE id=?1",
        [id],
        |r| {
            Ok(Local {
                id: r.get(0)?,
                folder: r.get(1)?,
                source: r.get(2)?,
                key: r.get(3)?,
                rating: r.get(4)?,
                revision: r.get(5)?,
            })
        },
    )
    .map_err(|_| "照片记录不存在".into())
}
fn source_files(roots: &HashMap<i64, PathBuf>, item: &Local) -> Result<Files> {
    Files::open(
        roots.get(&item.folder).ok_or("请重新打开照片目录")?,
        Path::new(&item.source),
        &item.key,
    )
}
fn baseline(c: &Catalog, id: &str) -> Result<Observation> {
    c.db.query_row(
        "SELECT target,baseline,error FROM xmp_state WHERE id=?1",
        [id],
        |r| {
            Ok(Observation {
                target: r.get(0)?,
                xml: r.get(1)?,
                error: r.get(2)?,
            })
        },
    )
    .map_err(|_| "请重新打开目录建立 XMP 基线".into())
}
#[derive(Clone)]
struct Job {
    token: String,
    id: String,
    revision: i64,
    source: String,
    target: String,
    staged: String,
    previous: String,
    expected: Option<String>,
    desired: String,
    status: String,
    error: Option<String>,
}
fn active_job(c: &Catalog, id: &str) -> Result<Option<Job>> {
    c.db.query_row("SELECT token,asset_id,revision,source,target,staged,previous,expected,desired,status,error FROM xmp_jobs WHERE asset_id=?1 AND status IN ('prepared','conflict')",[id],|r|Ok(Job{token:r.get(0)?,id:r.get(1)?,revision:r.get(2)?,source:r.get(3)?,target:r.get(4)?,staged:r.get(5)?,previous:r.get(6)?,expected:r.get(7)?,desired:r.get(8)?,status:r.get(9)?,error:r.get(10)?})).optional().map_err(|e|e.to_string())
}
fn record_seen(
    c: &Catalog,
    item: &Local,
    row: &mut Row,
    target: &str,
    xml: Option<&str>,
) -> Result<()> {
    row.disk_rating = xml.map(xmp::read).unwrap_or(Ok(None))?;
    row.disk_known = true;
    let token = Uuid::new_v4().to_string();
    c.db.execute(
        "UPDATE xmp_state SET seen_token=?2,seen_target=?3,seen_xml=?4 WHERE id=?1",
        params![item.id, token, target, xml],
    )
    .map_err(|e| e.to_string())?;
    row.review_token = Some(token);
    Ok(())
}
fn inspect(c: &Catalog, roots: &HashMap<i64, PathBuf>, item: &Local) -> Row {
    let mut row = Row {
        id: item.id.clone(),
        name: Path::new(&item.source)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        relative: roots
            .get(&item.folder)
            .and_then(|root| Path::new(&item.source).strip_prefix(root).ok())
            .unwrap_or(Path::new("照片"))
            .to_string_lossy()
            .into_owned(),
        rating: item.rating,
        revision: item.revision,
        disk_rating: None,
        disk_known: false,
        state: "error".into(),
        message: String::new(),
        review_token: None,
        can_repair: false,
    };
    let result = (|| {
        if !files::SUPPORTED {
            return Err("此平台尚未启用安全 XMP 同步".into());
        }
        let files = source_files(roots, item)?;
        if let Some(job) = active_job(c, &item.id)? {
            validate_job(item, &job)?;
            row.can_repair = files.read(&job.target)?.is_none() && files.regular(&job.previous)?;
            row.state = if job.status == "prepared" {
                "retry"
            } else {
                "error"
            }
            .into();
            row.message = job
                .error
                .clone()
                .unwrap_or_else(|| "上次写入未完成，可重试核验与恢复".into());
            if job.status == "prepared" && job.revision < item.revision {
                row.message = format!(
                    "{}；先完成之前已确认的任务，较新本机评分会继续保留待同步",
                    row.message
                );
            }
            if job.status == "conflict" {
                row.message = format!(
                    "{}；保留现场：{} / {}",
                    row.message, job.previous, job.staged
                );
                if !row.can_repair
                    && let Ok((target, Some(xml))) = files.resolve()
                {
                    record_seen(c, item, &mut row, &target, Some(&xml))?;
                }
            }
            return Ok(());
        }
        let saved = baseline(c, &item.id)?;
        let (target, xml) = files.resolve()?;
        row.disk_rating = xml.as_deref().map(xmp::read).unwrap_or(Ok(None))?;
        row.disk_known = true;
        if saved.error.is_some() || saved.target != target || saved.xml != xml {
            row.state = "conflict".into();
            row.message = "外部 XMP 与已知基线不同；未写入".into();
            record_seen(c, item, &mut row, &target, xml.as_deref())?;
        } else {
            let desired = xmp::merge(xml.as_deref(), item.rating)?;
            if desired.len() > MAX_XML {
                return Err("合并后的 XMP 超过上限".into());
            }
            row.state = "ready".into();
            row.message = "可写入；照片原件保持不变".into();
        }
        Ok::<_, String>(())
    })();
    if let Err(e) = result {
        row.state = "error".into();
        row.message = e;
    }
    row
}
pub fn report(c: &Catalog, roots: &HashMap<i64, PathBuf>, folder: i64) -> Result<Report> {
    report_page(c, roots, folder, 0)
}
pub fn report_page(
    c: &Catalog,
    roots: &HashMap<i64, PathBuf>,
    folder: i64,
    offset: u32,
) -> Result<Report> {
    if !roots.contains_key(&folder) {
        return Err("请先打开此照片目录".into());
    }
    let pending: i64 =
        c.db.query_row(
            "SELECT count(*) FROM pending_xmp p JOIN assets a ON a.id=p.id WHERE a.folder_id=?1",
            [folder],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let recoveries=c.db.query_row("SELECT count(*) FROM xmp_jobs j JOIN assets a ON a.id=j.asset_id WHERE a.folder_id=?1 AND j.expected IS NOT NULL",[folder],|r|r.get(0)).map_err(|e|e.to_string())?;
    let offset = offset.min((pending.saturating_sub(1).max(0) as u32 / 100) * 100);
    let ids:Vec<String>=c.db.prepare("SELECT a.id FROM pending_xmp p JOIN assets a ON a.id=p.id WHERE a.folder_id=?1 ORDER BY a.path LIMIT 100 OFFSET ?2").map_err(|e|e.to_string())?.query_map(params![folder,offset],|r|r.get(0)).map_err(|e|e.to_string())?.collect::<std::result::Result<_,_>>().map_err(|e|e.to_string())?;
    let items = ids
        .iter()
        .map(|id| local(c, id).map(|a| inspect(c, roots, &a)))
        .collect::<Result<_>>()?;
    Ok(Report {
        pending,
        recoveries,
        supported: files::SUPPORTED,
        offset,
        items,
    })
}
fn complete(
    c: &mut Catalog,
    item: &Local,
    job: Option<&Job>,
    target: &str,
    xml: &str,
    revision: i64,
) -> Result<()> {
    let previous = baseline(c, &item.id)?;
    let written_rating = xmp::read(xml)?;
    let tx = c.db.transaction().map_err(|e| e.to_string())?;
    // The verified write changed one physical sidecar. Advance equivalent
    // baselines only; independently conflicted or recovering rows need review.
    let peers = {
        let mut q = tx.prepare("SELECT a.id FROM folders f CROSS JOIN assets a JOIN xmp_state s ON s.id=a.id
            WHERE f.id<>?1 AND a.folder_id=f.id AND a.path=?2 AND a.identity=?3 AND a.rating IS ?4
            AND s.target=?5 AND s.baseline IS ?6 AND s.error IS NULL AND s.seen_token IS NULL
            AND NOT EXISTS (SELECT 1 FROM xmp_jobs WHERE asset_id=a.id AND status IN ('prepared','conflict'))")
            .map_err(|e|e.to_string())?;
        q.query_map(
            params![
                item.folder,
                item.source,
                item.key,
                written_rating,
                target,
                previous.xml
            ],
            |r| r.get::<_, String>(0),
        )
        .map_err(|e| e.to_string())?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
    };
    for peer in peers {
        tx.execute(
            "UPDATE xmp_state SET baseline=?2 WHERE id=?1",
            params![peer, xml],
        )
        .map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM pending_xmp WHERE id=?1", [peer])
            .map_err(|e| e.to_string())?;
    }
    tx.execute("UPDATE xmp_state SET target=?2,baseline=?3,error=NULL,seen_token=NULL,seen_target=NULL,seen_xml=NULL WHERE id=?1",params![item.id,target,xml]).map_err(|e|e.to_string())?;
    tx.execute(
        "DELETE FROM pending_xmp WHERE id=?1 AND revision=?2",
        params![item.id, revision],
    )
    .map_err(|e| e.to_string())?;
    if let Some(job) = job {
        tx.execute(
            "UPDATE xmp_jobs SET status='done',error=NULL WHERE token=?1",
            [&job.token],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}
fn conflict(c: &Catalog, job: &Job, message: &str) -> Result<()> {
    c.db.execute(
        "UPDATE xmp_jobs SET status='conflict',error=?2 WHERE token=?1",
        params![job.token, message],
    )
    .map_err(|e| e.to_string())?;
    Err(message.into())
}
#[derive(Clone, Copy, PartialEq)]
enum Checkpoint {
    Staged,
    PreviousSaved,
    BeforePublish,
    Published,
}
fn validate_job(item: &Local, job: &Job) -> Result<()> {
    let token = Uuid::parse_str(&job.token)
        .map_err(|_| "写入记录标识无效")?
        .to_string();
    let source = Path::new(&item.source);
    let allowed_target = job.target == files::default_target(source)
        || job.target
            == source
                .file_name()
                .map(Path::new)
                .unwrap_or(Path::new(""))
                .with_extension("xmp")
                .to_string_lossy();
    if job.source != item.source
        || job.revision < 0
        || job.revision > item.revision
        || !allowed_target
        || job.token != token
        || job.staged != format!(".moment-grove-{token}.staged.xmp")
        || job.previous != format!(".moment-grove-{token}.previous.xmp")
        || job.desired.len() > MAX_XML
    {
        return Err("照片位置或写入记录已变化，未在新位置重试".into());
    }
    xmp::read(&job.desired)?;
    if let Some(xml) = &job.expected {
        xmp::read(xml)?;
    }
    Ok(())
}
fn run_job(
    c: &mut Catalog,
    item: &Local,
    files: &Files,
    job: &Job,
    checkpoint: &mut impl FnMut(Checkpoint) -> Result<()>,
) -> Result<()> {
    if job.status != "prepared" {
        return Err("写入现场需人工核对，未自动重试".into());
    }
    if let Err(e) = validate_job(item, job) {
        return conflict(c, job, &e);
    }
    files.check_target(&job.target)?;
    let current = files.read(&job.target)?;
    let previous = match files.read(&job.previous) {
        Ok(value) => value,
        Err(_) => {
            if current.is_none() {
                let _ = files.rename(&job.previous, &job.target);
                let _ = files.sync();
            }
            return conflict(c, job, "恢复副本不可读，已停止并保留原始字节");
        }
    };
    if current.as_deref() == Some(&job.desired) {
        if job.expected.is_some() && previous != job.expected {
            return conflict(c, job, "恢复副本与预期不同，已保留现场");
        }
        files.sync()?;
        return complete(c, item, Some(job), &job.target, &job.desired, job.revision);
    }
    if let Some(expected) = &job.expected {
        if previous.is_none() {
            if current.as_ref() != Some(expected) {
                return conflict(c, job, "外部 XMP 已变化，未写入");
            }
            files.stage(&job.staged, &job.desired)?;
            checkpoint(Checkpoint::Staged)?;
            files.rename(&job.target, &job.previous)?;
            files.sync()?;
            checkpoint(Checkpoint::PreviousSaved)?;
            if !matches!(files.read(&job.previous),Ok(Some(ref actual)) if actual==expected) {
                // Never overwrite a file created by another app during this interval.
                let _ = files.rename(&job.previous, &job.target);
                let _ = files.sync();
                return conflict(c, job, "写入前外部文件发生变化，已停止并保留恢复现场");
            }
        } else if previous.as_ref() != Some(expected) {
            return conflict(c, job, "恢复副本已变化，已停止写入");
        }
    } else if previous.is_some() {
        return conflict(c, job, "出现非预期恢复副本，已保留现场");
    }
    if files.read(&job.target)?.is_some() {
        return conflict(c, job, "目标 XMP 已由其他程序创建，未覆盖");
    }
    files.stage(&job.staged, &job.desired)?;
    checkpoint(Checkpoint::BeforePublish)?;
    if let Err(e) = files.rename(&job.staged, &job.target) {
        // If publishing failed, put the previous file back only when the name is still free.
        if job.expected.is_some() {
            let _ = files.rename(&job.previous, &job.target);
            let _ = files.sync();
        }
        return Err(e);
    }
    files.sync()?;
    checkpoint(Checkpoint::Published)?;
    if files.read(&job.target)?.as_deref() != Some(&job.desired) {
        return conflict(c, job, "写入后 XMP 已变化，未确认成功");
    }
    if job.expected.is_some() && files.read(&job.previous)? != job.expected {
        return conflict(c, job, "写入时恢复副本发生变化，已保留现场");
    }
    complete(c, item, Some(job), &job.target, &job.desired, job.revision)
}
#[cfg(test)]
fn sync_one(
    c: &mut Catalog,
    roots: &HashMap<i64, PathBuf>,
    id: &str,
    checkpoint: &mut impl FnMut(Checkpoint) -> Result<()>,
) -> Result<()> {
    sync_one_at(c, roots, id, None, checkpoint)
}
fn sync_one_at(
    c: &mut Catalog,
    roots: &HashMap<i64, PathBuf>,
    id: &str,
    expected_revision: Option<i64>,
    checkpoint: &mut impl FnMut(Checkpoint) -> Result<()>,
) -> Result<()> {
    if !files::SUPPORTED {
        return Err("此平台尚未启用安全 XMP 同步".into());
    }
    let item = local(c, id)?;
    if expected_revision.is_some_and(|r| r != item.revision) {
        return Err("本机评分在核对后已变化，请重新检查".into());
    }
    let files = source_files(roots, &item)?;
    let job = if let Some(job) = active_job(c, id)? {
        job
    } else {
        let dirty: bool =
            c.db.prepare("SELECT 1 FROM pending_xmp WHERE id=?1")
                .map_err(|e| e.to_string())?
                .exists([id])
                .map_err(|e| e.to_string())?;
        if !dirty {
            return Ok(());
        }
        let saved = baseline(c, id)?;
        if let Some(e) = saved.error {
            return Err(e);
        }
        let (target, xml) = files.resolve()?;
        if saved.target != target || saved.xml != xml {
            return Err("外部 XMP 已变化，未写入".into());
        }
        let desired = xmp::merge(xml.as_deref(), item.rating)?;
        if desired.len() > MAX_XML {
            return Err("合并后的 XMP 超过上限".into());
        }
        if xml.as_deref() == Some(&desired) {
            return complete(c, &item, None, &target, &desired, item.revision);
        }
        let token = Uuid::new_v4().to_string();
        let job = Job {
            staged: format!(".moment-grove-{token}.staged.xmp"),
            previous: format!(".moment-grove-{token}.previous.xmp"),
            token,
            id: id.into(),
            revision: item.revision,
            source: item.source.clone(),
            target,
            expected: xml,
            desired,
            status: "prepared".into(),
            error: None,
        };
        c.db.execute("INSERT INTO xmp_jobs(token,asset_id,revision,source,target,staged,previous,expected,desired,status) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'prepared')",params![job.token,job.id,job.revision,job.source,job.target,job.staged,job.previous,job.expected,job.desired]).map_err(|e|e.to_string())?;
        job
    };
    let result = run_job(c, &item, &files, &job, checkpoint);
    if let Err(e) = &result {
        c.db.execute(
            "UPDATE xmp_jobs SET error=?2 WHERE token=?1",
            params![job.token, e],
        )
        .map_err(|e| e.to_string())?;
    }
    result
}
pub fn sync(
    c: &mut Catalog,
    roots: &HashMap<i64, PathBuf>,
    folder: i64,
    targets: &[Target],
) -> Result<Outcome> {
    if targets.is_empty()
        || targets.len() > 100
        || targets.iter().map(|t| &t.id).collect::<HashSet<_>>().len() != targets.len()
    {
        return Err("每次请选择 1–100 个不重复的同步目标".into());
    }
    for target in targets {
        if local(c, &target.id)?.folder != folder {
            return Err("同步目标不属于当前目录".into());
        }
    }
    let mut completed = 0;
    for target in targets {
        if sync_one_at(c, roots, &target.id, Some(target.revision), &mut |_| Ok(())).is_ok() {
            completed += 1;
        }
    }
    Ok(Outcome {
        completed,
        failed: targets.len() - completed,
        report: report(c, roots, folder)?,
    })
}
pub fn adopt(
    c: &mut Catalog,
    roots: &HashMap<i64, PathBuf>,
    id: &str,
    revision: i64,
    token: &str,
) -> Result<Batch> {
    let item = local(c, id)?;
    let job = active_job(c, id)?;
    if item.revision != revision || job.as_ref().is_some_and(|j| j.status == "prepared") {
        return Err("评分或写入状态已变化，请刷新后核对".into());
    }
    let (seen_token, target, xml): (Option<String>, Option<String>, Option<String>) =
        c.db.query_row(
            "SELECT seen_token,seen_target,seen_xml FROM xmp_state WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|e| e.to_string())?;
    let files = source_files(roots, &item)?;
    let (current_target, current_xml) = files.resolve()?;
    if let Some(job) = job {
        validate_job(&item, &job)?;
        if current_xml.is_none() {
            return Err("请先恢复缺失的 XMP 文件".into());
        }
    }
    if seen_token.as_deref() != Some(token)
        || target.as_deref() != Some(&current_target)
        || xml != current_xml
    {
        return Err("外部文件在核对后又发生变化，请刷新".into());
    }
    let rating = current_xml.as_deref().map(xmp::read).unwrap_or(Ok(None))?;
    c.adopt_xmp(id, rating, &current_target, current_xml.as_deref())
}
pub fn repair(c: &mut Catalog, roots: &HashMap<i64, PathBuf>, id: &str) -> Result<()> {
    let item = local(c, id)?;
    let job = active_job(c, id)?.ok_or("没有需恢复的写入记录")?;
    validate_job(&item, &job)?;
    let files = source_files(roots, &item)?;
    if files.read(&job.target)?.is_some() || !files.regular(&job.previous)? {
        return Err("目标或恢复副本已变化，请刷新核对".into());
    }
    files.rename(&job.previous, &job.target)?;
    files.sync()?;
    c.db.execute(
        "UPDATE xmp_jobs SET status='retained' WHERE token=?1",
        [&job.token],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests;
