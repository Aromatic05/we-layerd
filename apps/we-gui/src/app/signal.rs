use std::sync::atomic::{AtomicBool, Ordering};

struct ShutdownSignal {
    pending: AtomicBool,
    interrupted: AtomicBool,
}

impl ShutdownSignal {
    const fn new() -> Self {
        Self { pending: AtomicBool::new(false), interrupted: AtomicBool::new(false) }
    }

    fn request(&self) {
        self.interrupted.store(true, Ordering::Release);
        self.pending.store(true, Ordering::Release);
    }

    fn take_request(&self) -> bool {
        self.pending.swap(false, Ordering::AcqRel)
    }

    fn was_interrupted(&self) -> bool {
        self.interrupted.load(Ordering::Acquire)
    }
}

static SHUTDOWN_SIGNAL: ShutdownSignal = ShutdownSignal::new();

pub(super) fn install() -> Result<(), ctrlc::Error> {
    ctrlc::set_handler(request_shutdown)
}

fn request_shutdown() {
    SHUTDOWN_SIGNAL.request();
}

pub(super) fn take_shutdown_request() -> bool {
    SHUTDOWN_SIGNAL.take_request()
}

pub(super) fn was_interrupted() -> bool {
    SHUTDOWN_SIGNAL.was_interrupted()
}

#[cfg(test)]
mod tests {
    use super::ShutdownSignal;

    #[test]
    fn shutdown_request_is_consumed_once_and_remembers_interrupt() {
        let signal = ShutdownSignal::new();

        assert!(!signal.take_request());
        assert!(!signal.was_interrupted());

        signal.request();

        assert!(signal.take_request());
        assert!(!signal.take_request());
        assert!(signal.was_interrupted());
    }
}
