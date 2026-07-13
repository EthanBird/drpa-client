import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { App } from "./App";
import { useAppStore } from "./store";

describe("DRPA Next desktop shell", () => {
  afterEach(cleanup);

  beforeEach(() => {
    useAppStore.setState({
      activeNavigation: "workbench",
      commandOpen: false,
      compactMode: false,
      inspectorOpen: true,
      selectedPackageId: "com.drpa.invoice-hub",
      selectedProfileId: "monthly",
      snapshot: null,
    });
  });

  it("loads the browser-preview workspace and renders the selected task", async () => {
    render(<App />);

    await waitFor(() => expect(screen.getByRole("heading", { name: "月度结算", level: 1 })).toBeVisible());
    expect(screen.getByText("本地脚本包")).toBeVisible();
    expect(screen.getByRole("button", { name: "运行任务" })).toBeEnabled();
  });

  it("opens the command palette with the platform shortcut", async () => {
    render(<App />);
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });

    expect(await screen.findByRole("dialog", { name: "命令面板" })).toBeVisible();
    expect(screen.getByPlaceholderText("输入命令或搜索脚本包…")).toHaveFocus();
  });

  it("navigates to the library without recreating host state", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "脚本包" }));

    expect(await screen.findByRole("heading", { name: "脚本包" })).toBeVisible();
    expect(screen.getByText("发票中心")).toBeVisible();
  });
});
