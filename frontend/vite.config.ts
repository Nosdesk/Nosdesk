import { fileURLToPath, URL } from "node:url";

import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// https://vite.dev/config/
export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  define: {
    __VUE_PROD_DEVTOOLS__: true, // Enable Vue DevTools in production build
    __VUE_PROD_HYDRATION_MISMATCH_DETAILS__: false,
    __VUE_OPTIONS_API__: true,
    __VUE_PROD_TIPS__: false,
    __VUE_DEVTOOLS_GLOBAL_HOOK__: "window.__VUE_DEVTOOLS_GLOBAL_HOOK__",
  },
  // Build configuration - output to backend's public directory
  build: {
    outDir: "dist",
    emptyOutDir: true,
    // Ensure assets are referenced correctly when served by backend
    assetsDir: "assets",
  },
  server: {
    host: "0.0.0.0",
    port: 5173,
    // Docker-specific optimizations for file watching and HMR
    watch: {
      usePolling: true,  // Required for Docker on macOS/Windows
      interval: 1000,    // Reduce CPU usage with 1s polling interval
    },
    hmr: {
      clientPort: 5173,  // Match exposed Docker port for HMR websocket
    },
    proxy: {
      "/api": {
        target: process.env.VITE_API_URL || "http://127.0.0.1:8080",
        changeOrigin: true,
        secure: false,
        configure: (proxy, _options) => {
          proxy.on("error", (err, _req, _res) => {
            console.log("Proxy Error:", err);
          });
          proxy.on("proxyReq", (proxyReq, req, _res) => {
            console.log(
              "Proxy Request:",
              req.method,
              req.url,
              "→",
              proxyReq.path,
            );
          });
          proxy.on("proxyRes", (proxyRes, req, _res) => {
            console.log(
              "Proxy Response:",
              proxyRes.statusCode,
              req.method,
              req.url,
            );
          });
        },
      },
      "/uploads": {
        target: process.env.VITE_API_URL || "http://127.0.0.1:8080",
        changeOrigin: true,
      },
    },
  },
});
