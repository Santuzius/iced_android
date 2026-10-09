#!/usr/bin/env bash
# Builds the Rust library for the given ABIs and packs it into an APK; run inside `nix-shell` from the repository root.
#   ./build.sh                     all three ABIs, release
#   ./build.sh debug x86_64        one ABI (emulator), unoptimised Rust
# The APK ends up in app/build/outputs/apk/<profile>/.
set -euo pipefail
cd "$(dirname "$0")"

profile=${1:-release}
shift || true
abis=("${@:-arm64-v8a armeabi-v7a x86_64}")
targets=()
for abi in ${abis[*]}; do targets+=(-t "$abi"); done

cargo_profile=()
[ "$profile" = release ] && cargo_profile=(--release)

rm -rf app/src/main/jniLibs
# --platform 26 links against Android 8.0's libraries, the minimum the app supports.
cargo ndk --platform 26 "${targets[@]}" -o app/src/main/jniLibs build --lib "${cargo_profile[@]}"

task=assembleRelease
[ "$profile" = debug ] && task=assembleDebug
gradle --quiet "$task"
ls app/build/outputs/apk/"$profile"/*.apk
