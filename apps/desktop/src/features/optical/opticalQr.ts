import QRCode from "qrcode";

const WHITE = 0xffffffff;
const BLACK = 0xff000000;

function gridDimensions(count: number): { columns: number; rows: number } {
  if (count <= 1) return { columns: 1, rows: 1 };
  if (count === 2) return { columns: 1, rows: 2 };
  return { columns: 2, rows: 2 };
}

/**
 * Renders QR modules at one pixel per module, then lets CSS scale the canvas.
 * Pinning the mask skips the QR library's eight-mask scoring pass and avoids
 * rebuilding a 520×520 raster for every optical frame.
 */
export function renderOpticalQrGrid(canvas: HTMLCanvasElement, frames: readonly (string | Uint8Array)[], margin = 2): void {
  if (!frames.length) return;
  const codes = frames.map((frame) => QRCode.create(
    [typeof frame === "string" ? { mode: "alphanumeric" as const, data: frame } : { mode: "byte" as const, data: frame }],
    { errorCorrectionLevel: "L", maskPattern: 2 },
  ));
  const moduleCount = codes[0]!.modules.size;
  if (!codes.every((code) => code.modules.size === moduleCount)) throw new Error("二维码网格版本不一致");
  const { columns, rows } = gridDimensions(codes.length);
  const cell = moduleCount + margin * 2;
  const width = columns * cell;
  const height = rows * cell;
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext("2d", { alpha: false });
  if (!context) throw new Error("无法创建二维码画布");
  const image = context.createImageData(width, height);
  const pixels = new Uint32Array(image.data.buffer);
  pixels.fill(WHITE);
  codes.forEach((code, codeIndex) => {
    const originX = (codeIndex % columns) * cell + margin;
    const originY = Math.floor(codeIndex / columns) * cell + margin;
    const modules = code.modules.data;
    for (let y = 0; y < moduleCount; y += 1) {
      const row = (originY + y) * width + originX;
      const source = y * moduleCount;
      for (let x = 0; x < moduleCount; x += 1) {
        if (modules[source + x]) pixels[row + x] = BLACK;
      }
    }
  });
  context.putImageData(image, 0, 0);
}
