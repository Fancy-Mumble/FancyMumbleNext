//! Linux-specific `WebKitGTK` / `AppImage` environment workarounds.

use super::desktop;

/// Captures the `AppImage` runtime environment at detection time.
#[derive(Debug)]
struct AppImageEnv {
    appdir: String,
}

impl AppImageEnv {
    /// Returns `Some` when the process is running inside an `AppImage` bundle
    /// (either `APPIMAGE` or `APPDIR` env var is set).
    fn detect() -> Option<Self> {
        let appdir = std::env::var("APPDIR")
            .ok()
            .filter(|v| !v.is_empty());
        let in_appimage = appdir.is_some() || std::env::var_os("APPIMAGE").is_some();
        in_appimage.then(|| Self {
            appdir: appdir.unwrap_or_default(),
        })
    }

    /// Applies all environment variable workarounds before `GTK` starts.
    fn apply_workarounds(&self) {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        // AppImage-specific workaround for blank windows on some NVIDIA setups.
        if std::env::var_os("__NV_DISABLE_EXPLICIT_SYNC").is_none() {
            std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1");
        }
        if let Some(new_ld) = self.host_first_library_path() {
            std::env::set_var("LD_LIBRARY_PATH", new_ld);
        }
        self.set_webkit_exec_path();
        self.set_wayland_backend();
    }

    /// Returns `LD_LIBRARY_PATH` with host system directories first.
    ///
    /// `linuxdeploy` puts `AppDir` paths first, but the bundle includes
    /// `WebKit` without matching `libEGL`/`libGL`, causing ABI mismatches
    /// and grey rendering.  Hoisting known host dirs fixes this; `AppDir`
    /// paths remain as a fallback.  Returns `None` when `LD_LIBRARY_PATH`
    /// is unset.
    fn host_first_library_path(&self) -> Option<String> {
        let current = match std::env::var("LD_LIBRARY_PATH") {
            Ok(v) if !v.is_empty() => v,
            _ => return None,
        };

        let host_dirs = [
            "/usr/lib/x86_64-linux-gnu",
            "/usr/lib64",
            "/usr/lib",
            "/lib/x86_64-linux-gnu",
            "/lib64",
        ];

        let mut host_entries: Vec<String> = Vec::new();
        let mut appdir_entries: Vec<String> = Vec::new();
        let mut other_entries: Vec<String> = Vec::new();

        for entry in current.split(':') {
            if entry.is_empty() {
                continue;
            }
            if !self.appdir.is_empty() && entry.starts_with(&self.appdir) {
                appdir_entries.push(entry.to_owned());
            } else if host_dirs.contains(&entry) {
                host_entries.push(entry.to_owned());
            } else {
                other_entries.push(entry.to_owned());
            }
        }

        for dir in &host_dirs {
            if std::path::Path::new(dir).is_dir() && !host_entries.iter().any(|e| e == dir) {
                host_entries.push((*dir).to_owned());
            }
        }

        // Host first, then non-appdir extras, then appdir as fallback.
        let mut merged = host_entries;
        merged.extend(other_entries);
        merged.extend(appdir_entries);

        Some(merged.join(":"))
    }

    /// In AppImage on Wayland, force `GDK_BACKEND=wayland` to override
    /// the `x11` default from `pre_init` / `linuxdeploy`.
    fn set_wayland_backend(&self) {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            std::env::set_var("GDK_BACKEND", "wayland");
        }
    }

    /// Points `WEBKIT_EXEC_PATH` at the system `WebKit` helper directory so
    /// `WebKitNetworkProcess` version-matches the loaded `libwebkit2gtk-4.1`.
    /// Without this, bundled helpers cause IPC assertion failures on distros
    /// with a different `webkit2gtk` build.
    fn set_webkit_exec_path(&self) {
        if std::env::var_os("WEBKIT_EXEC_PATH").is_some() {
            return;
        }

        let candidates = [
            "/usr/lib/webkit2gtk-4.1",
            "/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1",
            "/usr/lib64/webkit2gtk-4.1",
            "/usr/libexec/webkit2gtk-4.1",
        ];

        for candidate in &candidates {
            if std::path::Path::new(candidate)
                .join("WebKitNetworkProcess")
                .exists()
            {
                std::env::set_var("WEBKIT_EXEC_PATH", candidate);
                tracing::info!("AppImage: WebKit helpers redirected to system path: {candidate}");
                return;
            }
        }
    }

    fn ensure_gstreamer_plugins(&self) {
        let system_dirs: Vec<&str> = [
            "/usr/lib/gstreamer-1.0",
            "/usr/lib/x86_64-linux-gnu/gstreamer-1.0",
            "/usr/lib64/gstreamer-1.0",
            "/usr/lib/aarch64-linux-gnu/gstreamer-1.0",
        ]
        .iter()
        .copied()
        .filter(|d| std::path::Path::new(d).is_dir())
        .collect();

        if system_dirs.is_empty() {
            return;
        }

        let extra = system_dirs.join(":");
        for var in ["GST_PLUGIN_SYSTEM_PATH", "GST_PLUGIN_SYSTEM_PATH_1_0"] {
            let merged = match std::env::var(var) {
                Ok(current) if !current.is_empty() => format!("{current}:{extra}"),
                _ => extra.clone(),
            };
            std::env::set_var(var, &merged);
        }
    }
}

/// Checks for required `GStreamer` plugins and logs a warning when any are absent.
///
/// Called after logging is initialised so the warning appears in the tracing
/// output.  Does not abort the process - the warning is informational and
/// allows the app to start even on incomplete system configurations.
pub(crate) fn check_dependencies() {
    if !autodetect_plugin_available() {
        tracing::warn!(
            "GStreamer plugin libgstautodetect not found. \
             Audio rendering may crash. Install gst-plugins-good: \
             Arch/Manjaro: sudo pacman -S gst-plugins-good | \
             Ubuntu/Debian: sudo apt install gstreamer1.0-plugins-good"
        );
    }
}

fn autodetect_plugin_available() -> bool {
    let standard_dirs: &[&str] = &[
        "/usr/lib/gstreamer-1.0",
        "/usr/lib/x86_64-linux-gnu/gstreamer-1.0",
        "/usr/lib64/gstreamer-1.0",
        "/usr/lib/aarch64-linux-gnu/gstreamer-1.0",
    ];

    let env_dirs: Vec<String> = ["GST_PLUGIN_SYSTEM_PATH_1_0", "GST_PLUGIN_SYSTEM_PATH"]
        .iter()
        .find_map(|var| std::env::var(var).ok().filter(|v| !v.is_empty()))
        .map(|v| v.split(':').map(str::to_owned).collect())
        .unwrap_or_default();

    env_dirs
        .iter()
        .map(String::as_str)
        .chain(standard_dirs.iter().copied())
        .any(|dir| std::path::Path::new(dir).join("libgstautodetect.so").exists())
}

/// Must be the very first call in `main()` on Linux.
///
/// Sets GTK/WebKit env vars early and, in `AppImage`, re-execs with a
/// host-first `LD_LIBRARY_PATH` (`_FANCY_REEXEC=1` avoids re-exec loops).
///
/// See <https://github.com/winfunc/opcode/issues/26>
pub fn pre_init() {
    // Temporary Linux workaround: disable WebKit compositing and default to
    // X11 before GTK init, unless the caller already provided overrides.
    if std::env::var_os("WEBKIT_DISABLE_COMPOSITING_MODE").is_none() {
        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
    }
    if std::env::var_os("GDK_BACKEND").is_none() {
        std::env::set_var("GDK_BACKEND", "x11");
    }

    // Re-exec with host-first LD_LIBRARY_PATH before any AppImage libs load.
    let Some(env) = AppImageEnv::detect() else {
        return;
    };
    if std::env::var_os("_FANCY_REEXEC").is_some() {
        return;
    }
    if let Some(new_ld) = env.host_first_library_path() {
        std::env::set_var("_FANCY_REEXEC", "1");
        std::env::set_var("LD_LIBRARY_PATH", new_ld);
        reexec_self();
    }
}

/// Early platform init (before GTK/Tauri starts): sets GTK identifiers
/// and, inside an `AppImage`, applies `WebKit`/`GStreamer` env overrides.
pub fn init_platform() {
    desktop::set_gtk_identifiers();
    if let Some(env) = AppImageEnv::detect() {
        env.apply_workarounds();
        env.ensure_gstreamer_plugins();
    }
}

fn reexec_self() {
    use std::os::unix::process::CommandExt;

    let Ok(exe) = std::fs::read_link("/proc/self/exe") else {
        return;
    };

    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let err = std::process::Command::new(&exe).args(args).exec();
    eprintln!("AppImage re-exec failed: {err}");
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    // Serialize tests that mutate process-wide env vars so parallel test
    // threads cannot interfere with each other.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn env(appdir: &str) -> AppImageEnv {
        AppImageEnv {
            appdir: appdir.to_owned(),
        }
    }

    // -- AppImageEnv::detect -------------------------------------------------

    #[test]
    fn appimage_detection_requires_appimage_or_appdir_var() {
        let _g = lock();
        std::env::remove_var("APPIMAGE");
        std::env::remove_var("APPDIR");
        assert!(AppImageEnv::detect().is_none());
    }

    #[test]
    fn appimage_detection_true_when_appimage_set() {
        let _g = lock();
        std::env::remove_var("APPIMAGE");
        std::env::remove_var("APPDIR");
        std::env::set_var("APPIMAGE", "/opt/apps/FancyMumble.AppImage");
        assert!(AppImageEnv::detect().is_some());
        std::env::remove_var("APPIMAGE");
    }

    #[test]
    fn appimage_detection_true_when_appdir_set() {
        let _g = lock();
        std::env::remove_var("APPIMAGE");
        std::env::remove_var("APPDIR");
        std::env::set_var("APPDIR", "/tmp/FancyMumble.AppDir");
        assert!(AppImageEnv::detect().is_some());
        std::env::remove_var("APPDIR");
    }

    // -- host_first_library_path ---------------------------------------------

    #[test]
    fn ld_path_reorder_none_when_ld_library_path_absent() {
        let _g = lock();
        std::env::remove_var("LD_LIBRARY_PATH");
        assert!(env("/tmp/app.AppDir").host_first_library_path().is_none());
    }

    #[test]
    fn ld_path_reorder_none_when_appdir_absent() {
        let _g = lock();
        std::env::set_var("LD_LIBRARY_PATH", "/tmp/app.AppDir/usr/lib:/usr/lib");
        // An empty appdir still produces Some - entries just aren't partitioned.
        let Some(result) = env("").host_first_library_path() else {
            panic!("Some expected when LD_LIBRARY_PATH is set");
        };
        assert!(result.contains("/usr/lib"), "entries must be preserved when appdir is empty");
        std::env::remove_var("LD_LIBRARY_PATH");
    }

    #[test]
    fn ld_path_reorder_host_dirs_precede_appdir_dirs() {
        let _g = lock();
        std::env::set_var(
            "LD_LIBRARY_PATH",
            "/tmp/app.AppDir/usr/lib:/tmp/app.AppDir/lib:/usr/lib",
        );

        let Some(result) = env("/tmp/app.AppDir").host_first_library_path() else {
            panic!("host_first_library_path should return Some");
        };
        let parts: Vec<&str> = result.split(':').collect();

        let Some(host_pos) = parts.iter().position(|p| *p == "/usr/lib") else {
            panic!("/usr/lib missing from reordered path: {result}");
        };
        let Some(appdir_pos) = parts.iter().position(|p| p.starts_with("/tmp/app.AppDir")) else {
            panic!("AppDir entry missing from reordered path: {result}");
        };
        assert!(host_pos < appdir_pos, "host dir must precede appdir: {result}");

        std::env::remove_var("LD_LIBRARY_PATH");
    }

    #[test]
    fn ld_path_reorder_preserves_all_original_entries() {
        let _g = lock();
        std::env::set_var(
            "LD_LIBRARY_PATH",
            "/mnt/appdir/usr/lib:/opt/custom/lib:/usr/lib",
        );

        let Some(result) = env("/mnt/appdir").host_first_library_path() else {
            panic!("host_first_library_path should return Some");
        };
        assert!(result.contains("/mnt/appdir/usr/lib"), "AppDir entry must be preserved");
        assert!(result.contains("/opt/custom/lib"), "extra entry must be preserved");
        assert!(result.contains("/usr/lib"), "host entry must be preserved");

        std::env::remove_var("LD_LIBRARY_PATH");
    }

    #[test]
    fn ld_path_reorder_skips_empty_colon_segments() {
        let _g = lock();
        std::env::set_var("LD_LIBRARY_PATH", "/tmp/a/lib::/usr/lib::");

        let Some(result) = env("/tmp/a").host_first_library_path() else {
            panic!("host_first_library_path should return Some");
        };
        assert!(!result.contains("::"), "empty segments must not appear in output: {result}");

        std::env::remove_var("LD_LIBRARY_PATH");
    }

    // -- set_wayland_backend -------------------------------------------------

    #[test]
    fn wayland_backend_set_when_wayland_display_present() {
        let _g = lock();
        std::env::set_var("WAYLAND_DISPLAY", "wayland-1");
        std::env::remove_var("GDK_BACKEND");

        env("/tmp/a").set_wayland_backend();

        assert_eq!(
            std::env::var("GDK_BACKEND").as_deref(),
            Ok("wayland"),
            "GDK_BACKEND must be set to wayland when WAYLAND_DISPLAY is present"
        );

        std::env::remove_var("WAYLAND_DISPLAY");
        std::env::remove_var("GDK_BACKEND");
    }

    #[test]
    fn wayland_backend_not_set_without_wayland_display() {
        let _g = lock();
        std::env::remove_var("WAYLAND_DISPLAY");
        std::env::set_var("GDK_BACKEND", "x11");

        env("/tmp/a").set_wayland_backend();

        assert_eq!(
            std::env::var("GDK_BACKEND").as_deref(),
            Ok("x11"),
            "GDK_BACKEND must not be changed when WAYLAND_DISPLAY is absent"
        );

        std::env::remove_var("GDK_BACKEND");
    }

    #[test]
    fn apply_appimage_workarounds_sets_wayland_backend_on_wayland_session() {
        let _g = lock();
        std::env::set_var("WAYLAND_DISPLAY", "wayland-1");
        std::env::remove_var("GDK_BACKEND");
        std::env::remove_var("LD_LIBRARY_PATH");

        env("/tmp/a").apply_workarounds();

        assert_eq!(
            std::env::var("GDK_BACKEND").as_deref(),
            Ok("wayland"),
            "apply_workarounds must call set_wayland_backend"
        );

        std::env::remove_var("WAYLAND_DISPLAY");
        std::env::remove_var("GDK_BACKEND");
        std::env::remove_var("WEBKIT_DISABLE_DMABUF_RENDERER");
        std::env::remove_var("WEBKIT_EXEC_PATH");
    }

    // -- pre_init sentinel ---------------------------------------------------

    #[test]
    fn pre_init_does_not_reexec_when_sentinel_is_set() {
        let _g = lock();
        std::env::set_var("APPIMAGE", "/fake/app.AppImage");
        std::env::set_var("APPDIR", "/fake/app.AppDir");
        std::env::set_var("_FANCY_REEXEC", "1");

        // Must return without calling reexec_self (which would either crash or
        // replace the process image).
        pre_init();

        std::env::remove_var("APPIMAGE");
        std::env::remove_var("APPDIR");
        std::env::remove_var("_FANCY_REEXEC");
    }

    // -- check_dependencies --------------------------------------------------

    #[test]
    fn check_dependencies_does_not_panic() {
        // Smoke-test: function must complete without panicking regardless of
        // which GStreamer plugins are installed on the host machine.
        check_dependencies();
    }

    #[test]
    fn autodetect_plugin_found_when_in_env_path() {
        let _g = lock();
        let dir = std::env::temp_dir().join("fancy_gst_test");
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("create temp dir: {e}"));
        std::fs::write(dir.join("libgstautodetect.so"), b"")
            .unwrap_or_else(|e| panic!("write fake plugin: {e}"));
        std::env::set_var(
            "GST_PLUGIN_SYSTEM_PATH_1_0",
            dir.to_str().unwrap_or_else(|| panic!("non-UTF-8 path")),
        );
        let found = autodetect_plugin_available();
        std::env::remove_var("GST_PLUGIN_SYSTEM_PATH_1_0");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(found, "plugin under GST_PLUGIN_SYSTEM_PATH_1_0 must be detected");
    }

    // -- set_webkit_exec_path ------------------------------------------------

    #[test]
    fn webkit_exec_path_not_overwritten_when_already_set() {
        let _g = lock();
        std::env::set_var("WEBKIT_EXEC_PATH", "/my/custom/webkit");

        env("/tmp/a").set_webkit_exec_path();

        assert_eq!(
            std::env::var("WEBKIT_EXEC_PATH").as_deref(),
            Ok("/my/custom/webkit"),
            "a user-supplied WEBKIT_EXEC_PATH must not be replaced"
        );

        std::env::remove_var("WEBKIT_EXEC_PATH");
    }
}
