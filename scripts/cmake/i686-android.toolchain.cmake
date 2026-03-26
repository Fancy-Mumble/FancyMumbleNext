# Wrapper toolchain for i686-linux-android.
#
# See aarch64-android.toolchain.cmake for rationale.

set(ANDROID_ABI "x86")
set(ANDROID_PLATFORM "android-24")
include($ENV{NDK_HOME}/build/cmake/android.toolchain.cmake)
