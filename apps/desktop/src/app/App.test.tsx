import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { StudioPage } from "../pages/StudioPage";
import { desktopGateway } from "../infra/gateway";
import { App } from "./App";
import { useAppStore } from "./store";

vi.mock("@monaco-editor/react", async () => {
  const React = await vi.importActual<typeof import("react")>("react");
  return {
    default: ({ value, onChange }: { value?: string; onChange?: (value: string) => void }) =>
      React.createElement("textarea", {
        "aria-label": "mock-editor",
        value: value ?? "",
        onChange: (event: { target: { value: string } }) => onChange?.(event.target.value),
      }),
    loader: { config: vi.fn() },
  };
});
vi.mock("monaco-editor/esm/vs/editor/editor.api.js", () => ({}));
vi.mock("monaco-editor/esm/vs/editor/editor.worker.js?worker", () => ({ default: class EditorWorker {} }));
vi.mock("monaco-editor/esm/vs/basic-languages/python/python.contribution.js", () => ({}));
vi.mock("monaco-editor/esm/vs/basic-languages/yaml/yaml.contribution.js", () => ({}));


describe("DRPA Next desktop shell", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  beforeEach(() => {
    localStorage.clear();
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

  it("runs the first matching command from the command palette with Enter", async () => {
    render(<App />);
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    const input = await screen.findByPlaceholderText("输入命令或搜索脚本包…");

    fireEvent.change(input, { target: { value: "记录" } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(await screen.findByRole("heading", { name: "运行记录" })).toBeVisible();
  });

  it("navigates to the library without recreating host state", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "脚本包" }));

    expect(await screen.findByRole("heading", { name: "脚本包" })).toBeVisible();
    expect(screen.getByText("发票中心")).toBeVisible();
  });

  it("shows automation schedules from the workspace snapshot", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "自动化计划" }));

    expect(await screen.findByRole("heading", { name: "自动化计划" })).toBeVisible();
    expect(screen.getByText("月度发票归档")).toBeVisible();
    expect(screen.getByText("每月 1 日 · 08:30")).toBeVisible();
    expect(screen.getByText("P2 Scheduler")).toBeVisible();
  });

  it("creates a Studio project from a display name without asking for an id", async () => {
    render(<StudioPage />);

    expect(await screen.findByRole("heading", { name: "开发工作室" })).toBeVisible();
    expect(screen.queryByLabelText(/项目 ID/i)).not.toBeInTheDocument();
    const name = screen.getByLabelText("项目名称");
    fireEvent.change(name, { target: { value: "每日图片" } });
    fireEvent.click(screen.getByRole("button", { name: "新建项目" }));

    expect(await screen.findByText(/内部 ID 已自动生成/)).toBeVisible();
  });

  it("opens the Studio file context menu", async () => {
    const { container } = render(<StudioPage />);
    expect(await screen.findByRole("heading", { name: "开发工作室" })).toBeVisible();

    const filePane = container.querySelector(".studio-files");
    expect(filePane).not.toBeNull();
    fireEvent.contextMenu(filePane!);

    const menu = container.querySelector(".studio-context-menu");
    expect(menu).not.toBeNull();
    expect(menu).toHaveTextContent("新建文件");
    expect(menu).toHaveTextContent("新建文件夹");
    expect(menu).toHaveTextContent("导入文件");
  });

  it("creates and selects a local task profile", async () => {
    vi.spyOn(window, "prompt").mockReturnValue("回归测试配置");
    const startRun = vi.spyOn(desktopGateway, "startRun").mockResolvedValue("run-regression");
    render(<App />);
    await screen.findByRole("heading", { name: "月度结算", level: 1 });

    fireEvent.click(screen.getByRole("button", { name: "创建任务配置" }));

    expect(await screen.findByRole("heading", { name: "回归测试配置", level: 1 })).toBeVisible();
    expect(screen.getByText("已创建任务配置：回归测试配置")).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "运行任务" }));
    await waitFor(() => expect(startRun).toHaveBeenCalledWith("com.drpa.invoice-hub", "monthly", {}));
  });
});
