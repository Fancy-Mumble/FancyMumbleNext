import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import svgr from "vite-plugin-svgr";

const port = Number(process.env.VITE_PORT) || 1420;

export default defineConfig({
  plugins: [react(), svgr()],

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
    // Tauri uses Chromium on Windows/Linux and WebKit on macOS.
    target: "esnext",
    minify: !process.env.TAURI_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_DEBUG,
  },
});
