#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimePhase {
    Idle,
    Starting,
    Running,
    Paused,
    Stopping,
    Failed,
}

impl RuntimePhase {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Stopping => "stopping",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RuntimeControl {
    paused: bool,
    stopping: bool,
}

impl RuntimeControl {
    pub(crate) fn pause(&mut self) {
        self.paused = true;
    }

    pub(crate) fn resume(&mut self) {
        self.paused = false;
    }

    pub(crate) fn stop(&mut self) {
        self.stopping = true;
        self.paused = true;
    }

    pub(crate) fn paused(&self) -> bool {
        self.paused
    }

    pub(crate) fn stopping(&self) -> bool {
        self.stopping
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeControl;

    #[test]
    fn pause_resume_stop_are_pure_runtime_state() {
        let mut control = RuntimeControl::default();
        assert!(!control.paused());
        assert!(!control.stopping());

        control.pause();
        assert!(control.paused());
        assert!(!control.stopping());

        control.resume();
        assert!(!control.paused());

        control.stop();
        assert!(control.paused());
        assert!(control.stopping());
    }
}
