//! A normal close/quit waits for the WebView's style write queue. Programmatic
//! startup-failure exits and OS-forced termination are outside this handshake.
#[derive(Default)]
pub struct Gate {
    armed: bool,
    sequence: u32,
    pending: Option<u32>,
    approved: bool,
}

#[derive(Debug, PartialEq)]
pub enum Request {
    Allow,
    Notify(u32),
    Wait,
}

impl Gate {
    pub fn arm(&mut self) {
        self.armed = true;
    }

    pub fn request(&mut self) -> Request {
        if !self.armed || self.approved {
            return Request::Allow;
        }
        if self.pending.is_some() {
            return Request::Wait;
        }
        self.sequence = self.sequence.wrapping_add(1);
        self.pending = Some(self.sequence);
        Request::Notify(self.sequence)
    }

    pub fn resolve(&mut self, token: u32, proceed: bool) -> Result<(), &'static str> {
        if self.pending != Some(token) {
            return Err("退出请求已失效");
        }
        self.pending = None;
        self.approved = proceed;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_quit_waits_for_matching_acknowledgement() {
        let mut gate = Gate::default();
        assert_eq!(gate.request(), Request::Allow);
        gate.arm();
        assert_eq!(gate.request(), Request::Notify(1));
        assert_eq!(gate.request(), Request::Wait);
        assert!(gate.resolve(2, true).is_err());
        assert_eq!(gate.request(), Request::Wait);
        gate.resolve(1, true).unwrap();
        assert_eq!(gate.request(), Request::Allow);
        assert!(gate.resolve(1, true).is_err());
    }

    #[test]
    fn cancel_keeps_protection_and_rejects_late_success() {
        let mut gate = Gate::default();
        gate.arm();
        assert_eq!(gate.request(), Request::Notify(1));
        gate.resolve(1, false).unwrap();
        assert_eq!(gate.request(), Request::Notify(2));
        assert!(gate.resolve(1, true).is_err());
        assert_eq!(gate.request(), Request::Wait);
        gate.resolve(2, true).unwrap();
        assert_eq!(gate.request(), Request::Allow);
    }
}
