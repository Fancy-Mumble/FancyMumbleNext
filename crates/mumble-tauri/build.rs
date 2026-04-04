//! Build script for the `mumble-tauri` crate.
//!
//! Invokes `tauri-build` and configures platform-specific linker flags.
//! On desktop, also builds the AGPL-isolated `signal-bridge` cdylib from
//! its separate workspace and copies the resulting library next to the
//! executable so `load_signal_bridge` finds it at runtime.
fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    // Build signal-bridge BEFORE tauri_build::build() so that the
    // library file exists when Tauri validates bundle resource globs
    // (TAURI_CONFIG -> bundle.resources -> "signal-bridge/*.dll" etc.).
    if target_os != "android" && std::env::var("SKIP_SIGNAL_BRIDGE").is_err() {
        build_signal_bridge();
    }

    tauri_build::build();

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

/// Build the signal-bridge cdylib from its separate workspace and copy
/// the output library next to the mumble-tauri executable.
fn build_signal_bridge() {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
            panic!("CARGO_MANIFEST_DIR must be set in build scripts");
        });
    let bridge_dir = std::path::Path::new(&manifest_dir).join("../signal-bridge");

    // If the signal-bridge crate is not present (e.g. shallow checkout),
    // skip silently.
    if !bridge_dir.join("Cargo.toml").exists() {
        println!("cargo:warning=signal-bridge crate not found at {}, skipping", bridge_dir.display());
        return;
    }

    // Re-run this build script when signal-bridge sources change.
    println!("cargo:rerun-if-changed=../signal-bridge/src");
    println!("cargo:rerun-if-changed=../signal-bridge/Cargo.toml");
    println!("cargo:rerun-if-env-changed=SKIP_SIGNAL_BRIDGE");

    // Match the current profile: use --release when we are building in
    // release mode, otherwise default (debug).
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let mut cmd = std::process::Command::new("cargo");
    let _ = cmd.arg("build").current_dir(&bridge_dir);
    if profile == "release" {
        let _ = cmd.arg("--release");
    }

    eprintln!("building signal-bridge ({profile})...");
    let status = cmd.status().unwrap_or_else(|e| {
        panic!("failed to run `cargo build` for signal-bridge: {e}");
    });
    if !status.success() {
        panic!("signal-bridge build failed (exit code: {status})");
    }

    // Determine library filename and source path.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let lib_name = match target_os.as_str() {
        "windows" => "signal_bridge.dll",
        "macos" => "libsignal_bridge.dylib",
        _ => "libsignal_bridge.so",
    };

    // signal-bridge has its own target/ directory because it is workspace-excluded.
    let bridge_lib = bridge_dir.join("target").join(&profile).join(lib_name);
    if !bridge_lib.exists() {
        panic!(
            "signal-bridge library not found at {} after build",
            bridge_lib.display()
        );
    }

    // Copy next to the mumble-tauri executable (workspace target/{profile}/).
    // OUT_DIR is inside target/{profile}/build/mumble-tauri-*/out/ -- walk
    // up to reach target/{profile}/.
    let out_dir =
        std::env::var("OUT_DIR").unwrap_or_else(|_| {
            panic!("OUT_DIR must be set in build scripts");
        });
    let out_path = std::path::Path::new(&out_dir);
    // target/{profile}/build/crate-hash/out -> target/{profile}
    let target_profile_dir = out_path
        .ancestors()
        .find(|p| p.file_name().map(|n| n == "debug" || n == "release").unwrap_or(false))
        .unwrap_or_else(|| {
            panic!("could not locate target/{profile} from OUT_DIR={out_dir}");
        });

    let dest = target_profile_dir.join(lib_name);
    let _ = std::fs::copy(&bridge_lib, &dest).unwrap_or_else(|e| {
        panic!(
            "failed to copy {} -> {}: {e}",
            bridge_lib.display(),
            dest.display()
        );
    });
    eprintln!("copied signal-bridge to {}", dest.display());

    // Also copy into the signal-bridge/ subdirectory next to the crate
    // root so that `cargo tauri build` can include it as a bundled
    // resource (bundle.resources: ["signal-bridge/*.dll"]).
    let bundle_dir = std::path::Path::new(&manifest_dir).join("signal-bridge");
    let _ = std::fs::create_dir_all(&bundle_dir);
    let bundle_dest = bundle_dir.join(lib_name);
    let _ = std::fs::copy(&bridge_lib, &bundle_dest).unwrap_or_else(|e| {
        panic!(
            "failed to copy {} -> {}: {e}",
            bridge_lib.display(),
            bundle_dest.display()
        );
    });
    eprintln!("copied signal-bridge to {}", bundle_dest.display());
}
