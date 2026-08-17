import OpticalDecodeWorker from "./opticalDecode.worker?worker&inline";

export interface OpticalDecodedSymbol {
  text: string;
  box: { x: number; y: number; width: number; height: number };
}

interface WorkerSlot {
  worker: Worker;
  busy: boolean;
}

export class OpticalDecodePool {
  private readonly slots: WorkerSlot[];
  private nextId = 1;

  constructor(size: number, onDecoded: (symbols: OpticalDecodedSymbol[]) => void) {
    this.slots = Array.from({ length: Math.max(1, Math.min(4, size)) }, () => {
      const worker = new OpticalDecodeWorker();
      const slot: WorkerSlot = { worker, busy: false };
      worker.onmessage = (event: MessageEvent<{ id: number; symbols: OpticalDecodedSymbol[] }>) => {
        slot.busy = false;
        onDecoded(event.data.symbols);
      };
      worker.onerror = () => { slot.busy = false; };
      return slot;
    });
  }

  get busyCount(): number {
    return this.slots.filter((slot) => slot.busy).length;
  }

  submit(image: ImageData, maxSymbols: number): boolean {
    const slot = this.slots.find((candidate) => !candidate.busy);
    if (!slot) return false;
    slot.busy = true;
    slot.worker.postMessage({
      id: this.nextId++,
      buffer: image.data.buffer,
      width: image.width,
      height: image.height,
      maxSymbols,
    }, [image.data.buffer]);
    return true;
  }

  terminate(): void {
    for (const slot of this.slots) slot.worker.terminate();
    this.slots.length = 0;
  }
}

