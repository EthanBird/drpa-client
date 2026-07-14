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
    const agentSession = { id: "agent-test-session", title: "新对话", projectId: "", createdAt: 1, updatedAt: 1, messages: [] };
    useAppStore.setState({
      activeNavigation: "workbench",
      commandOpen: false,
      theme: "light",
      inspectorOpen: true,
      selectedPackageId: "com.drpa.invoice-hub",
      selectedProfileId: "monthly",
      snapshot: null,
      agentBaseUrl: "https://api.openai.com/v1",
      agentModel: "gpt-5.4-mini",
      agentProjectId: "",
      agentInspectorOpen: true,
      agentSessions: [agentSession],
      activeAgentSessionId: agentSession.id,
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

  it("uses light theme by default and switches theme from settings", async () => {
    render(<App />);
    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("light"));
    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    const darkTheme = await screen.findByRole("radio", { name: "暗色" });
    fireEvent.click(darkTheme);

    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("dark"));
    expect(screen.queryByText("紧凑布局")).not.toBeInTheDocument();
  });

  it("shows the current system account instead of a hard-coded profile", async () => {
    vi.spyOn(desktopGateway, "getCurrentUser").mockResolvedValue({ displayName: "ci-runner", accountName: "CI\\ci-runner", initials: "CR" });
    render(<App />);

    expect(await screen.findByText("ci-runner")).toBeVisible();
    expect(screen.getByText("本机用户")).toBeVisible();
    expect(screen.getByText("CR")).toBeVisible();
  });

  it("runs the lightweight AI Agent with persisted endpoint settings and a session key", async () => {
    const project = { id: "project-000000000000000000000001", name: "Agent 测试项目", files: ["manifest.yaml", "main.py"] };
    vi.spyOn(desktopGateway, "listStudioProjects").mockResolvedValue([project]);
    const runAgent = vi.spyOn(desktopGateway, "runAgentTurn").mockResolvedValue({
      message: "项目校验通过。",
      tools: [{ callId: "call-1", name: "rpaz_validate", status: "completed", summary: "manifest.yaml 校验通过", output: '{"ok":true}' }],
      usage: { promptTokens: 20, completionTokens: 10 },
      durationMs: 31,
    });
    useAppStore.setState({ activeNavigation: "agent" });
    render(<App />);

    expect(await screen.findByRole("heading", { name: "AI Agent" })).toBeVisible();
    fireEvent.change(screen.getByLabelText("Agent 开发项目"), { target: { value: project.id } });
    fireEvent.change(screen.getByLabelText("API Key"), { target: { value: "session-key" } });
    const composer = screen.getByPlaceholderText(/向 Agent 描述/);
    fireEvent.change(composer, { target: { value: "校验当前项目" } });
    fireEvent.keyDown(composer, { key: "Enter", ctrlKey: true });

    await waitFor(() => expect(runAgent).toHaveBeenCalledWith(expect.objectContaining({
      baseUrl: "https://api.openai.com/v1",
      model: "gpt-5.4-mini",
      apiKey: "session-key",
      projectId: project.id,
    })));
    expect(await screen.findByText("项目校验通过。")).toBeVisible();
    expect(screen.getByText("manifest.yaml 校验通过")).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "隐藏 Agent 配置" }));
    expect(screen.queryByLabelText("OpenAI 兼容 URL")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "显示 Agent 配置" }));
    expect(screen.getByLabelText("OpenAI 兼容 URL")).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "新建 Agent 对话" }));
    expect(screen.getByText("从一个 RPAZ 开发任务开始")).toBeVisible();
    const previousSession = screen.getByRole("button", { name: "打开对话 校验当前项目" });
    fireEvent.click(previousSession);
    expect(screen.getByText("项目校验通过。")).toBeVisible();
  });

  it("opens bundled multi-page HTML development documentation", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "开发文档" }));

    expect(await screen.findByRole("heading", { name: "开发文档" })).toBeVisible();
    expect(screen.getByTitle("DRPA 开发概览")).toHaveAttribute("src", expect.stringContaining("docs/index.html"));
    fireEvent.click(screen.getByRole("button", { name: /RPAZ 规范/ }));
    expect(screen.getByTitle("DRPA RPAZ 规范")).toHaveAttribute("src", expect.stringContaining("docs/rpaz.html"));
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

  it("creates Studio files with an inline explorer input", async () => {
    const project = { id: "project-000000000000000000000001", name: "测试项目", files: ["main.py"] };
    const prompt = vi.spyOn(window, "prompt");
    vi.spyOn(desktopGateway, "listStudioProjects").mockResolvedValue(project.files.length ? [project] : []);
    const writeFile = vi.spyOn(desktopGateway, "writeProjectFile").mockResolvedValue();
    render(<StudioPage />);

    await screen.findByText("测试项目");
    fireEvent.click(screen.getByTitle("新建文件"));
    const input = screen.getByLabelText("新文件名称");
    fireEvent.change(input, { target: { value: "worker.py" } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => expect(writeFile).toHaveBeenCalledWith(project.id, "worker.py", ""));
    expect(prompt).not.toHaveBeenCalled();
  });

  it("keeps large notebooks inside a scroll region and paints running state before execution completes", async () => {
    const project = { id: "project-000000000000000000000001", name: "Notebook 压力测试", files: ["notebook.ipynb"] };
    const notebook = JSON.stringify({
      cells: Array.from({ length: 30 }, (_, index) => ({ cell_type: "code", execution_count: null, metadata: {}, outputs: [], source: `value_${index} = ${index}` })),
      metadata: {}, nbformat: 4, nbformat_minor: 5,
    });
    vi.spyOn(desktopGateway, "listStudioProjects").mockResolvedValue([project]);
    vi.spyOn(desktopGateway, "readProjectFile").mockResolvedValue(notebook);
    vi.spyOn(desktopGateway, "prepareStudioKernel").mockResolvedValue();
    let resolveExecution!: (value: Awaited<ReturnType<typeof desktopGateway.executeStudioCell>>) => void;
    vi.spyOn(desktopGateway, "executeStudioCell").mockImplementation(() => new Promise((resolve) => { resolveExecution = resolve; }));
    const { container } = render(<StudioPage />);

    expect(await screen.findByText("Notebook 压力测试")).toBeVisible();
    await waitFor(() => expect(container.querySelectorAll(".notebook-cell")).toHaveLength(30));
    expect(container.querySelector(".notebook-scroll")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "运行单元格 1" }));
    expect(await screen.findByText("正在运行")).toBeVisible();

    resolveExecution({ executionCount: 1, stdout: "", stderr: "", result: "0", traceback: [], outputs: [], variables: [], durationMs: 8 });
    await waitFor(() => expect(screen.getByText("Kernel 就绪")).toBeVisible());
  });

  it("deletes a Studio project from its context menu after in-app confirmation", async () => {
    const project = { id: "project-000000000000000000000001", name: "待删除项目", files: ["main.py"] };
    vi.spyOn(desktopGateway, "listStudioProjects").mockResolvedValueOnce([project]).mockResolvedValue([]);
    const deleteProject = vi.spyOn(desktopGateway, "deleteStudioProject").mockResolvedValue();
    render(<StudioPage />);

    const projectButton = await screen.findByRole("button", { name: /待删除项目/ });
    fireEvent.contextMenu(projectButton);
    fireEvent.click(screen.getByRole("button", { name: "删除开发项目" }));
    expect(screen.getByRole("dialog", { name: "确认删除" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: /^删除$/ }));

    await waitFor(() => expect(deleteProject).toHaveBeenCalledWith(project.id));
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
