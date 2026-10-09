//! Scrolling the focused text field into view, as Android does when the keyboard opens.
use iced_core::widget::operation::{Focusable, Operation, Outcome, Scrollable};
use iced_core::widget::{Id, operation::scrollable::AbsoluteOffset};
use iced_core::{Rectangle, Vector};
use iced_runtime::Task;

/// Space kept between the focused widget and the edge of the visible area, in logical pixels.
const MARGIN: f32 = 16.0;

/// Scrolls every `scrollable` around the focused widget (usually a text field) just far enough to show it.
///
/// Run it when the keyboard insets grow (see [`insets`](crate::insets)): the keyboard shrinks the visible area and may cover the field being typed into.
pub fn scroll_to_focused<T: Send + 'static>() -> Task<T> {
    iced_runtime::task::widget(FindFocused { bounds: None })
}

/// First pass: where the focused widget is.
struct FindFocused {
    bounds: Option<Rectangle>,
}

impl<T: Send + 'static> Operation<T> for FindFocused {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
        operate(self);
    }

    fn focusable(&mut self, _id: Option<&Id>, bounds: Rectangle, state: &mut dyn Focusable) {
        if state.is_focused() {
            self.bounds = Some(bounds);
        }
    }

    fn finish(&self) -> Outcome<T> {
        match self.bounds {
            Some(target) => Outcome::Chain(Box::new(Reveal { target })),
            None => Outcome::None,
        }
    }
}

/// Second pass: scroll the scrollables that contain it.
struct Reveal {
    target: Rectangle,
}

impl<T: Send + 'static> Operation<T> for Reveal {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
        operate(self);
    }

    fn scrollable(&mut self, _id: Option<&Id>, bounds: Rectangle, content_bounds: Rectangle, translation: Vector, state: &mut dyn Scrollable) {
        // Layout positions inside a scrollable ignore its scroll offset, so the target is measured from the content's top.
        if !content_bounds.contains(self.target.center()) {
            return;
        }

        let top = self.target.y - content_bounds.y;
        let bottom = top + self.target.height;
        let visible_top = translation.y;
        let visible_bottom = translation.y + bounds.height;

        let offset = if top - MARGIN < visible_top {
            top - MARGIN
        } else if bottom + MARGIN > visible_bottom {
            // Taller than the visible area: show its top.
            (bottom + MARGIN - bounds.height).min(top - MARGIN)
        } else {
            return;
        };

        state.scroll_to(AbsoluteOffset { x: None, y: Some(offset.max(0.0)) });
    }
}
