import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

import QRCode from "qrcode";

const projectRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const outputRoot = join(projectRoot, "miniprogram");
const workerSource = await readFile(join(outputRoot, "workers", "optical-decoder.js"), "utf8");

let messageHandler;
const messages = [];
const waiters = new Set();

function publish(message) {
  messages.push(message);
  for (const waiter of waiters) waiter(message);
}

function waitFor(type, timeoutMs = 15_000) {
  const existing = messages.find((message) => message.type === type);
  if (existing) return Promise.resolve(existing);
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      waiters.delete(onMessage);
      reject(new Error(`Timed out waiting for worker message: ${type}`));
    }, timeoutMs);
    const onMessage = (message) => {
      if (message.type !== type) return;
      clearTimeout(timeout);
      waiters.delete(onMessage);
      resolve(message);
    };
    waiters.add(onMessage);
  });
}

const worker = {
  onMessage(handler) {
    messageHandler = handler;
  },
  postMessage(message) {
    publish(message);
  },
  getCameraFrameData() {
    return new ArrayBuffer(0);
  },
};

const WXWebAssembly = {
  CompileError: WebAssembly.CompileError,
  Global: WebAssembly.Global,
  Instance: WebAssembly.Instance,
  LinkError: WebAssembly.LinkError,
  Memory: WebAssembly.Memory,
  Module: WebAssembly.Module,
  RuntimeError: WebAssembly.RuntimeError,
  Table: WebAssembly.Table,
  async instantiate(path, imports) {
    const binary = await readFile(join(outputRoot, path));
    return WebAssembly.instantiate(binary, imports);
  },
};

const context = vm.createContext({
  Array,
  ArrayBuffer,
  Blob,
  DataView,
  Date,
  Error,
  FinalizationRegistry,
  Float32Array,
  Float64Array,
  Int8Array,
  Int16Array,
  Int32Array,
  Map,
  Math,
  Object,
  Promise,
  RangeError,
  Reflect,
  Set,
  String,
  Symbol,
  TextDecoder,
  TextEncoder,
  TypeError,
  Uint8Array,
  Uint8ClampedArray,
  Uint16Array,
  Uint32Array,
  WeakMap,
  WXWebAssembly,
  clearTimeout,
  console,
  navigator: { language: "zh-CN" },
  setTimeout,
  worker,
});
context.globalThis = context;

vm.runInContext(workerSource, context, { filename: "optical-decoder.js" });
await waitFor("ready");

function renderQr(input) {
  const qr = QRCode.create(input, { errorCorrectionLevel: "L", maskPattern: 2 });
  const quiet = 4;
  const scale = 8;
  const width = (qr.modules.size + quiet * 2) * scale;
  const rgba = new Uint8ClampedArray(width * width * 4);
  rgba.fill(255);
  for (let row = 0; row < qr.modules.size; row += 1) {
    for (let column = 0; column < qr.modules.size; column += 1) {
      if (!qr.modules.data[row * qr.modules.size + column]) continue;
      for (let y = 0; y < scale; y += 1) {
        for (let x = 0; x < scale; x += 1) {
          const pixel = (((row + quiet) * scale + y) * width + (column + quiet) * scale + x) * 4;
          rgba[pixel] = 0;
          rgba[pixel + 1] = 0;
          rgba[pixel + 2] = 0;
        }
      }
    }
  }
  return { rgba, width };
}

async function decodeSynthetic(input, frameId) {
  messages.length = 0;
  const { rgba, width } = renderQr(input);
  messageHandler({
    message: {
      type: "decode",
      frameId,
      width,
      height: width,
      direct: false,
      data: rgba.buffer,
    },
  });
  return waitFor("decoded");
}

const payload = "DRPA2:WASM-SMOKE-TEST";
const decoded = await decodeSynthetic(payload, 1);
if (decoded.text !== payload) throw new Error(`Unexpected WASM decode result: ${decoded.text}`);

const binaryPayload = Uint8Array.from([0x44, 0x52, 0x50, 0x41, 0x02, 0x00, 0xff, 0x7f, 0x10, 0x80, 0x33, 0x5a]);
const decodedBinary = await decodeSynthetic([{ data: Buffer.from(binaryPayload), mode: "byte" }], 2);
const receivedBinary = new Uint8Array(decodedBinary.bytes);
if (receivedBinary.length !== binaryPayload.length || receivedBinary.some((value, index) => value !== binaryPayload[index])) {
  throw new Error("WASM binary QR payload did not round-trip");
}

console.log(`ZXing-C++ WASM worker decoded text and binary QR payloads (${decoded.decodeMs}/${decodedBinary.decodeMs} ms)`);
