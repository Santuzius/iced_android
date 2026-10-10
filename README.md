# iced_android

Runs [iced](https://github.com/iced-rs/iced) apps on Android with iced's normal runtime: `iced::application` or `iced::daemon`, `Task`, `Subscription`, the same code as on desktop.

It has two parts:
- the [iced fork](https://github.com/Santuzius/iced/tree/android-0.14), branch `android-0.14`, which runs iced's winit shell on Android and adds touch support to some widgets
- this crate, which connects iced to the parts of Android that only Java can reach, through `IcedActivity` (a `NativeActivity` subclass in `java/`)

A complete app is the [runtime example](examples/runtime) (see [Example](#example) below).

## What works

- soft keyboard: predictions, autocorrect, swipe typing and capitalisation, with the text around the cursor passed to the keyboard (password fields pass it masked); the word being typed goes straight into the text; password and terminal modes
- [`scroll_to_focused()`]: scrolls the focused text field into view when the keyboard opens
- long press in `text_input` and `text_editor` selects a word and opens Android's Cut/Copy/Paste/Select all menu; dragging after the long press extends the selection
- `text_editor`: tap to place the cursor, drag to scroll
- `scrollable`: fling (it keeps scrolling after a swipe), and swipes that start on a button scroll instead of pressing it; scrolling keeps the focused field and the keyboard
- `tooltip`: opens after a long press and closes on lift, without pressing the button underneath
- `pick_list`: opens on lift, so swipes that start on it scroll
- clipboard (`clipboard::read`/`write`)
- insets: the app draws behind the status bar, navigation bar, display cutout and keyboard and pads its content by [`insets()`]
- [`font_scale()`]: the system font size, which iced's logical pixels do not follow by themselves; scale by it, e.g. with `scale_factor`
- light/dark system-bar icons, system dark mode (`system::theme`, `theme_changes`), back gesture (as `Key::Named(BrowserBack)`)
- pause/resume (Home, app switcher) and rotation without restarting the app
- [`foreground()`]: tells the app when it can no longer be seen, so it can close its window, which frees iced's GPU resources, and open it again on return (see the example)
- the app keeps running when Android destroys the activity (e.g. swiped away from the recent apps while a foreground service keeps the process alive); the next activity attaches to it instead of crashing with winit's `RecreationAttempt`. This needs the [android-activity fork](#android-activity) below
- fonts from `/system/fonts`, since iced finds system fonts only through fontconfig; mapped from the files, not copied to the heap, and their pages released from the app's memory while it is in the background (13 MB less on a Pixel 4a with Android 13); colour emoji on Android 15+ with the `emoji` feature (see below)

Tested on Android 8.0 (emulator), 13 (Pixel 4a) and 17 (emulator, 16 KB pages); details below.

## Setup

**Cargo.toml.** Use iced from the fork; every iced crate must come from the same place:

```toml
[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
iced = { git = "https://github.com/Santuzius/iced", branch = "android-0.14", features = ["x11"] }
iced_android = { git = "https://github.com/Santuzius/iced_android" }
```

<a id="android-activity"></a>Also use the android-activity fork, branch `process-lifetime-0.6`, which lets `android_main` outlive a destroyed activity (`iced_android::init` turns this on). winit allows only one event loop per process ([winit#3325](https://github.com/rust-windowing/winit/issues/3325)), so without it a second activity in the same process cannot start iced again:

```toml
[patch.crates-io]
android-activity = { git = "https://github.com/Santuzius/android-activity", branch = "process-lifetime-0.6" }
```

`x11` (or `wayland`) only satisfies iced's check that Unix targets choose a display server; nothing of it is built for Android. If other dependencies use iced from crates.io, redirect them to the fork too:

```toml
[patch.crates-io]
iced = { git = "https://github.com/Santuzius/iced", branch = "android-0.14" }
iced_core = { git = "https://github.com/Santuzius/iced", branch = "android-0.14" }
# … every iced_* crate they use
```

**Rust.** Start the app from `android_main`, and use the crate's helpers. They do nothing on other platforms, so no `cfg` is needed:

```rust
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: iced_android::AndroidApp) {
    iced_android::init(app);
    let _ = run();
}

pub fn run() -> iced::Result {
    let mut app = iced::application(App::new, App::update, App::view)
        .subscription(App::subscription)
        .default_font(iced_android::DEFAULT_FONT);

    for font in iced_android::fonts() {
        app = app.font(font);
    }

    app.run()
}

// In the subscription: iced_android::insets().map(Message::Insets)
// In the view: container(content).padding(self.insets)
// On Message::Insets, if insets.bottom grew (keyboard opened): return iced_android::scroll_to_focused()
// On theme changes: iced_android::set_system_bars_dark(is_dark)
// On Back at the top level: iced_android::move_to_background()
// In the subscription: iced_android::foreground().map(Message::Foreground); on false close the window, on true open it again
```

**Gradle.** Compile `IcedActivity` from wherever Cargo checked the crate out (app/build.gradle):

```groovy
android {
    sourceSets.main.java.srcDirs += icedAndroidJava()
}

def icedAndroidJava() {
    def metadata = providers.exec {
        commandLine 'cargo', 'metadata', '--format-version', '1', '--manifest-path', "${rootDir}/Cargo.toml"
    }.standardOutput.asText.get()
    def crate = new groovy.json.JsonSlurper().parseText(metadata).packages.find { it.name == 'iced_android' }
    return new File(crate.manifest_path).parentFile.toPath().resolve('java').toString()
}
```

**AndroidManifest.xml.** Use `IcedActivity` (or a subclass) with the library name and these attributes:

```xml
<application android:enableOnBackInvokedCallback="true" android:theme="@android:style/Theme.DeviceDefault.NoActionBar" …>
    <activity
        android:name="io.github.santuzius.icedandroid.IcedActivity"
        android:configChanges="orientation|screenSize|screenLayout|smallestScreenSize|density|keyboard|keyboardHidden|navigation|uiMode|locale|layoutDirection|fontScale"
        android:windowSoftInputMode="adjustResize|stateHidden"
        android:exported="true">
        <!-- intent-filter MAIN/LAUNCHER -->
        <meta-data android:name="android.app.lib_name" android:value="your_lib_name" />
    </activity>
</application>
```

`configChanges` keeps rotation and dark mode from restarting the activity, which would restart `android_main`.

## Emoji on Android 15+

iced draws bitmap (CBDT) and COLRv0 colour fonts, but not COLRv1. Up to Android 14 the system has a bitmap emoji font; Android 15+ ships its emoji only as COLRv1. The `emoji` feature bundles Mozilla's [Twemoji](https://github.com/mozilla/twemoji-colr) font (COLRv0, 1.5 MB) and loads it only when the system has no usable emoji font:

```toml
iced_android = { git = "https://github.com/Santuzius/iced_android", features = ["emoji"] }
```

Its art is licensed CC-BY 4.0, which requires credit: show "Twemoji by Twitter, CC-BY 4.0" somewhere in the app, e.g. in an About screen. Without the feature, emoji on Android 15+ show as boxes or in black and white; ⚠ ✓ ✗ → ↻ work everywhere in any case. ⟳ (U+27F3) is in no Android system font.

## Example

[examples/runtime](examples/runtime) is one iced app (`iced::daemon`) for desktop and Android, with three pages that exercise what [Celeste](https://github.com/Santuzius/celeste) needs:
- runtime: `Task::perform`, `Subscription::run` with `stream::channel` fed from a worker thread, `time::every`, `window::close_events`, `system::theme` and `theme_changes`, `keyboard::listen`, `clipboard::read`/`write`
- widgets: `button`, `container`, `text_input` (also secure), `text_editor`, `tooltip`, `pick_list`, `svg` (Tabler icons via `icondata`, gradients), `scrollable`, `opaque` + `stack` modal, `center`, `toggler`, `rich_text`, `mouse_area`
- Android: everything under [What works](#what-works)

Files:
- `src/app.rs`: the iced app, platform-independent; `cargo run --bin desktop` runs it on Linux
- `src/lib.rs`: `android_main`, which hands over to `iced_android`
- `app/build.gradle`, `app/src/main/AndroidManifest.xml`: the setup described above, with `iced_android` as a path dependency

Build and run:

```bash
nix-shell                               # in the repository root: Android SDK, NDK, emulator, JDK, Gradle (NixOS); elsewhere set ANDROID_HOME and ANDROID_NDK_HOME yourself
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cd examples/runtime
./build.sh                              # all ABIs, release; ./build.sh debug x86_64 for the emulator only
adb install -r app/build/outputs/apk/release/app-release.apk
adb logcat -s iced-runtime              # Rust log and panics
```

Emulators (x86_64 images for API 26, 33, 36 and 37.0 are in `shell.nix`):

```bash
avdmanager create avd -n api26 -k "system-images;android-26;google_apis;x86_64" -d pixel_4a
avdmanager create avd -n api37 -k "system-images;android-37.0;google_apis_ps16k;x86_64" -d pixel_4a
emulator -avd api26 -gpu swiftshader_indirect
```

To work on the iced fork locally, point Cargo at a local clone in `examples/runtime/.cargo/config.toml` (gitignored):

```toml
[patch."https://github.com/Santuzius/iced"]
iced = { path = "../../../iced" }
iced_core = { path = "../../../iced/core" }
iced_futures = { path = "../../../iced/futures" }
iced_runtime = { path = "../../../iced/runtime" }
iced_widget = { path = "../../../iced/widget" }
iced_winit = { path = "../../../iced/winit" }
```

Tested:

| Device | Android | Graphics | Result |
|---|---|---|---|
| Pixel 4a | 13 (GrapheneOS), arm64 | wgpu, Vulkan | everything above |
| Emulator API 26 | 8.0, x86_64 | wgpu, OpenGL ES | starts, keyboard and typing, fonts (an earlier version) |
| Emulator API 37.0 | 17, x86_64, 16 KB pages | wgpu, OpenGL ES | starts, `Task`, keyboard and typing (an earlier version); colour emoji with Twemoji |
| Galaxy A3 (2017) | 8.0, armv7 | – | not tested; the APK contains `armeabi-v7a` |

## Limitations

- No selection handles (the drag markers at both ends of a selection): after a long press, the selection can only be extended by dragging before lifting.
- `mouse_area` fires `on_press` on touch-down, so a swipe that starts on it still presses it; use `on_release` for touch-friendly areas.

## Origin

This started as a fork of [ibaryshnikov/android-iced-example](https://github.com/ibaryshnikov/android-iced-example) (MIT). The example's Gradle and manifest skeleton and its launcher icons come from there, and the Rust ↔ Java calls for keyboard and clipboard follow its approach; hence its copyright line in `LICENSE`.

## License

MIT, except the bundled Twemoji font in `fonts/`: Apache 2.0 (font) and CC-BY 4.0 (art), see `fonts/LICENSE-Twemoji.md`.
