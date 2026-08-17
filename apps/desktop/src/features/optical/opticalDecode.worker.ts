/// <reference lib="webworker" />

import jsQR from "jsqr";

interface DecodeRequest {
  id: number;
  buffer: ArrayBuffer;
  width: number;
  height: number;
  maxSymbols: number;
}

interface DecodedSymbol {
  text: string;
  box: { x: number; y: number; width: number; height: number };
}

const workerScope = self as unknown as DedicatedWorkerGlobalScope;

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

workerScope.onmessage = (event: MessageEvent<DecodeRequest>) => {
  const { id, buffer, width, height, maxSymbols } = event.data;
  const pixels = new Uint8ClampedArray(buffer);
  const symbols: DecodedSymbol[] = [];
  try {
    for (let index = 0; index < Math.max(1, Math.min(4, maxSymbols)); index += 1) {
      const code = jsQR(pixels, width, height, { inversionAttempts: "dontInvert" });
      if (!code) break;
      const points = [code.location.topLeftCorner, code.location.topRightCorner, code.location.bottomRightCorner, code.location.bottomLeftCorner];
      const left = Math.min(...points.map((point) => point.x));
      const top = Math.min(...points.map((point) => point.y));
      const right = Math.max(...points.map((point) => point.x));
      const bottom = Math.max(...points.map((point) => point.y));
      const symbol = { text: code.data, box: { x: left, y: top, width: right - left, height: bottom - top } };
      symbols.push(symbol);
      eraseSymbol(pixels, width, height, symbol.box);
    }
    workerScope.postMessage({ id, symbols });
  } catch {
    workerScope.postMessage({ id, symbols: [] });
  }
};

