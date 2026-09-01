import OpticalDecodeWorker from "./opticalDecode.worker?worker";

export interface OpticalDecodedSymbol {
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

export interface OpticalDecodeJob {
  originX?: number;
  originY?: number;
  full: boolean;
  maxSymbols: number;
}

interface WorkerSlot {
  worker: Worker;
  busy: boolean;
  ready: boolean;
}

export class OpticalDecodePool {
  private readonly slots: WorkerSlot[];
  private nextId = 1;
  private stopped = false;

  constructor(
    size: number,
    onDecoded: (symbols: OpticalDecodedSymbol[], details: { engine: "wasm" | "js"; full: boolean }) => void,
    onReady?: (details: { ready: number; total: number; engine: "wasm" | "js" }) => void,
  ) {
    this.slots = Array.from({ length: Math.max(1, Math.min(4, size)) }, () => {
      const worker = new OpticalDecodeWorker();
      const slot: WorkerSlot = { worker, busy: false, ready: false };
      worker.onmessage = (event: MessageEvent<{
        type: "ready" | "decoded";
        engine: "wasm" | "js";
        full?: boolean;
        symbols?: OpticalDecodedSymbol[];
      }>) => {
        if (this.stopped) return;
        if (event.data.type === "ready") {
          slot.ready = true;
          onReady?.({ ready: this.readyCount, total: this.size, engine: event.data.engine });
          return;
        }
        slot.busy = false;
        onDecoded(event.data.symbols ?? [], { engine: event.data.engine, full: Boolean(event.data.full) });
      };
      worker.onerror = () => { slot.busy = false; };
      return slot;
    });
  }

  get size(): number {
    return this.slots.length;
  }

  get readyCount(): number {
    return this.slots.filter((slot) => slot.ready).length;
  }

  get busyCount(): number {
    return this.slots.filter((slot) => slot.busy).length;
  }

  get freeCount(): number {
    return this.slots.filter((slot) => slot.ready && !slot.busy).length;
  }

  submit(image: ImageData, job: OpticalDecodeJob): boolean {
    const slot = this.slots.find((candidate) => candidate.ready && !candidate.busy);
    if (!slot || this.stopped) return false;
    slot.busy = true;
    const buffer = image.data.buffer as ArrayBuffer;
    slot.worker.postMessage({
      type: "decode",
      id: this.nextId++,
      buffer,
      width: image.width,
      height: image.height,
      originX: job.originX ?? 0,
      originY: job.originY ?? 0,
      full: job.full,
      maxSymbols: job.maxSymbols,
    }, [buffer]);
    return true;
  }

  terminate(): void {
    this.stopped = true;
    for (const slot of this.slots) slot.worker.terminate();
    this.slots.length = 0;
  }
}

