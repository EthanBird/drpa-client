import { describe, expect, it } from "vitest";

import {
  createOpticalCarousel,
  createOpticalTransfer,
  decodeOpticalFrame,
  OpticalReceiver,
} from "./opticalProtocol";

describe("DRPA optical transfer protocol", () => {
  it("reassembles out-of-order QR frames and verifies the file digest", async () => {
    const source = new TextEncoder().encode("光学通道 round trip ".repeat(160));
    const transfer = await createOpticalTransfer(source, "测试/报告.txt", "text/plain", 320);
    const receiver = new OpticalReceiver();

    expect(receiver.accept(transfer.metadataFrame)).toBe(true);
    for (const frame of [...transfer.dataFrames].reverse()) expect(receiver.accept(frame)).toBe(true);
    expect(receiver.progress()).toMatchObject({ name: "报告.txt", receivedChunks: transfer.dataFrames.length, percent: 100 });

    const completed = await receiver.complete();
    expect(completed?.name).toBe("报告.txt");
    expect(Array.from(completed?.bytes ?? [])).toEqual(Array.from(source));
    expect(completed?.sha256).toBe(transfer.sha256);
  });

  it("ignores duplicate and corrupted data frames", async () => {
    const transfer = await createOpticalTransfer(new Uint8Array(700).fill(23), "payload.bin", "application/octet-stream", 320);
    const receiver = new OpticalReceiver();
    receiver.accept(transfer.metadataFrame);
    expect(receiver.accept(transfer.dataFrames[0])).toBe(true);
    expect(receiver.accept(transfer.dataFrames[0])).toBe(false);
    const corrupt = `${transfer.dataFrames[1].slice(0, -1)}${transfer.dataFrames[1].endsWith("A") ? "B" : "A"}`;
    expect(receiver.accept(corrupt)).toBe(false);
    expect(receiver.progress().receivedChunks).toBe(1);
  });

  it("inserts recurring metadata so a receiver can join a long carousel late", async () => {
    const transfer = await createOpticalTransfer(new Uint8Array(5_000), "late.bin", "", 320);
    const carousel = createOpticalCarousel(transfer, 4);
    const metadataCount = carousel.filter((frame) => decodeOpticalFrame(frame)?.kind === "metadata").length;
    expect(metadataCount).toBe(Math.ceil(transfer.dataFrames.length / 4));
    expect(decodeOpticalFrame("unrelated QR content")).toBeNull();
  });
});
