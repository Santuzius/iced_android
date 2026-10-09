//! The regular iced runtime (`iced::daemon`, `Task`, `Subscription`) on Android, with the features Celeste needs.
//!
//! - `app` is plain iced code and runs unchanged on desktop (`cargo run --bin desktop`) and Android.
//! - The Android support comes from the iced fork (branch `android-0.14`) and the `iced_android` crate with its `IcedActivity`.
mod app;

pub use app::run;

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: iced_android::AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("iced-runtime"));
    std::panic::set_hook(Box::new(|info| log::error!("{info}")));

    iced_android::init(app);

    if let Err(error) = run() {
        log::error!("iced stopped: {error}");
    }
}
