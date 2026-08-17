import QRCode from "qrcode";
import { describe, expect, it } from "vitest";

import {
  createOpticalTransfer,
  decodeBase45,
  encodeBase45,
  MAX_OPTICAL_FRAME_BYTES,
  OpticalReceiver,
} from "./opticalProtocol";

describe("DRPA high-speed optical transfer protocol", () => {
  it("round-trips arbitrary bytes through Base45", () => {
    const source = Uint8Array.from({ length: 1025 }, (_, index) => (index * 197 + 31) & 0xff);
    expect(Array.from(decodeBase45(encodeBase45(source)))).toEqual(Array.from(source));
  });

  it("reassembles out-of-order systematic frames and verifies the file digest", async () => {
    const source = new TextEncoder().encode("光学通道 round trip ".repeat(320));
    const transfer = await createOpticalTransfer(source, "测试/报告.txt", "text/plain", 900);
    const receiver = new OpticalReceiver();

    for (const sequence of Array.from({ length: transfer.totalChunks }, (_, index) => index).reverse()) {
      expect(receiver.accept(transfer.createFrameText(sequence))).toBe(true);
    }
    expect(receiver.isComplete()).toBe(true);
    const completed = await receiver.complete();
    expect(completed?.name).toBe("报告.txt");
    expect(Array.from(completed?.bytes ?? [])).toEqual(Array.from(source));
    expect(completed?.sha256).toBe(transfer.sha256);
  });

  it("uses repair frames to recover dropped source blocks without replaying a full cycle", async () => {
    const source = Uint8Array.from({ length: 24_000 }, (_, index) => index % 251);
    const transfer = await createOpticalTransfer(source, "payload.bin", "application/octet-stream", 900);
    const receiver = new OpticalReceiver();
    const dropped = new Set([1, 4, 9, 13, 17].filter((index) => index < transfer.totalChunks));

    for (let sequence = 0; sequence < transfer.totalChunks; sequence += 1) {
      if (!dropped.has(sequence)) receiver.accept(transfer.createFrame(sequence));
    }
    for (let cycle = 0; cycle < 8 && !receiver.isComplete(); cycle += 1) {
      const start = transfer.totalChunks * (cycle * 2 + 1);
      for (let offset = 0; offset < transfer.totalChunks && !receiver.isComplete(); offset += 1) {
        receiver.accept(transfer.createFrame(start + offset));
      }
    }

    expect(receiver.isComplete()).toBe(true);
    expect(Array.from((await receiver.complete())?.bytes ?? [])).toEqual(Array.from(source));
  });

  it("rejects duplicate and corrupted frames", async () => {
    const transfer = await createOpticalTransfer(new Uint8Array(2_000).fill(23), "payload.bin", "application/octet-stream", 900);
    const receiver = new OpticalReceiver();
    const frame = transfer.createFrame(0);
    expect(receiver.accept(frame)).toBe(true);
    expect(receiver.accept(frame)).toBe(false);
    const corrupted = frame.slice();
    corrupted[corrupted.length - 1] ^= 0xff;
    expect(receiver.accept(corrupted)).toBe(false);
  });

  it("fits the maximum DRPA frame into a version-40 L QR symbol", async () => {
    const transfer = await createOpticalTransfer(new Uint8Array(8_000), "capacity.bin", "application/octet-stream", MAX_OPTICAL_FRAME_BYTES);
    const qr = QRCode.create([{ mode: "alphanumeric", data: transfer.createFrameText(0) }], { errorCorrectionLevel: "L", maskPattern: 2 });
    expect(qr.version).toBeLessThanOrEqual(40);
  });
});
