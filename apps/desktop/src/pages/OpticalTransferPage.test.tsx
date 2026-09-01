import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { OpticalTransfer } from "../features/optical/opticalProtocol";
import { OpticalTransferPage } from "./OpticalTransferPage";

const opticalMocks = vi.hoisted(() => ({ prepare: vi.fn(), renderGrid: vi.fn(), renderWebEntry: vi.fn() }));

vi.mock("../features/optical/opticalProtocol", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../features/optical/opticalProtocol")>();
  return { ...actual, prepareOpticalTransfer: opticalMocks.prepare };
});

vi.mock("../features/optical/opticalQr", () => ({
  renderOpticalQrGrid: opticalMocks.renderGrid,
  renderOpticalWebEntryQr: opticalMocks.renderWebEntry,
}));

describe("OpticalTransferPage QR fullscreen", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("falls back to an application-window fullscreen view when the native API is unavailable", async () => {
    const transfer: OpticalTransfer = {
      sessionId: "12345678",
      name: "sample.bin",
      mime: "application/octet-stream",
      size: 3,
      transmittedSize: 3,
      compression: "none",
      sha256: "00".repeat(32),
      frameBytes: 1465,
      blockBytes: 1435,
      totalChunks: 1,
      createFrame: () => new Uint8Array([1, 2, 3]),
      createFrameText: () => "DRPA2:test",
    };
    opticalMocks.prepare.mockResolvedValue(transfer);
    Object.defineProperty(HTMLElement.prototype, "requestFullscreen", {
      configurable: true,
      value: vi.fn().mockRejectedValue(new Error("not supported")),
    });

    const { container } = render(<OpticalTransferPage />);
    const input = container.querySelector<HTMLInputElement>('input[type="file"]');
    expect(input).not.toBeNull();
    fireEvent.change(input!, { target: { files: [new File(["abc"], "sample.bin")] } });

    const enterButton = await screen.findByRole("button", { name: "全屏显示二维码" });
    fireEvent.click(enterButton);

    const exitButton = await screen.findByRole("button", { name: "退出二维码全屏" });
    expect(exitButton.closest(".optical-stage")).toHaveClass("is-qr-fullscreen");
    fireEvent.click(exitButton);
    await waitFor(() => expect(screen.getByRole("button", { name: "全屏显示二维码" })).toBeVisible());
  });

  it("offers both the WeChat mini program and browser receiver with enlarged scan codes", async () => {
    render(<OpticalTransferPage webEntryUrl="https://example.test/optical/" />);

    expect(screen.getByRole("region", { name: "手机接收端入口" })).toBeVisible();
    expect(screen.getByAltText("DRPA 光学传输微信小程序码")).toBeVisible();
    expect(screen.getByLabelText("扫码打开 DRPA 光学传输网页版")).toBeVisible();
    expect(opticalMocks.renderWebEntry).toHaveBeenCalledWith(expect.any(HTMLCanvasElement), "https://example.test/optical/");

    fireEvent.click(screen.getByRole("button", { name: "放大微信小程序码" }));
    const miniProgramDialog = screen.getByRole("dialog", { name: "微信小程序码" });
    expect(within(miniProgramDialog).getByAltText("放大的 DRPA 光学传输微信小程序码")).toBeVisible();
    expect(within(miniProgramDialog).getByText(/微信原生文件转发面板/)).toBeVisible();
    fireEvent.click(within(miniProgramDialog).getByRole("button", { name: "关闭手机接收端二维码" }));

    fireEvent.click(screen.getByRole("button", { name: "放大网页版二维码" }));
    const webDialog = screen.getByRole("dialog", { name: "手机网页版二维码" });
    const expandedWebCanvas = within(webDialog).getByLabelText("放大的 DRPA 光学传输网页版二维码");
    expect(expandedWebCanvas).toBeVisible();
    await waitFor(() => expect(opticalMocks.renderWebEntry).toHaveBeenCalledWith(expandedWebCanvas, "https://example.test/optical/"));
  });
});
