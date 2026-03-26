# Wrapper toolchain for armv7-linux-androideabi.
#
# See aarch64-android.toolchain.cmake for rationale.

set(ANDROID_ABI "armeabi-v7a")
set(ANDROID_PLATFORM "android-24")
include($ENV{NDK_HOME}/build/cmake/android.toolchain.cmake)
