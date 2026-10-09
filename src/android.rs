//! Java ↔ Rust glue. The Java half is java/io/github/santuzius/icedandroid/IcedActivity.java.
//!
//! - Rust → Java: keyboard, text menu, clipboard, system bars and "move to background" call methods on the activity.
//! - Java → Rust: the `native*` functions below run on Android's UI thread and hand their events to the iced runtime through `iced_winit::android`.
use crate::Insets;

use iced_core::input_method::{self, Purpose};
use iced_core::keyboard::key::{Code, Named, Physical};
use iced_core::keyboard::{self, Key, Location, Modifiers};
use iced_core::theme::Mode;
use iced_core::{Event, Font, Point};
use iced_futures::futures::SinkExt;
use iced_futures::{Subscription, stream};
use iced_widget::ime_context;
use iced_winit::android::{self as runtime, Platform};
use jni::objects::{JClass, JObject, JObjectArray, JString, JValue};
use jni::sys::{jboolean, jfloat, jint, JNI_TRUE};
use jni::{JNIEnv, JavaVM};
use std::borrow::Cow;
use std::sync::OnceLock;
use tokio::sync::watch;

pub use iced_winit::android::AndroidApp;

/// Connects the iced runtime to `IcedActivity`. Call it first in `android_main`, before starting the iced app.
pub fn init(app: AndroidApp) {
    // winit allows one event loop per process, so a second `android_main` (after the activity was destroyed, e.g. swiped away from the recent apps, while the process lives on) could not start iced again. With this the running one takes over the next activity instead.
    app.set_outlive_activity(true);
    let _ = APP.set(app.clone());
    runtime::init(app, Java);
    ime_context::set_menu_handler(show_text_menu);
}

/// Android's UI font. Every Android version since 5.0 ships it as /system/fonts/Roboto-Regular.ttf.
pub const DEFAULT_FONT: Font = Font::with_name("Roboto");

/// Each entry lists alternatives, first usable one wins:
/// - Bold Roboto is a separate file up to Android 11; later versions ship one variable font with all weights.
/// - From Android 13 flags are in their own bitmap font, NotoColorEmojiFlags.ttf.
/// - Emoji fonts count only as bitmap (CBDT) fonts. From Android 13 NotoColorEmoji.ttf is a COLRv1 font, which iced cannot draw; the bitmap version moved to NotoColorEmojiLegacy.ttf, which Android 15+ no longer ships. A COLRv1 font must not be loaded at all: it claims ⚠ and other symbols with emoji forms and draws them blank, while NotoSansSymbols-Regular-Subsetted.ttf has them in black and white.
/// - Without a usable system emoji font, the `emoji` feature adds the bundled Twemoji font.
pub fn fonts() -> Vec<Cow<'static, [u8]>> {
    const FILES: [&[&str]; 7] = [
        &["Roboto-Regular.ttf"],
        &["Roboto-Bold.ttf"],
        &["NotoColorEmojiLegacy.ttf", "NotoColorEmoji.ttf"],
        &["NotoColorEmojiFlags.ttf"],
        &["NotoSansSymbols-Regular-Subsetted.ttf"],
        &["NotoSansSymbols-Regular-Subsetted2.ttf"],
        &["DroidSansMono.ttf"],
    ];

    FILES
        .iter()
        .filter_map(|alternatives| {
            let is_emoji = alternatives.iter().any(|file| file.contains("Emoji"));
            let font = alternatives
                .iter()
                .filter_map(|file| map_font(&format!("/system/fonts/{file}")))
                .find(|font| !is_emoji || has_table(font, b"CBDT"))
                .map(Cow::Borrowed);

            // Android 15+ has no emoji font iced can draw. The bundled one takes the system one's place: before the symbol fonts, which also have ☁ ✅ and flags' letters, but only in black and white.
            #[cfg(feature = "emoji")]
            if font.is_none() && alternatives.contains(&"NotoColorEmoji.ttf") {
                log::info!("Using the bundled Twemoji font for emoji");
                return Some(Cow::Borrowed(TWEMOJI));
            }

            if font.is_none() {
                log::info!("Font not found or not usable: {}", alternatives.join(" or "));
            }

            font
        })
        .collect()
}

/// Maps a font file read-only instead of copying it to the heap: its pages are file-backed, so Android can drop them while the app sits in the background, and they are shared with every other process that maps the same system font. Mapped once per process and never unmapped, like a font in the binary.
fn map_font(path: &str) -> Option<&'static [u8]> {
    static MAPPED: std::sync::Mutex<Vec<(String, &'static [u8])>> = std::sync::Mutex::new(Vec::new());

    let mut mapped = MAPPED.lock().ok()?;
    if let Some((_, font)) = mapped.iter().find(|(known, _)| known == path) {
        return Some(font);
    }

    let file = std::fs::File::open(path).ok()?;
    // SAFETY: system fonts are read-only files that do not change while the system runs.
    let map = unsafe { memmap2::Mmap::map(&file) }.ok()?;
    let font: &'static [u8] = &Box::leak(Box::new(map))[..];
    mapped.push((path.to_owned(), font));

    Some(font)
}

/// Mozilla's Twemoji font: colour layers in the COLRv0 format, which iced can draw. Art CC-BY 4.0 by Twitter, font Apache 2.0 by Mozilla; see fonts/LICENSE-Twemoji.md.
#[cfg(feature = "emoji")]
const TWEMOJI: &[u8] = include_bytes!("../fonts/Twemoji.Mozilla.ttf");

/// Whether the font file has a table with this tag, from the table directory at the start of every TrueType/OpenType file.
fn has_table(font: &[u8], tag: &[u8; 4]) -> bool {
    let count = font.get(4..6).map_or(0, |n| u16::from_be_bytes([n[0], n[1]]) as usize);
    (0..count).any(|i| font.get(12 + 16 * i..16 + 16 * i) == Some(tag))
}

pub fn insets() -> Subscription<Insets> {
    Subscription::run(|| {
        stream::channel(4, async |mut output| {
            let mut receiver = insets_channel().subscribe();

            loop {
                let insets = *receiver.borrow_and_update();
                let _ = output.send(insets).await;

                if receiver.changed().await.is_err() {
                    break;
                }
            }
        })
    })
}

pub fn foreground() -> Subscription<bool> {
    Subscription::run(|| {
        stream::channel(4, async |mut output| {
            let mut receiver = foreground_channel().subscribe();

            loop {
                // `None` until the activity reported its first start.
                let foreground = *receiver.borrow_and_update();

                if let Some(foreground) = foreground {
                    let _ = output.send(foreground).await;
                }

                if receiver.changed().await.is_err() {
                    break;
                }
            }
        })
    })
}

fn foreground_channel() -> &'static watch::Sender<Option<bool>> {
    static CHANNEL: OnceLock<watch::Sender<Option<bool>>> = OnceLock::new();
    CHANNEL.get_or_init(|| watch::Sender::new(None))
}

fn insets_channel() -> &'static watch::Sender<Insets> {
    static CHANNEL: OnceLock<watch::Sender<Insets>> = OnceLock::new();
    CHANNEL.get_or_init(|| watch::Sender::new(Insets::default()))
}

pub fn move_to_background() {
    call_activity("moveToBackground", "()V", &[]);
}

pub fn set_system_bars_dark(dark: bool) {
    call_activity("setSystemBarsDark", "(Z)V", &[JValue::Bool(dark.into())]);
}

/// Android's floating Cut/Copy/Paste toolbar, after a long press in a text field.
fn show_text_menu(position: Point, has_selection: bool) {
    call_activity("showTextMenu", "(FFZ)V", &[JValue::Float(position.x), JValue::Float(position.y), JValue::Bool(has_selection.into())]);
}

static APP: OnceLock<AndroidApp> = OnceLock::new();

struct Java;

impl Platform for Java {
    fn show_keyboard(&self, purpose: Purpose) {
        let purpose = match purpose {
            Purpose::Normal => 0,
            Purpose::Secure => 1,
            Purpose::Terminal => 2,
        };
        call_activity("showKeyboard", "(I)V", &[JValue::Int(purpose)]);
    }

    fn hide_keyboard(&self) {
        call_activity("hideKeyboard", "()V", &[]);
    }

    fn read_clipboard(&self) -> Option<String> {
        with_activity(|env, activity| {
            let value = env.call_method(activity, "readClipboard", "()Ljava/lang/String;", &[])?.l()?;

            if value.is_null() {
                return Ok(None);
            }

            Ok(Some(env.get_string(&JString::from(value))?.into()))
        })
        .flatten()
    }

    fn write_clipboard(&self, contents: String) {
        with_activity(|env, activity| {
            let contents = env.new_string(contents)?;
            env.call_method(activity, "writeClipboard", "(Ljava/lang/String;)V", &[JValue::Object(&contents)])?;
            Ok(())
        });
    }

    fn touch_started(&self) {
        call_activity("hideTextMenu", "()V", &[]);
    }
}

fn call_activity(name: &str, signature: &str, args: &[JValue]) {
    with_activity(|env, activity| {
        env.call_method(activity, name, signature, args)?;
        Ok(())
    });
}

/// Runs `f` with the activity on the current thread, attaching it to the Java VM first. Logs Java exceptions instead of leaving them pending.
fn with_activity<T>(f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<T>) -> Option<T> {
    // Not ndk_context::android_context(): with android-activity its context is the Application, which lacks IcedActivity's methods.
    let app = APP.get()?;
    // SAFETY: android-activity keeps the VM and a global reference to the activity alive while android_main runs.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }.ok()?;
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let mut env = vm.attach_current_thread_permanently().ok()?;

    match f(&mut env, &activity) {
        Ok(value) => Some(value),
        Err(error) => {
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_describe();
                let _ = env.exception_clear();
            }

            log::error!("Java call failed: {error}");
            None
        }
    }
}

// Java → Rust. Names follow JNI's `Java_<package>_<class>_<method>` scheme for the static native methods in IcedActivity.

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_santuzius_icedandroid_IcedActivity_nativeImeCommit(mut env: JNIEnv, _: JClass, text: JString) {
    let Ok(text) = env.get_string(&text).map(String::from) else {
        return;
    };
    runtime::send_event(Event::InputMethod(input_method::Event::Commit(text)));
}

/// Keys the input connection needs: 0 = Backspace, 1 = Delete, 2 = Enter, 3 = Left, 4 = Right, 5 = Shift+Right.
#[unsafe(no_mangle)]
extern "system" fn Java_io_github_santuzius_icedandroid_IcedActivity_nativeImeKey(_: JNIEnv, _: JClass, key: jint) {
    let (named, code, modifiers) = match key {
        0 => (Named::Backspace, Code::Backspace, Modifiers::empty()),
        1 => (Named::Delete, Code::Delete, Modifiers::empty()),
        3 => (Named::ArrowLeft, Code::ArrowLeft, Modifiers::empty()),
        4 => (Named::ArrowRight, Code::ArrowRight, Modifiers::empty()),
        5 => (Named::ArrowRight, Code::ArrowRight, Modifiers::SHIFT),
        _ => (Named::Enter, Code::Enter, Modifiers::empty()),
    };
    press(Key::Named(named), Physical::Code(code), modifiers);
}

/// A text menu entry, as the shortcut iced's text fields know: 0 = cut, 1 = copy, 2 = paste, 3 = select all.
#[unsafe(no_mangle)]
extern "system" fn Java_io_github_santuzius_icedandroid_IcedActivity_nativeShortcut(_: JNIEnv, _: JClass, action: jint) {
    let (letter, code) = match action {
        0 => ("x", Code::KeyX),
        1 => ("c", Code::KeyC),
        2 => ("v", Code::KeyV),
        _ => ("a", Code::KeyA),
    };
    press(Key::Character(letter.into()), Physical::Code(code), Modifiers::CTRL);
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_santuzius_icedandroid_IcedActivity_nativeBack(_: JNIEnv, _: JClass) {
    // The same key winit reports for KEYCODE_BACK, so apps handle both paths alike.
    press(Key::Named(Named::BrowserBack), Physical::Code(Code::BrowserBack), Modifiers::empty());
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_santuzius_icedandroid_IcedActivity_nativeInsets(_: JNIEnv, _: JClass, top: jfloat, right: jfloat, bottom: jfloat, left: jfloat) {
    insets_channel().send_replace(Insets { top, right, bottom, left });
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_santuzius_icedandroid_IcedActivity_nativeForeground(_: JNIEnv, _: JClass, foreground: jboolean) {
    foreground_channel().send_replace(Some(foreground == JNI_TRUE));
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_santuzius_icedandroid_IcedActivity_nativeNightMode(_: JNIEnv, _: JClass, night: jboolean) {
    runtime::set_system_theme(if night == JNI_TRUE { Mode::Dark } else { Mode::Light });
}

/// The focused iced field's text as `[before, selected, after]`, so the keyboard's predictions see the words already there; masked in password fields, `null` without a focused field.
#[unsafe(no_mangle)]
extern "system" fn Java_io_github_santuzius_icedandroid_IcedActivity_nativeTextContext<'local>(mut env: JNIEnv<'local>, _: JClass) -> JObjectArray<'local> {
    let null = || JObjectArray::from(JObject::null());
    let Some(context) = ime_context::get() else {
        return null();
    };

    let array = || -> jni::errors::Result<JObjectArray<'local>> {
        let array = env.new_object_array(3, "java/lang/String", JObject::null())?;

        for (index, text) in [context.before, context.selected, context.after].into_iter().enumerate() {
            let text = env.new_string(text)?;
            env.set_object_array_element(&array, index as i32, text)?;
        }

        Ok(array)
    };

    array().unwrap_or_else(|_| null())
}

/// Whether Backspace and Delete remove whole graphemes in the focused field (see `ime_context::Context`).
#[unsafe(no_mangle)]
extern "system" fn Java_io_github_santuzius_icedandroid_IcedActivity_nativeDeletesGraphemes(_: JNIEnv, _: JClass) -> jboolean {
    ime_context::get().is_none_or(|context| context.deletes_graphemes).into()
}

/// Presses and releases a key. text_input reads Ctrl and Shift from its last ModifiersChanged event, text_editor from the key event, so modified keys send both.
fn press(key: Key, physical_key: Physical, modifiers: Modifiers) {
    let text = match &key {
        Key::Named(Named::Enter) => Some("\r".into()),
        _ => None,
    };

    if !modifiers.is_empty() {
        runtime::send_event(Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)));
    }

    runtime::send_event(Event::Keyboard(keyboard::Event::KeyPressed { key: key.clone(), modified_key: key.clone(), physical_key, location: Location::Standard, modifiers, text, repeat: false }));
    runtime::send_event(Event::Keyboard(keyboard::Event::KeyReleased { key: key.clone(), modified_key: key, physical_key, location: Location::Standard, modifiers }));

    if !modifiers.is_empty() {
        runtime::send_event(Event::Keyboard(keyboard::Event::ModifiersChanged(Modifiers::empty())));
    }
}
