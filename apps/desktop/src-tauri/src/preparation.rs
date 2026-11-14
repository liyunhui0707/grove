//! Read-only batch review progress. It never owns media, catalogs, or durable jobs.
use crate::catalog::Result;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub phase: &'static str,
    pub checked: usize,
    pub total: usize,
    pub current: Option<String>,
}
pub struct Review<'a> {
    pub cancel: &'a AtomicBool,
    emit: Box<dyn FnMut(Progress) + 'a>,
    state: Progress,
}
impl<'a> Review<'a> {
    pub fn new(cancel: &'a AtomicBool, emit: impl FnMut(Progress) + 'a) -> Self {
        Self {
            cancel,
            emit: Box::new(emit),
            state: Progress {
                phase: "checking",
                checked: 0,
                total: 0,
                current: None,
            },
        }
    }
    pub fn check(&self) -> Result<()> {
        if self.cancel.load(Ordering::SeqCst) {
            Err("已取消清单核对，本次未开始文件操作".into())
        } else {
            Ok(())
        }
    }
    pub fn begin(&mut self, phase: &'static str, total: usize) -> Result<()> {
        self.check()?;
        self.state = Progress {
            phase,
            checked: 0,
            total,
            current: None,
        };
        (self.emit)(self.state.clone());
        self.check()
    }
    pub fn item(&mut self, name: &str) -> Result<()> {
        self.check()?;
        self.state.current = Some(name.into());
        (self.emit)(self.state.clone());
        self.check()
    }
    pub fn done(&mut self) -> Result<()> {
        self.check()?;
        self.state.checked += 1;
        (self.emit)(self.state.clone());
        self.check()
    }
}
