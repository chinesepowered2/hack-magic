import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { nodePolyfills } from "vite-plugin-node-polyfills";

export default defineConfig({
  root: __dirname,
  plugins: [react(), nodePolyfills({ include: ["buffer", "crypto", "stream", "util"], globals: { Buffer: true } })],
  server: { fs: { allow: [".."] } },
  build: { outDir: "dist", emptyOutDir: true },
});
