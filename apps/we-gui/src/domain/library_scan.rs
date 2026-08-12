#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScanRequest {
    pub generation: u64,
    pub workshop_path: String,
}

#[derive(Debug, Default)]
pub(crate) struct LibraryScanScheduler {
    generation: u64,
    active_generation: Option<u64>,
    pending: Option<ScanRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScanCompletion {
    pub accept_result: bool,
    pub next: Option<ScanRequest>,
}

impl LibraryScanScheduler {
    pub(crate) fn request(&mut self, workshop_path: String) -> Option<ScanRequest> {
        self.generation = self.generation.wrapping_add(1);
        let request = ScanRequest { generation: self.generation, workshop_path };
        if self.active_generation.is_none() {
            self.active_generation = Some(request.generation);
            Some(request)
        } else {
            self.pending = Some(request);
            None
        }
    }

    pub(crate) fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.pending = None;
    }

    pub(crate) fn complete(&mut self, generation: u64) -> ScanCompletion {
        if self.active_generation != Some(generation) {
            return ScanCompletion { accept_result: false, next: None };
        }

        self.active_generation = None;
        let next = self.pending.take();
        if let Some(request) = next.as_ref() {
            self.active_generation = Some(request.generation);
        }
        ScanCompletion { accept_result: generation == self.generation && next.is_none(), next }
    }
}

#[cfg(test)]
mod tests {
    use super::LibraryScanScheduler;

    #[test]
    fn repeated_scan_requests_keep_one_active_scan_and_only_the_latest_pending_path() {
        let mut scheduler = LibraryScanScheduler::default();
        let first =
            scheduler.request("first".to_string()).expect("first request starts immediately");

        for index in 0..100 {
            assert_eq!(scheduler.request(format!("pending-{index}")), None);
        }

        let completion = scheduler.complete(first.generation);
        assert!(!completion.accept_result);
        let next = completion.next.expect("latest request starts after the active scan completes");
        assert_eq!(next.workshop_path, "pending-99");
        assert!(scheduler.complete(next.generation).accept_result);
    }

    #[test]
    fn invalidating_the_requested_path_rejects_an_in_flight_result() {
        let mut scheduler = LibraryScanScheduler::default();
        let active = scheduler.request("old".to_string()).expect("scan starts");

        scheduler.invalidate();
        let completion = scheduler.complete(active.generation);

        assert!(!completion.accept_result);
        assert_eq!(completion.next, None);
    }
}
