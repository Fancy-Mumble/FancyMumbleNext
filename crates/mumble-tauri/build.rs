fn main() {
    tauri_build::build();

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
    {
        println!("cargo:rustc-link-lib=delayimp");
        println!("cargo:rustc-link-arg=/DELAYLOAD:comctl32.dll");
    }
}
