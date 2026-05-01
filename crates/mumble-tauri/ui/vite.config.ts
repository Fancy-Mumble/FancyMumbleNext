import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const port = Number(process.env.VITE_PORT) || 1420;

export default defineConfig({
  plugins: [react()],

  // Prevent Vite from clearing the terminal so Tauri logs stay visible.
  clearScreen: false,

  server: {
    port,
    strictPort: true,
    host: true,
    hmr: {
      // On Android devices, "localhost" resolves to the device itself.
      // Use the dev machine's LAN IP so HMR WebSocket can connect.
      host: process.env.TAURI_DEV_HOST || "localhost",
    },
  },

  // Expose TAURI_* env variables to client code.
  envPrefix: ["VITE_", "TAURI_"],

  build: {
    // Tauri uses Chromium on Windows/Linux/Android and WebKit on macOS/iOS.
    target: "esnext",
    // Tauri ships with Chromium-based WebViews on Windows/Linux/Android
    // and WKWebView on macOS/iOS.  All current WebViews understand the
    // unprefixed `backdrop-filter`, but older WKWebView still requires
    // the `-webkit-` prefix, so source CSS keeps both forms.  Important:
    // when both declarations have identical values, esbuild dedupes them
    // and keeps only the LAST one in source order.  All paired
    // declarations in this codebase are written as `-webkit-...` first,
    // unprefixed second so esbuild always preserves the standard
    // property (Android Chromium WebView rejects `-webkit-backdrop-filter`
    // as invalid and silently drops it).
    cssTarget: ["chrome108", "safari15"],
    minify: !process.env.TAURI_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_DEBUG,
  },
});
