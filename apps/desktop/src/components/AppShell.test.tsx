import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useAppStore } from "../app/store";
import { AppShell } from "./AppShell";

const nativeWindow = vi.hoisted(() => ({
  close: vi.fn().mockResolvedValue(undefined),
  isMaximized: vi.fn().mockResolvedValue(false),
  minimize: vi.fn().mockResolvedValue(undefined),
  onResized: vi.fn().mockResolvedValue(() => undefined),
  startDragging: vi.fn().mockResolvedValue(undefined),
  toggleMaximize: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => nativeWindow }));

describe("AppShell native title bar", () => {
  beforeEach(() => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
    useAppStore.setState({ activeNavigation: "overview", collapsedSidebars: {} });
    for (const mock of Object.values(nativeWindow)) mock.mockClear();
    nativeWindow.isMaximized.mockResolvedValue(false);
    nativeWindow.onResized.mockResolvedValue(() => undefined);
  });

  afterEach(() => {
    cleanup();
    delete (window as typeof window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  it("does not start a manual drag on a single title-bar click and toggles exactly once on double click", async () => {
    const { container } = render(<AppShell><div>内容</div></AppShell>);
    const dragSurface = container.querySelector<HTMLElement>(".titlebar-spacer");
    expect(dragSurface).not.toBeNull();

    fireEvent.mouseDown(dragSurface!, { button: 0, detail: 1 });
    expect(nativeWindow.startDragging).not.toHaveBeenCalled();

    fireEvent.doubleClick(dragSurface!, { button: 0, detail: 2 });
    await waitFor(() => expect(nativeWindow.toggleMaximize).toHaveBeenCalledTimes(1));
  });
});
