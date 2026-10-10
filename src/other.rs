//! Platforms without Java bridge: everything does nothing.
use crate::Insets;

use iced_core::Font;
use iced_futures::Subscription;

use std::borrow::Cow;

pub const DEFAULT_FONT: Font = Font::DEFAULT;

pub fn fonts() -> Vec<Cow<'static, [u8]>> {
    Vec::new()
}

pub fn insets() -> Subscription<Insets> {
    Subscription::none()
}

pub fn foreground() -> Subscription<bool> {
    Subscription::none()
}

pub fn system_scale() -> Subscription<f32> {
    Subscription::none()
}

pub fn move_to_background() {}

pub fn set_system_bars_dark(_dark: bool) {}
