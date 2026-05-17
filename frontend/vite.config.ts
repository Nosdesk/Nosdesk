import { fileURLToPath, URL } from "node:url";

import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// True only when invoked as `vite build --watch` (our
// `dev:unified` script + the Docker frontend-watch service).
// `vite build` without --watch leaves `build.watch` null so it
// completes and exits like a one-shot build is expected to.
const isWatchBuild = process.argv.includes("--watch");

// https://vite.dev/config/
export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
      // Use full Vue build with runtime compiler for plugin template strings
      "vue": "vue/dist/vue.esm-bundler.js",
    },
  },
  define: {
    __VUE_PROD_DEVTOOLS__: false,
    __VUE_PROD_HYDRATION_MISMATCH_DETAILS__: false,
    __VUE_OPTIONS_API__: true,
    __VUE_PROD_TIPS__: false,
    __VUE_DEVTOOLS_GLOBAL_HOOK__: "window.__VUE_DEVTOOLS_GLOBAL_HOOK__",
  },
  // Optimize dependency pre-bundling
  optimizeDeps: {
    include: [
      'vue',
      'vue-router',
      'pinia',
      'axios',
      'date-fns',
      'yjs',
      'prosemirror-state',
      'prosemirror-view',
      'prosemirror-model',
    ],
  },
  // Build configuration - output to backend's public directory
  build: {
    outDir: "dist",
    emptyOutDir: true,
    // Ensure assets are referenced correctly when served by backend
    assetsDir: "assets",
    sourcemap: false,
    // Skip minification in watch mode for faster rebuilds. `true`
    // lets Vite pick its current default minifier (esbuild on v7,
    // oxc on v8) so we don't have to track which one is the default.
    minify: process.env.NODE_ENV === 'production' ? true : false,
    // Rollup `watch.include` extends what the watcher tracks
    // when `vite build --watch` is running (our `dev:unified`
    // script + the frontend-watch container). Without listing
    // `../i18n/locales/**`, a *brand new* FTL file there
    // doesn't trigger a rebuild: Vite's `import.meta.glob`
    // only re-evaluates when a file already in the dep graph
    // changes, so adding a locale requires a watcher restart.
    // Gated on the `--watch` CLI flag because setting
    // `build.watch` to any non-null object forces Vite into
    // watch mode regardless of how it was invoked — including
    // a plain `vite build`, which would then hang waiting for
    // changes instead of completing.
    watch: isWatchBuild
      ? { include: ['src/**', '../i18n/locales/**'] }
      : null,
    rolldownOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes('node_modules')) return;
          // Vue core framework
          if (/[\\/](vue|vue-router|pinia)[\\/]/.test(id)) return 'vendor-vue';
          // ProseMirror + Yjs editor stack
          if (/[\\/](prosemirror-|y-prosemirror|yjs|y-protocols|y-websocket|y-indexeddb|lib0)[\\/]/.test(id)) return 'vendor-editor';
          // Utility libraries
          if (/[\\/](axios|date-fns|@date-fns|dompurify|marked)[\\/]/.test(id)) return 'vendor-utils';
          // Heavy optional/media libraries
          if (/[\\/](heic2any|jszip|qrcode|highlight\.js|lowlight)[\\/]/.test(id)) return 'vendor-media';
        },
      },
    },
  },
  server: {
    host: "0.0.0.0",
    port: 5173,
    // Widen Vite's dev-server file-read sandbox to the workspace
    // root so `import.meta.glob('../../../i18n/locales/...')` can
    // reach the shared Fluent catalogues outside `frontend/`.
    // Production builds bundle the files at compile time and
    // don't go through this gate.
    fs: {
      allow: ["..", "./"],
    },
    // Docker-specific optimizations for file watching and HMR
    watch: {
      usePolling: true,  // Required for Docker on macOS/Windows
      interval: 300,     // Faster polling for quicker HMR
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
