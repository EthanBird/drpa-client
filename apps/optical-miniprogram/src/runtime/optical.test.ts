import { describe, expect, it } from "vitest";

import { createOpticalTransfer, OpticalReceiver } from "./optical";

describe("DRPA mini program optical runtime", () => {
  it("uses the mini-program gzip and SHA-256 adapters for a compatible round trip", async () => {
    const source = new TextEncoder().encode("微信小程序光学传输兼容性测试。".repeat(600));
    const transfer = await createOpticalTransfer(source, "小程序测试.txt", "text/plain", 900);
    const receiver = new OpticalReceiver();

    expect(transfer.compression).toBe("gzip");
    for (let sequence = 0; sequence < transfer.totalChunks; sequence += 1) {
      expect(receiver.accept(transfer.createFrameText(sequence))).toBe(true);
    }

    const result = await receiver.complete();
    expect(result?.name).toBe("小程序测试.txt");
    expect(Array.from(result?.bytes ?? [])).toEqual(Array.from(source));
    expect(result?.sha256).toBe(transfer.sha256);
  });

  it("recovers missing source blocks from repair frames", async () => {
    const source = Uint8Array.from({ length: 28_000 }, (_, index) => (index * 31 + 7) & 0xff);
    const transfer = await createOpticalTransfer(source, "repair.bin", "application/octet-stream", 900);
    const receiver = new OpticalReceiver();

    for (let sequence = 0; sequence < transfer.totalChunks; sequence += 1) {
      if (sequence % 9 !== 2) receiver.accept(transfer.createFrame(sequence));
    }
    for (let sequence = transfer.totalChunks; sequence < transfer.totalChunks * 20 && !receiver.isComplete(); sequence += 1) {
      receiver.accept(transfer.createFrame(sequence));
    }

    expect(receiver.isComplete()).toBe(true);
    expect(Array.from((await receiver.complete())?.bytes ?? [])).toEqual(Array.from(source));
  });
});
