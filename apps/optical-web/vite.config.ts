import react from "@vitejs/plugin-react";
import { defineConfig, type Plugin } from "vite";

const precacheManifest = (): Plugin => ({
  name: "drpa-optical-precache-manifest",
  generateBundle(_options, bundle) {
    const files = Object.values(bundle)
      .map((entry) => entry.fileName)
      .filter((fileName) => /\.(?:css|js|wasm)$/.test(fileName));
    this.emitFile({
      type: "asset",
      fileName: "precache-manifest.json",
      source: JSON.stringify({ files }, null, 2),
    });
  },
});

export default defineConfig({
  // Relative assets work both at /drpa-client/ and on a future custom domain.
  base: "./",
  plugins: [react(), precacheManifest()],
  build: {
    target: "es2022",
    sourcemap: true,
  },
});
