//! Build script for the `mumble-tauri` crate.
//!
//! Invokes `tauri-build` and configures Windows-specific linker flags to
//! delay-load `comctl32.dll`, preventing startup failures in test binaries.
fn main() {
    tauri_build::build();

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    // Oboe (Android audio) is a C++ library whose pure-virtual functions
    // need the C++ runtime (`__cxa_pure_virtual` etc.).  The Rust linker
    // uses NDK clang (C mode) which does NOT auto-link libc++.
    //
    // We link against libc++_shared.so (the NDK's dynamic C++ runtime)
    // rather than libc++_static.a because static linking pulls in CRT
    // builtins whose static constructors (init_have_lse_atomics ->
    // getauxval) crash with SIGSEGV on some ARM64 devices during dlopen.
    //
    // The Tauri CLI automatically detects libc++_shared.so as a NEEDED
    // dependency and symlinks it into the jniLibs dir for APK bundling.
    if target_os == "android" {
        let ndk_home = std::env::var("NDK_HOME")
            .or_else(|_| std::env::var("ANDROID_NDK_HOME"))
            .unwrap_or_else(|_| {
                panic!("NDK_HOME or ANDROID_NDK_HOME must be set for Android builds");
            });

        let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
        let ndk_triple = match target_arch.as_str() {
            "aarch64" => "aarch64-linux-android",
            "arm" => "arm-linux-androideabi",
            "x86_64" => "x86_64-linux-android",
            "x86" => "i686-linux-android",
            other => panic!("unsupported Android arch: {other}"),
        };

        let host = if cfg!(target_os = "linux") {
            "linux-x86_64"
        } else if cfg!(target_os = "windows") {
            "windows-x86_64"
        } else {
            "darwin-x86_64"
        };

        let sysroot_lib = format!(
            "{ndk_home}/toolchains/llvm/prebuilt/{host}/sysroot/usr/lib/{ndk_triple}"
        );

        // Copy libc++_shared.so into OUT_DIR so we can add a clean search
        // path.  Adding {sysroot_lib} directly would also expose libc.a
        // (static bionic) which the linker picks up INSTEAD of the dynamic
        // libc.so (located in the API-level subdirectory).  That pulls in
        // pthread_create, __init_tcb and other internals whose static
        // versions crash with SEGV_ACCERR when loaded via dlopen.
        let out_dir =
            std::env::var("OUT_DIR").unwrap_or_else(|_| {
                panic!("OUT_DIR must be set in build scripts");
            });
        let src = format!("{sysroot_lib}/libc++_shared.so");
        let dst = format!("{out_dir}/libc++_shared.so");
        let _bytes = std::fs::copy(&src, &dst).unwrap_or_else(|e| {
            panic!("failed to copy libc++_shared.so from {src} to {dst}: {e}");
        });
        println!("cargo:rustc-link-search=native={out_dir}");
        println!("cargo:rustc-link-lib=c++_shared");

        // The NDK's libclang_rt.builtins contains outlined-atomics
        // helpers whose constructor (init_have_lse_atomics) calls a
        // statically-linked getauxval that crashes with SIGSEGV on
        // dlopen (null ELF auxiliary vector pointer).  Compile a safe
        // getauxval that reads /proc/self/auxv directly: because our
        // object is linked before the builtins archive, the linker
        // resolves init_have_lse_atomics' reference to our version.
        if target_arch == "aarch64" {
            cc::Build::new()
                .file("src/getauxval_fix.c")
                .flag("-mno-outline-atomics")
                .compile("getauxval_fix");
        }
    }

    // tauri_build embeds a Common Controls v6 manifest into binaries via
    // `cargo:rustc-link-arg-bins`.  The lib-test binary is NOT a "bin"
    // target, so it gets comctl32 v5.82 at runtime which is missing
    // `TaskDialogIndirect` → STATUS_ENTRYPOINT_NOT_FOUND on startup.
    //
    // Fix: delay-load comctl32.dll so the import is resolved lazily
    // instead of at process start.  The real binary's manifest activates
    // comctl32 v6 before any call.  The test binary never calls comctl32
    // functions, so the lazy load never fires and startup succeeds.
    #[cfg(windows)]
    if target_os == "windows" {
        println!("cargo:rustc-link-lib=delayimp");
        println!("cargo:rustc-link-arg=/DELAYLOAD:comctl32.dll");
    }
}
