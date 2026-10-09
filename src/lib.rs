//! Runs [iced](https://github.com/iced-rs/iced) apps on Android, together with the Android support in the [iced fork](https://github.com/Santuzius/iced/tree/android-0.14).
//!
//! The fork lets iced's normal runtime (`iced::application`, `iced::daemon`) run on Android. This crate connects it to the parts of Android only Java can reach, through `IcedActivity` (in `java/`):
//! - soft keyboard with predictions and autocorrect, and the Cut/Copy/Paste menu after a long press in a text field
//! - clipboard
//! - insets: the space taken by the status bar, navigation bar, display cutout and keyboard
//! - light/dark system-bar icons, system dark mode, the back gesture
//! - fonts from `/system/fonts`
//!
//! Everything except [`init`] also exists on other platforms, where it does nothing, so apps can call it without `cfg`.
#[cfg(target_os = "android")]
mod android;
#[cfg(not(target_os = "android"))]
mod other;

#[cfg(target_os = "android")]
pub use android::{AndroidApp, init};
#[cfg(target_os = "android")]
use android as platform;
#[cfg(not(target_os = "android"))]
use other as platform;

use iced_core::{Font, Padding};
use iced_futures::Subscription;

use std::borrow::Cow;

/// The system UI font: Roboto on Android, iced's default elsewhere. Pass it to `default_font`.
pub const DEFAULT_FONT: Font = platform::DEFAULT_FONT;

/// Fonts to load at start with `.font(…)`: Roboto, symbols and emoji from `/system/fonts`. iced finds system fonts only through fontconfig, which Android lacks. Empty elsewhere.
pub fn fonts() -> Vec<Cow<'static, [u8]>> {
    platform::fonts()
}

/// The current [`Insets`], and every change. Android draws the app behind the system bars and keyboard, so the app pads its content by them. Never emits elsewhere.
pub fn insets() -> Subscription<Insets> {
    platform::insets()
}

/// What Android's back gesture does at an app's top level: the app keeps running, the launcher comes to the front. Does nothing elsewhere.
pub fn move_to_background() {
    platform::move_to_background();
}

/// Dark status and navigation bar icons on a light app (`false`), light icons on a dark one (`true`). Does nothing elsewhere.
pub fn set_system_bars_dark(dark: bool) {
    platform::set_system_bars_dark(dark);
}

/// Space taken by the status bar, navigation bar, display cutout and soft keyboard, in logical pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Insets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl From<Insets> for Padding {
    fn from(insets: Insets) -> Self {
        Padding { top: insets.top, right: insets.right, bottom: insets.bottom, left: insets.left }
    }
}
