# Wrapper toolchain for x86_64-linux-android.
#
# See aarch64-android.toolchain.cmake for rationale.

set(ANDROID_ABI "x86_64")
set(ANDROID_PLATFORM "android-24")
include($ENV{NDK_HOME}/build/cmake/android.toolchain.cmake)
