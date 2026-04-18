/*
 * Wayland surface force-commit helper for GNOME/Mutter multi-monitor.
 *
 * On GNOME Wayland, Mutter stops delivering frame callbacks to windows
 * on monitors without cursor activity.  GTK3's Wayland backend only
 * commits surface updates in response to frame callbacks, so canvas
 * draws from WebKitGTK become invisible when the cursor is on another
 * monitor.
 *
 * This helper bypasses GTK's frame-callback gating by calling
 * wl_surface_damage_buffer + wl_surface_commit directly on the
 * underlying Wayland surface.  The compositor then notices the damage,
 * re-composites the window, and delivers a frame callback that
 * restarts GTK's normal paint cycle.
 *
 * All Wayland and GDK symbols are resolved at runtime via dlsym so
 * that the binary works on both Wayland and X11 without hard linking.
 */

#include <dlfcn.h>
#include <stddef.h>
#include <stdint.h>

/* Wayland surface request opcodes (from wayland.xml) */
#define WL_SURFACE_DAMAGE         2
#define WL_SURFACE_COMMIT         6

typedef void *(*GdkGetWlSurfaceFn)(void *);
typedef void  (*WlProxyMarshalFn)(void *, uint32_t, ...);

static GdkGetWlSurfaceFn s_get_wl_surface;
static WlProxyMarshalFn  s_proxy_marshal;
static int               s_init_done;

static void ensure_init(void) {
    if (s_init_done) return;
    s_init_done = 1;

    void *gdk = dlopen("libgdk-3.so.0", RTLD_LAZY | RTLD_NOLOAD);
    if (gdk) {
        s_get_wl_surface = (GdkGetWlSurfaceFn)dlsym(
            gdk, "gdk_wayland_window_get_wl_surface");
    }

    void *wl = dlopen("libwayland-client.so.0", RTLD_LAZY | RTLD_NOLOAD);
    if (wl) {
        s_proxy_marshal = (WlProxyMarshalFn)dlsym(
            wl, "wl_proxy_marshal");
    }
}

/* Returns the wl_surface for a GdkWindow, or NULL on X11 / error. */
void *fancy_get_wl_surface(void *gdk_window) {
    ensure_init();
    if (!s_get_wl_surface || !gdk_window) return NULL;
    return s_get_wl_surface(gdk_window);
}

/* Damages a small region on the surface and commits.
 * Uses wl_surface.damage (opcode 2, version 1) rather than
 * wl_surface.damage_buffer (opcode 9, version 4) because GDK3
 * binds wl_compositor at version 3 -- calling a v4 request on a
 * v3 surface is a Wayland protocol error.
 * Safe to call with NULL (no-op). */
void fancy_wl_surface_force_commit(void *wl_surface) {
    ensure_init();
    if (!s_proxy_marshal || !wl_surface) return;
    s_proxy_marshal(wl_surface, WL_SURFACE_DAMAGE,
                    (int32_t)0, (int32_t)0, (int32_t)1, (int32_t)1);
    s_proxy_marshal(wl_surface, WL_SURFACE_COMMIT);
}

/* Returns non-zero if Wayland surface operations are available. */
int fancy_wayland_available(void) {
    ensure_init();
    return s_get_wl_surface != NULL && s_proxy_marshal != NULL;
}
