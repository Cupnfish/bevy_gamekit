#!/usr/bin/env bash
# 构建 demo 的 Android APK：cargo-ndk 交叉编译 so -> Gradle 打包
# 用法：scripts/build-android.sh [debug|release] [--install]
set -euo pipefail
cd "$(dirname "$0")/.."

BUILD_TYPE="${1:-release}"
INSTALL_FLAG="${2:-}"

if [ -z "${JAVA_HOME:-}" ]; then
    export JAVA_HOME="C:\\Program Files\\Android\\Android Studio\\jbr"
fi

echo "== [1/2] cargo-ndk 编译 libdemo.so (arm64-v8a, $BUILD_TYPE) =="
cargo ndk -P 26 -t arm64-v8a -o android/app/src/main/jniLibs build --"$BUILD_TYPE" -p demo

# cargo-ndk 不做剥离，release 产物需手动 strip 掉调试段（190MB -> ~85MB）
NDK="$ANDROID_NDK_HOME"
if [ -z "$NDK" ]; then
    NDK="${ANDROID_HOME:-$LOCALAPPDATA/Android/Sdk}/ndk/29.0.14206865"
fi
"$NDK/toolchains/llvm/prebuilt/windows-x86_64/bin/llvm-strip.exe" \
    android/app/src/main/jniLibs/arm64-v8a/libdemo.so

echo "== [2/2] Gradle 打包 APK =="
# 同步 Bevy 资产到 APK assets（bevy_asset 在 Android 上从 APK assets 目录读取）
mkdir -p android/app/src/main/assets
cp -r apps/demo/assets/. android/app/src/main/assets/
# gradle 偶发向旧 APK 追加写入导致体积翻倍，先清理
rm -f android/app/build/outputs/apk/debug/app-debug.apk
(cd android && ./gradlew --no-daemon assembleDebug)

APK="android/app/build/outputs/apk/debug/app-debug.apk"
ls -la "$APK"

if [ "$INSTALL_FLAG" = "--install" ]; then
    SDK="${ANDROID_HOME:-$LOCALAPPDATA/Android/Sdk}"
    "$SDK/platform-tools/adb.exe" install -r "$APK"
    "$SDK/platform-tools/adb.exe" shell am start -n cn.taptap.demo/.NativeActivity
fi
