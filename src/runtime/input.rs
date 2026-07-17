use we_renderer::InputEvent;

#[derive(Debug, Default)]
pub(crate) struct PendingInput {
    events: Vec<InputEvent>,
}

impl PendingInput {
    pub(crate) fn push(&mut self, event: InputEvent) {
        if matches!(event, InputEvent::PointerMove { .. }) {
            if let Some(last @ InputEvent::PointerMove { .. }) = self.events.last_mut() {
                *last = event;
                return;
            }
        }
        self.events.push(event);
    }

    pub(crate) fn drain(&mut self) -> Vec<InputEvent> {
        std::mem::take(&mut self.events)
    }

    pub(crate) fn clear(&mut self) {
        self.events.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::PendingInput;
    use we_renderer::InputEvent;

    #[test]
    fn consecutive_pointer_moves_are_coalesced_to_the_latest_position() {
        let mut pending = PendingInput::default();
        pending.push(InputEvent::PointerMove { x: 0.1, y: 0.2 });
        pending.push(InputEvent::PointerMove { x: 0.8, y: 0.9 });

        assert_eq!(pending.drain(), vec![InputEvent::PointerMove { x: 0.8, y: 0.9 }]);
    }

    #[test]
    fn pointer_moves_separated_by_an_action_are_not_coalesced() {
        let mut pending = PendingInput::default();
        pending.push(InputEvent::PointerMove { x: 0.1, y: 0.2 });
        pending.push(InputEvent::PointerDown { x: 0.1, y: 0.2, button: 0 });
        pending.push(InputEvent::PointerMove { x: 0.8, y: 0.9 });

        assert_eq!(
            pending.drain(),
            vec![
                InputEvent::PointerMove { x: 0.1, y: 0.2 },
                InputEvent::PointerDown { x: 0.1, y: 0.2, button: 0 },
                InputEvent::PointerMove { x: 0.8, y: 0.9 },
            ]
        );
    }

    #[test]
    fn semantic_state_transitions_are_never_discarded() {
        let mut pending = PendingInput::default();
        for focused in (0..600).map(|index| index % 2 == 0) {
            pending.push(InputEvent::Focus { focused });
        }

        let events = pending.drain();
        assert_eq!(events.len(), 600);
        assert_eq!(events.first(), Some(&InputEvent::Focus { focused: true }));
        assert_eq!(events.last(), Some(&InputEvent::Focus { focused: false }));
    }
}
