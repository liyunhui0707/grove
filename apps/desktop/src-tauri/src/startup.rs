//! Bounded local startup evidence. Only fixed phase codes and build identity;
//! never exception strings, executable/media paths, account data or telemetry.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    BackendStarted,
    BackendReady,
    BackendFailed,
    WebviewLoaded,
    FrontendEntry,
    FrontendReady,
    FrontendFailed,
    ReadinessTimeout,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrontendStage {
    #[serde(rename = "frontend_entry")]
    Entry,
    #[serde(rename = "frontend_ready")]
    Ready,
    #[serde(rename = "frontend_failed")]
    Failed,
}
impl From<FrontendStage> for Stage {
    fn from(s: FrontendStage) -> Self {
        match s {
            FrontendStage::Entry => Self::FrontendEntry,
            FrontendStage::Ready => Self::FrontendReady,
            FrontendStage::Failed => Self::FrontendFailed,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    stage: Stage,
    elapsed_ms: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Run {
    id: String,
    version: String,
    build: String,
    started_ms: u64,
    events: Vec<Event>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    version: u32,
    current: Run,
    previous: Option<Run>,
}
#[derive(Serialize)]
pub struct Summary {
    pub ready: bool,
    pub stage: Stage,
    pub recorded: bool,
}
struct State {
    report: Report,
    ready: bool,
    warned: bool,
    recorded: bool,
}
pub struct Session {
    path: PathBuf,
    started: Instant,
    writable: bool,
    state: Mutex<State>,
}
fn identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn valid(run: &Run) -> bool {
    uuid::Uuid::parse_str(&run.id).is_ok()
        && identity(&run.version)
        && identity(&run.build)
        && run.events.len() <= 16
}
fn read(path: &Path) -> Result<Option<Report>, ()> {
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(()),
    };
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 16384 {
        return Err(());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| ())?
        .take(16385)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    let report: Report = serde_json::from_slice(&bytes).map_err(|_| ())?;
    if bytes.len() > 16384
        || report.version != 1
        || !valid(&report.current)
        || report.previous.as_ref().is_some_and(|r| !valid(r))
    {
        return Err(());
    }
    Ok(Some(report))
}
impl Session {
    pub fn new(data: &Path, version: &str, build: &str) -> Self {
        let path = data.join("startup.json");
        let existing = read(&path);
        let writable = existing.is_ok();
        let current = Run {
            id: uuid::Uuid::new_v4().to_string(),
            version: if identity(version) {
                version
            } else {
                "unavailable"
            }
            .into(),
            build: if identity(build) {
                build
            } else {
                "unavailable"
            }
            .into(),
            started_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(u64::MAX as u128) as u64,
            events: Vec::new(),
        };
        let session = Self {
            path,
            started: Instant::now(),
            writable,
            state: Mutex::new(State {
                report: Report {
                    version: 1,
                    current,
                    previous: existing.ok().flatten().map(|r| r.current),
                },
                ready: false,
                warned: false,
                recorded: false,
            }),
        };
        session.record(Stage::BackendStarted);
        session
    }
    fn persist(&self, report: &Report) -> Result<(), ()> {
        if !self.writable {
            return Err(());
        }
        // Refuse changed, linked, corrupt or future records, rather than
        // replacing an unknown file to make diagnostics appear successful.
        read(&self.path)?;
        let bytes = serde_json::to_vec(report).map_err(|_| ())?;
        let temp = self
            .path
            .with_file_name(format!("startup-{}.tmp", uuid::Uuid::new_v4()));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp).map_err(|_| ())?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| ())?;
        fs::rename(temp, &self.path).map_err(|_| ())
    }
    pub fn record(&self, stage: Stage) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.ready {
            return;
        } // Startup-only: never reset a working UI.
        if state
            .report
            .current
            .events
            .last()
            .is_some_and(|e| e.stage == stage)
        {
            return;
        }
        let events = &mut state.report.current.events;
        if events.len() == 16 {
            events.remove(0);
        }
        events.push(Event {
            stage,
            elapsed_ms: self.started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        });
        state.ready = stage == Stage::FrontendReady;
        state.recorded = self.persist(&state.report).is_ok();
    }
    pub fn warn_if_unready(&self) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state.ready || state.warned {
            return false;
        }
        state.warned = true;
        drop(state);
        self.record(Stage::ReadinessTimeout);
        true
    }
    pub fn summary(&self) -> Summary {
        match self.state.lock() {
            Ok(s) => Summary {
                ready: s.ready,
                stage: s
                    .report
                    .current
                    .events
                    .last()
                    .map(|e| e.stage)
                    .unwrap_or(Stage::BackendStarted),
                recorded: s.recorded,
            },
            Err(_) => Summary {
                ready: false,
                stage: Stage::BackendStarted,
                recorded: false,
            },
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn records_only_bounded_codes_and_retains_one_previous_run() {
        let t = tempfile::tempdir().unwrap();
        let a = Session::new(t.path(), "0.5.0", "test-build");
        a.record(Stage::BackendReady);
        a.record(Stage::FrontendEntry);
        a.record(Stage::FrontendReady);
        assert!(a.summary().ready && a.summary().recorded);
        a.record(Stage::FrontendFailed);
        assert!(!a.warn_if_unready());
        let b = Session::new(t.path(), "0.5.0", "test-build");
        for _ in 0..40 {
            b.record(Stage::FrontendEntry);
            b.record(Stage::FrontendFailed);
        }
        let c = Session::new(t.path(), "0.5.0", "test-build");
        c.record(Stage::FrontendReady);
        let raw = fs::read_to_string(t.path().join("startup.json")).unwrap();
        let report: Report = serde_json::from_str(&raw).unwrap();
        assert!(raw.len() < 16384 && report.previous.unwrap().events.len() == 16);
        for excluded in ["/Users/", "/private/", "photo", "error", "token", "path"] {
            assert!(!raw.contains(excluded));
        }
        assert!(!c.warn_if_unready());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(t.path().join("startup.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
    #[test]
    fn corrupt_large_future_records_remain_unchanged_and_do_not_block_readiness() {
        for bytes in [
            b"broken".to_vec(),
            vec![b' '; 16385],
            br#"{"version":99}"#.to_vec(),
        ] {
            let t = tempfile::tempdir().unwrap();
            let p = t.path().join("startup.json");
            fs::write(&p, &bytes).unwrap();
            let s = Session::new(t.path(), "0.5.0", "build");
            s.record(Stage::FrontendReady);
            assert!(s.summary().ready && !s.summary().recorded);
            assert_eq!(fs::read(p).unwrap(), bytes);
        }
    }
    #[test]
    fn timeout_is_once_only_and_late_readiness_wins() {
        let t = tempfile::tempdir().unwrap();
        let s = Session::new(t.path(), "0.5.0", "build");
        assert!(s.warn_if_unready());
        assert!(!s.warn_if_unready());
        s.record(Stage::FrontendReady);
        assert!(s.summary().ready);
        assert!(!s.warn_if_unready());
        assert!(serde_json::from_str::<FrontendStage>("\"backend_ready\"").is_err());
        assert!(serde_json::from_str::<FrontendStage>("\"/private/photo.jpg\"").is_err());
    }
    #[cfg(unix)]
    #[test]
    fn linked_records_are_not_followed_or_replaced() {
        let t = tempfile::tempdir().unwrap();
        let target = t.path().join("marker");
        fs::write(&target, b"keep").unwrap();
        std::os::unix::fs::symlink(&target, t.path().join("startup.json")).unwrap();
        let s = Session::new(t.path(), "0.5.0", "build");
        s.record(Stage::FrontendReady);
        assert!(s.summary().ready && !s.summary().recorded);
        assert_eq!(fs::read(&target).unwrap(), b"keep");
        assert!(
            fs::symlink_metadata(t.path().join("startup.json"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}
