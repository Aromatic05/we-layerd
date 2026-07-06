use we_renderer::InputEvent;

#[derive(Debug, Default)]
pub(crate) struct PendingInput {
    events: Vec<InputEvent>,
}

impl PendingInput {
    pub(crate) fn push(&mut self, event: InputEvent) {
        self.events.push(event);
    }

    pub(crate) fn drain(&mut self) -> Vec<InputEvent> {
        std::mem::take(&mut self.events)
    }

    pub(crate) fn clear(&mut self) {
        self.events.clear();
    }
}
