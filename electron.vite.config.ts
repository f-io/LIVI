import { resolve } from "path";
import { defineConfig, externalizeDepsPlugin } from "electron-vite";
import react from "@vitejs/plugin-react";
import { NodeGlobalsPolyfillPlugin } from "@esbuild-plugins/node-globals-polyfill";
import path from "path";

const alias = {
  "@renderer": resolve(__dirname, "src/renderer/src"),
  "@carplay/web": resolve(__dirname, "src/renderer/components/web/CarplayWeb.ts"),
  "@carplay/messages": resolve(__dirname, "src/main/carplay/messages"),
  "@carplay": resolve(__dirname, "src/main/carplay"),
  "@main": path.resolve(__dirname, "src/main"),
  "@worker": path.resolve(__dirname, "src/renderer/src/components/worker"),
  "@store": path.resolve(__dirname, "src/renderer/src/store"),
  "@utils": path.resolve(__dirname, "src/renderer/src/utils"),
  stream: "stream-browserify",
  Buffer: "buffer",
};

export default defineConfig({
  main: {
    plugins: [externalizeDepsPlugin({})],
    build: {
      outDir: "out/main",
      rollupOptions: {
        input: {
          main: resolve(__dirname, "src/main/index.ts"),
          usbWorker: resolve(__dirname, "src/main/usb/USBWorker.ts"),
        },
        output: { entryFileNames: "[name].js" },
      },
    },
    resolve: { alias },
  },

  preload: {
    plugins: [externalizeDepsPlugin({})],
    build: { outDir: "out/preload" },
    resolve: { alias },
  },

  renderer: {
    base: "app://",
    publicDir: resolve(__dirname, "src/renderer/public"),
    build: {
      outDir: "out/renderer",
      rollupOptions: {
        input: { index: resolve(__dirname, "src/renderer/index.html") },
        output: {
          entryFileNames: "index.js",
          assetFileNames: (chunkInfo) => {
            if (chunkInfo.name?.endsWith(".css")) return "index.css";
            return "assets/[name].[ext]";
          },
        },
      },
    },
    resolve: { alias },
    optimizeDeps: {
      exclude: ["audio.worklet.js"],
      esbuildOptions: {
        define: { global: "globalThis" },
        plugins: [NodeGlobalsPolyfillPlugin({ process: true, buffer: true })],
      },
    },
    plugins: [react({})],
    server: {
      headers: {
        "Cross-Origin-Embedder-Policy": "require-corp",
        "Cross-Origin-Opener-Policy": "same-origin",
        "Cross-Origin-Resource-Policy": "same-site",
      },
    },
    worker: { format: "es" },
  },
});
