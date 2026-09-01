/// <reference lib="webworker" />

import jsQR from "jsqr";
import { prepareZXingModule, readBarcodes, type ReadResult } from "zxing-wasm/reader";
import wasmUrl from "zxing-wasm/reader/zxing_reader.wasm?url";

interface DecodeRequest {
  type: "decode";
  id: number;
  buffer: ArrayBuffer;
  width: number;
  height: number;
  originX: number;
  originY: number;
  full: boolean;
  maxSymbols: number;
}

interface DecodedSymbol {
  bytes: Uint8Array;
  text: string;
  box: { x: number; y: number; width: number; height: number };
  quad?: {
    topLeft: { x: number; y: number };
    topRight: { x: number; y: number };
    bottomRight: { x: number; y: number };
    bottomLeft: { x: number; y: number };
  };
}

const workerScope = self as unknown as DedicatedWorkerGlobalScope;
let wasmReady = false;

function fromZXing(result: ReadResult, originX: number, originY: number): DecodedSymbol {
  const shifted = (point: { x: number; y: number }) => ({ x: point.x + originX, y: point.y + originY });
  const quad = {
    topLeft: shifted(result.position.topLeft),
    topRight: shifted(result.position.topRight),
    bottomRight: shifted(result.position.bottomRight),
    bottomLeft: shifted(result.position.bottomLeft),
  };
  const xs = [quad.topLeft.x, quad.topRight.x, quad.bottomRight.x, quad.bottomLeft.x];
  const ys = [quad.topLeft.y, quad.topRight.y, quad.bottomRight.y, quad.bottomLeft.y];
  const left = Math.min(...xs);
  const top = Math.min(...ys);
  return {
    bytes: Uint8Array.from(result.bytes),
    text: result.text,
    box: { x: left, y: top, width: Math.max(...xs) - left, height: Math.max(...ys) - top },
    quad,
  };
}

function eraseSymbol(pixels: Uint8ClampedArray, width: number, height: number, box: DecodedSymbol["box"]): void {
  const padding = Math.max(4, Math.round(Math.max(box.width, box.height) * 0.08));
  const left = Math.max(0, Math.floor(box.x - padding));
  const top = Math.max(0, Math.floor(box.y - padding));
  const right = Math.min(width, Math.ceil(box.x + box.width + padding));
  const bottom = Math.min(height, Math.ceil(box.y + box.height + padding));
  for (let y = top; y < bottom; y += 1) {
    let offset = (y * width + left) * 4;
    for (let x = left; x < right; x += 1) {
      pixels[offset] = 255;
      pixels[offset + 1] = 255;
      pixels[offset + 2] = 255;
      pixels[offset + 3] = 255;
      offset += 4;
    }
  }
}

async function decodeWithWasm(request: DecodeRequest, pixels: Uint8ClampedArray): Promise<DecodedSymbol[]> {
  const results = await readBarcodes(
    { data: pixels, width: request.width, height: request.height } as ImageData,
    {
      formats: ["QRCode"],
      tryHarder: true,
      tryRotate: false,
      tryInvert: false,
      tryDownscale: request.full,
      tryDenoise: false,
      maxNumberOfSymbols: request.full ? request.maxSymbols : 1,
      returnErrors: false,
      textMode: "Plain",
    },
  );
  return results.filter((result) => result.isValid && result.bytes.byteLength > 0).map((result) => fromZXing(result, request.originX, request.originY));
}

function decodeWithJsFallback(request: DecodeRequest, pixels: Uint8ClampedArray): DecodedSymbol[] {
  const symbols: DecodedSymbol[] = [];
  for (let index = 0; index < Math.max(1, Math.min(4, request.maxSymbols)); index += 1) {
    const code = jsQR(pixels, request.width, request.height, { inversionAttempts: "dontInvert" });
    if (!code) break;
    const points = [code.location.topLeftCorner, code.location.topRightCorner, code.location.bottomRightCorner, code.location.bottomLeftCorner];
    const left = Math.min(...points.map((point) => point.x));
    const top = Math.min(...points.map((point) => point.y));
    const right = Math.max(...points.map((point) => point.x));
    const bottom = Math.max(...points.map((point) => point.y));
    const symbol: DecodedSymbol = {
      bytes: Uint8Array.from(code.binaryData),
      text: code.data,
      box: { x: request.originX + left, y: request.originY + top, width: right - left, height: bottom - top },
    };
    symbols.push(symbol);
    eraseSymbol(pixels, request.width, request.height, { x: left, y: top, width: right - left, height: bottom - top });
    if (!request.full) break;
  }
  return symbols;
}

workerScope.onmessage = async (event: MessageEvent<DecodeRequest>) => {
  const request = event.data;
  if (request.type !== "decode") return;
  const pixels = new Uint8ClampedArray(request.buffer);
  let engine: "wasm" | "js" = "wasm";
  let symbols: DecodedSymbol[] = [];
  try {
    if (!wasmReady) throw new Error("WASM decoder is not ready");
    symbols = await decodeWithWasm(request, pixels);
  } catch {
    engine = "js";
    symbols = decodeWithJsFallback(request, pixels);
  }
  const transfers = symbols.map((symbol) => symbol.bytes.buffer as ArrayBuffer);
  workerScope.postMessage({ type: "decoded", id: request.id, symbols, engine, full: request.full }, transfers);
};

void (async () => {
  try {
    await prepareZXingModule({
      overrides: { locateFile: (path: string, prefix: string) => path.endsWith(".wasm") ? wasmUrl : prefix + path },
      fireImmediately: true,
    });
    wasmReady = true;
    workerScope.postMessage({ type: "ready", engine: "wasm" });
  } catch (error) {
    workerScope.postMessage({ type: "ready", engine: "js", error: String(error) });
  }
})();
