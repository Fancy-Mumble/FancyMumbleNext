# Wrapper toolchain for aarch64-linux-android.
#
# cmake-rs does not auto-set ANDROID_ABI when the toolchain file is
# provided via the CMAKE_TOOLCHAIN_FILE_<target> environment variable
# (it only does so when .define() is called in code).  Without this
# wrapper the NDK toolchain defaults to armeabi-v7a, producing
# conflicting --target flags and a broken compiler test.

set(ANDROID_ABI "arm64-v8a")
set(ANDROID_PLATFORM "android-24")
include($ENV{NDK_HOME}/build/cmake/android.toolchain.cmake)

# Disable outlined atomics.  The outlined-atomics helpers in
# libclang_rt.builtins crash with SIGSEGV inside getauxval when the
# shared library is loaded via dlopen (null ELF auxiliary vector).
add_compile_options(-mno-outline-atomics)
