import { prepareZXingModule, readBarcodes, type ReaderOptions } from "zxing-wasm";

declare const worker: WechatMiniprogram.Worker;

interface DecodeMessage {
  type: "decode";
  frameId: number;
  width: number;
  height: number;
  direct: boolean;
  data?: ArrayBuffer;
}

const FAST_OPTIONS: ReaderOptions = {
  formats: ["QRCode"],
  tryHarder: false,
  tryRotate: true,
  tryInvert: false,
  tryDownscale: true,
  tryDenoise: false,
  maxNumberOfSymbols: 1,
  textMode: "Plain",
  binarizer: "LocalAverage",
};

let wasmReady = false;
let decoding = false;
let pendingFrame: DecodeMessage | null = null;
let missStreak = 0;
let directFrameFailures = 0;
let statsStartedAt = Date.now();
let statsFrames = 0;

function send(message: WechatMiniprogram.IAnyObject): void {
  worker.postMessage(message);
}

function unwrapMessage(event: WechatMiniprogram.WorkerOnMessageListenerResult | DecodeMessage): DecodeMessage | null {
  const candidate = "message" in event ? event.message : event;
  if (!candidate || typeof candidate !== "object" || (candidate as DecodeMessage).type !== "decode") return null;
  return candidate as DecodeMessage;
}

function performanceSample(decodeMs: number): { decodeMs: number; fps: number } {
  statsFrames += 1;
  const now = Date.now();
  const elapsed = now - statsStartedAt;
  if (elapsed < 800) return { decodeMs, fps: 0 };
  const fps = Math.max(1, Math.round((statsFrames * 1000) / elapsed));
  statsStartedAt = now;
  statsFrames = 0;
  return { decodeMs, fps };
}

function currentFrame(message: DecodeMessage): ArrayBuffer | null {
  if (!message.direct) return message.data instanceof ArrayBuffer ? message.data : null;
  try {
    const data = worker.getCameraFrameData();
    if (data instanceof ArrayBuffer && data.byteLength > 0) {
      directFrameFailures = 0;
      return data;
    }
  } catch {
    // ExperimentalWorker frame access is not available on every client.
  }
  directFrameFailures += 1;
  return null;
}

async function decodeFrame(message: DecodeMessage): Promise<void> {
  const startedAt = Date.now();
  const frame = currentFrame(message);
  const expectedBytes = message.width * message.height * 4;
  if (!frame || frame.byteLength !== expectedBytes) {
    send({
      type: directFrameFailures >= 2 && message.direct ? "frame-unavailable" : "miss",
      frameId: message.frameId,
      ...performanceSample(Date.now() - startedAt),
    });
    return;
  }

  const robustPass = missStreak > 0 && missStreak % 6 === 0;
  try {
    const results = await readBarcodes(
      {
        data: new Uint8ClampedArray(frame),
        width: message.width,
        height: message.height,
      } as ImageData,
      robustPass ? { ...FAST_OPTIONS, tryHarder: true, tryInvert: true } : FAST_OPTIONS,
    );
    const result = results.find((entry) => entry.isValid && entry.format === "QRCode");
    if (!result) {
      missStreak += 1;
      send({ type: "miss", frameId: message.frameId, ...performanceSample(Date.now() - startedAt) });
      return;
    }

    missStreak = 0;
    const bytes = Uint8Array.from(result.bytes);
    send({
      type: "decoded",
      frameId: message.frameId,
      text: result.text,
      bytes: bytes.buffer,
      ...performanceSample(Date.now() - startedAt),
    });
  } catch (error) {
    send({
      type: "decode-error",
      frameId: message.frameId,
      message: String(error),
      ...performanceSample(Date.now() - startedAt),
    });
  }
}

async function pump(): Promise<void> {
  if (!wasmReady || decoding || !pendingFrame) return;
  const frame = pendingFrame;
  pendingFrame = null;
  decoding = true;
  try {
    await decodeFrame(frame);
  } finally {
    decoding = false;
    if (pendingFrame) void pump();
  }
}

worker.onMessage((event) => {
  const message = unwrapMessage(event as WechatMiniprogram.WorkerOnMessageListenerResult);
  if (!message) return;
  pendingFrame = message;
  void pump();
});

void prepareZXingModule({
  overrides: {
    instantiateWasm(imports: WebAssembly.Imports, successCallback: (instance: WebAssembly.Instance) => void) {
      void WXWebAssembly.instantiate("wasm/zxing_full.wasm", imports as unknown as WXWebAssembly.Imports)
        .then((result: WXWebAssembly.Instance | { instance: WXWebAssembly.Instance }) => {
          const instance = "instance" in result ? result.instance : result;
          successCallback(instance as unknown as WebAssembly.Instance);
        })
        .catch((error: unknown) => send({ type: "wasm-error", message: String(error) }));
      return {};
    },
  },
  fireImmediately: true,
})
  .then(() => {
    wasmReady = true;
    send({ type: "ready", engine: "ZXing-C++ WASM" });
    void pump();
  })
  .catch((error: unknown) => send({ type: "wasm-error", message: String(error) }));
