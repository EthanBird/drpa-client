import { cp, mkdir, rm } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { build } from "esbuild";

const projectRoot = dirname(fileURLToPath(import.meta.url));
const outputRoot = join(projectRoot, "miniprogram");
const zxingRoot = join(projectRoot, "..", "..", "node_modules", "zxing-wasm");

await rm(outputRoot, { recursive: true, force: true });
await mkdir(outputRoot, { recursive: true });

await build({
  absWorkingDir: projectRoot,
  entryPoints: {
    app: "src/app.ts",
    "pages/transfer/index": "src/pages/transfer/index.ts",
    "pages/privacy/index": "src/pages/privacy/index.ts",
    "workers/optical-decoder": "src/workers/optical-decoder.ts",
  },
  outdir: outputRoot,
  bundle: true,
  alias: {
    "zxing-wasm": join(zxingRoot, "dist", "miniprogram", "index.js"),
  },
  format: "cjs",
  platform: "browser",
  mainFields: ["miniprogram", "browser", "module", "main"],
  target: "es2015",
  minify: true,
  treeShaking: true,
  legalComments: "none",
  charset: "utf8",
});

await cp(join(projectRoot, "src/static"), outputRoot, { recursive: true, force: true });
await mkdir(join(outputRoot, "wasm"), { recursive: true });
await cp(join(zxingRoot, "dist", "full", "zxing_full.wasm"), join(outputRoot, "wasm", "zxing_full.wasm"));

console.log(`DRPA mini program built at ${outputRoot}`);
