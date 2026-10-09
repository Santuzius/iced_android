# Nix dev shell for building and running the examples on NixOS: Android SDK, NDK, emulator, JDK and Gradle. Enter with `nix-shell`.
# Rust comes from rustup, because the Android standard libraries are installed per target: `rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android`.
{ pkgs ? import <nixpkgs> {
    config = {
      allowUnfree = true;
      android_sdk.accept_license = true;
    };
  }
}:

let
  # Highest API level the Gradle builds compile against; the minimum (26, Android 8.0) is set in each app/build.gradle.
  compileSdk = "36";
  buildTools = "36.0.0";
  # NDK r28+ aligns native libraries to 16 KB pages by default, which Android 15+ devices may require.
  ndk = "28.2.13676358";

  android = pkgs.androidenv.composeAndroidPackages {
    # 26, 33 and 37.0 are only here for their emulator images: Android 8.0 (Galaxy A3 2017), 13 (Pixel 4a) and 17 (Pixel 7).
    platformVersions = [ "26" "33" compileSdk "37.0" ];
    buildToolsVersions = [ buildTools ];
    includeNDK = true;
    ndkVersions = [ ndk ];
    includeEmulator = true;
    includeSystemImages = true;
    # google_apis images include the Play Store keyboard (Gboard); API 37.0 only comes as google_apis_ps16k, with 16 KB memory pages.
    systemImageTypes = [ "google_apis" ];
    abiVersions = [ "x86_64" ];
  };
  sdk = android.androidsdk;
in
pkgs.mkShell {
  nativeBuildInputs = with pkgs; [
    sdk
    jdk17
    gradle
    cargo-ndk
  ];

  ANDROID_HOME = "${sdk}/libexec/android-sdk";
  ANDROID_SDK_ROOT = "${sdk}/libexec/android-sdk";
  ANDROID_NDK_HOME = "${sdk}/libexec/android-sdk/ndk/${ndk}";
  ANDROID_NDK_ROOT = "${sdk}/libexec/android-sdk/ndk/${ndk}";
  JAVA_HOME = pkgs.jdk17.home;
  # The Android Gradle plugin downloads aapt2 from Maven as a dynamically linked binary that NixOS cannot run; use the SDK's patched one.
  GRADLE_OPTS = "-Dorg.gradle.project.android.aapt2FromMavenOverride=${sdk}/libexec/android-sdk/build-tools/${buildTools}/aapt2";
}
