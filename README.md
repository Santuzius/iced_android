# iced_android

Runs [iced](https://github.com/iced-rs/iced) apps on Android with iced's normal runtime: `iced::application` or `iced::daemon`, `Task`, `Subscription`, the same code as on desktop.

It has two parts:
- the [iced fork](https://github.com/Santuzius/iced/tree/android-0.14), branch `android-0.14`, which runs iced's winit shell on Android and adds touch support to some widgets
- this crate, which connects iced to the parts of Android that only Java can reach, through `IcedActivity` (a `NativeActivity` subclass in `java/`)

A complete app is the [Runtime example](https://github.com/Santuzius/android-iced-example/tree/main/Runtime) in android-iced-example.

## What works

- soft keyboard: predictions, autocorrect, swipe typing and capitalisation, with the text around the cursor passed to the keyboard; password and terminal modes
- long press in `text_input` and `text_editor` selects a word and opens Android's Cut/Copy/Paste/Select all menu; dragging after the long press extends the selection
- `text_editor`: tap to place the cursor, drag to scroll
- `scrollable`: fling (it keeps scrolling after a swipe), and swipes that start on a button scroll instead of pressing it
- clipboard (`clipboard::read`/`write`)
- insets: the app draws behind the status bar, navigation bar, display cutout and keyboard and pads its content by [`insets()`]
- light/dark system-bar icons, system dark mode (`system::theme`, `theme_changes`), back gesture (as `Key::Named(BrowserBack)`)
- pause/resume (Home, app switcher) and rotation without restarting the app
- fonts from `/system/fonts`, since iced finds system fonts only through fontconfig

Tested on Android 8.0 (emulator), 13 (Pixel 4a) and 17 (emulator, 16 KB pages).

## Setup

**Cargo.toml.** Use iced from the fork; every iced crate must come from the same place:

```toml
[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
iced = { git = "https://github.com/Santuzius/iced", branch = "android-0.14", features = ["x11"] }
iced_android = { git = "https://github.com/Santuzius/iced_android" }
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
// On theme changes: iced_android::set_system_bars_dark(is_dark)
// On Back at the top level: iced_android::move_to_background()
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

## Limitations

- No selection handles (the drag markers at both ends of a selection): after a long press, the selection can only be extended by dragging before lifting.
- Android 15+ ships its colour emoji only as a COLRv1 font, which iced cannot draw; emoji then show as boxes or in black and white. ⚠ ✓ ✗ → ↻ work everywhere. ⟳ (U+27F3) is in no Android system font.

## License

MIT
