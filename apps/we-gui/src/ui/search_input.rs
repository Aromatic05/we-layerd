use iced::{
    advanced::{
        layout, mouse, overlay, renderer, text,
        widget::{Operation, Tree},
        Clipboard, Layout, Shell, Widget,
    },
    keyboard,
    widget::text_input::{self, TextInput},
    Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector,
};

type InputState = text_input::State<<Renderer as text::Renderer>::Paragraph>;

/// Clears a focused native search input on Escape without dropping its focus.
pub(crate) fn clearable<'a, M: Clone + 'a>(input: TextInput<'a, M>, on_clear: M) -> Element<'a, M> {
    Element::new(Clearable { input: input.into(), on_clear })
}

struct Clearable<'a, M> {
    input: Element<'a, M>,
    on_clear: M,
}

fn intercept_escape<M: Clone>(
    child: &Tree,
    event: &Event,
    on_clear: &M,
    shell: &mut Shell<'_, M>,
) -> bool {
    if matches!(
        event,
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            ..
        })
    ) && child.state.downcast_ref::<InputState>().is_focused()
    {
        shell.publish(on_clear.clone());
        shell.capture_event();
        true
    } else {
        false
    }
}

impl<M: Clone> Widget<M, Theme, Renderer> for Clearable<'_, M> {
    fn size(&self) -> Size<Length> {
        self.input.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.input.as_widget().size_hint()
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(self.input.as_widget())]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.input));
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.input.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.input.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        if intercept_escape(&tree.children[0], event, &self.on_clear, shell) {
            return;
        }
        self.input.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.input.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.input.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, M, Theme, Renderer>> {
        self.input.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::{event, keyboard};

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Message {
        Changed(String),
        Clear,
    }

    fn input() -> Element<'static, Message> {
        clearable(
            iced::widget::text_input("Search", "clock").id("search").on_input(Message::Changed),
            Message::Clear,
        )
    }

    fn key_pressed(key: keyboard::Key) -> Event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            modified_key: key.clone(),
            key,
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::default(),
            text: None,
            repeat: false,
        })
    }

    #[test]
    fn focused_escape_publishes_clear_captures_event_and_keeps_native_focus() {
        let input = input();
        let mut tree = Tree::new(input.as_widget());
        tree.children[0].state.downcast_mut::<InputState>().focus();
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);

        assert!(intercept_escape(
            &tree.children[0],
            &key_pressed(keyboard::Key::Named(keyboard::key::Named::Escape)),
            &Message::Clear,
            &mut shell,
        ));
        assert_eq!(shell.event_status(), event::Status::Captured);
        assert_eq!(messages, [Message::Clear]);
        assert!(tree.children[0].state.downcast_ref::<InputState>().is_focused());
    }

    #[test]
    fn unfocused_escape_is_left_for_other_widgets() {
        let input = input();
        let tree = Tree::new(input.as_widget());
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);

        assert!(!intercept_escape(
            &tree.children[0],
            &key_pressed(keyboard::Key::Named(keyboard::key::Named::Escape)),
            &Message::Clear,
            &mut shell,
        ));
        assert_eq!(shell.event_status(), event::Status::Ignored);
        assert!(messages.is_empty());
        assert!(!tree.children[0].state.downcast_ref::<InputState>().is_focused());
    }

    #[test]
    fn focused_input_leaves_typing_shortcuts_and_escape_release_untouched() {
        let input = input();
        let mut tree = Tree::new(input.as_widget());
        tree.children[0].state.downcast_mut::<InputState>().focus();
        let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
        let events = [
            key_pressed(keyboard::Key::Character("a".into())),
            Event::Keyboard(keyboard::Event::ModifiersChanged(keyboard::Modifiers::CTRL)),
            Event::Keyboard(keyboard::Event::KeyReleased {
                key: escape.clone(),
                modified_key: escape,
                physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Escape),
                location: keyboard::Location::Standard,
                modifiers: keyboard::Modifiers::default(),
            }),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        ];
        for event in events {
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            assert!(!intercept_escape(&tree.children[0], &event, &Message::Clear, &mut shell));
            assert_eq!(shell.event_status(), event::Status::Ignored);
            assert!(messages.is_empty());
        }
        assert!(tree.children[0].state.downcast_ref::<InputState>().is_focused());
    }

    #[test]
    fn rebuilding_after_clear_preserves_native_focus_for_the_next_escape() {
        let input = input();
        let mut tree = Tree::new(input.as_widget());
        tree.children[0].state.downcast_mut::<InputState>().focus();
        let cleared = clearable(
            iced::widget::text_input("Search", "").id("search").on_input(Message::Changed),
            Message::Clear,
        );
        tree.diff(cleared.as_widget());
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);

        assert!(intercept_escape(
            &tree.children[0],
            &key_pressed(keyboard::Key::Named(keyboard::key::Named::Escape)),
            &Message::Clear,
            &mut shell,
        ));
        assert_eq!(shell.event_status(), event::Status::Captured);
        assert_eq!(messages, [Message::Clear]);
        assert!(tree.children[0].state.downcast_ref::<InputState>().is_focused());
    }
}
