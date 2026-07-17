import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { StudioPage } from "../pages/StudioPage";
import { WorkbenchPage } from "../pages/WorkbenchPage";
import { RuntimePage } from "../pages/RuntimePage";
import { SettingsPage } from "../pages/SettingsPage";
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
      fontScale: "standard",
      inspectorOpen: true,
      selectedPackageId: "com.drpa.invoice-hub",
      selectedProfileId: "monthly",
      snapshot: null,
      agentBaseUrl: "https://api.openai.com/v1",
      agentModel: "gpt-5.4-mini",
      agentApiKey: "",
      agentStreamEnabled: true,
      agentContextWindow: 128000,
      agentMaxOutputTokens: 4096,
      agentTemperature: 0.2,
      agentProjectId: "",
      agentInspectorOpen: true,
      agentSessions: [agentSession],
      activeAgentSessionId: agentSession.id,
    });
  });

  it("loads the browser-preview workspace and renders the selected task", async () => {
    const reportUiReady = vi.spyOn(desktopGateway, "reportUiReady");
    render(<App />);

    await waitFor(() => expect(screen.getByRole("heading", { name: "月度结算", level: 1 })).toBeVisible());
    expect(screen.getByText("本地脚本包")).toBeVisible();
    expect(screen.getByRole("button", { name: "运行任务" })).toBeEnabled();
    await waitFor(() => expect(reportUiReady).toHaveBeenCalledOnce());
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
    const openWorkspace = vi.spyOn(desktopGateway, "openWorkspaceDataDirectory").mockResolvedValue();
    render(<App />);
    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("light"));
    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    const darkTheme = await screen.findByRole("radio", { name: "暗色" });
    fireEvent.click(darkTheme);

    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("dark"));
    fireEvent.click(screen.getByRole("radio", { name: /大110%/ }));
    await waitFor(() => expect(document.documentElement.dataset.fontScale).toBe("large"));
    fireEvent.click(screen.getByRole("button", { name: "在资源管理器中打开" }));
    await waitFor(() => expect(openWorkspace).toHaveBeenCalledOnce());
    expect(screen.queryByText("紧凑布局")).not.toBeInTheDocument();
  });

  it("uses Linux platform capabilities and hides the Windows updater", async () => {
    vi.spyOn(desktopGateway, "getPlatformCapabilities").mockResolvedValue({
      os: "linux",
      displayName: "Linux x86_64",
      runtimeTarget: "linux-x86_64",
      supportsWindowsUpdates: false,
      fileManagerName: "文件管理器",
      dataDirectoryPolicy: "XDG 本地数据目录",
    });
    render(<SettingsPage />);

    expect(await screen.findByRole("button", { name: "在文件管理器中打开" })).toBeVisible();
    expect(screen.queryByRole("heading", { name: "Windows 轻量热更新" })).not.toBeInTheDocument();
    expect(screen.getByText(/XDG 本地数据目录/)).toBeVisible();
  });

  it("requires an explicit in-app confirmation before rebuilding the sealed runtime", async () => {
    const repairRuntime = vi.spyOn(desktopGateway, "repairRuntime").mockResolvedValue({
      state: "ready",
      bundleVersion: "test",
      pythonVersion: "3.11.9",
      runtimeRoot: "runtime",
      environmentRoot: "environment",
      browserExecutable: "browser",
      message: "ready",
    });
    render(<RuntimePage />);

    fireEvent.click(await screen.findByRole("button", { name: "强制重建生成环境" }));

    expect(screen.getByRole("dialog", { name: "确认强制重建运行环境" })).toBeVisible();
    expect(repairRuntime).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "确认重建" }));
    await waitFor(() => expect(repairRuntime).toHaveBeenCalledOnce());
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
    fireEvent.click(screen.getByRole("button", { name: "删除对话 新对话" }));
    expect(screen.getByRole("dialog", { name: "确认删除对话" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(screen.getByRole("button", { name: "打开对话 新对话" })).toBeVisible();
    const previousSession = screen.getByRole("button", { name: "打开对话 校验当前项目" });
    fireEvent.click(previousSession);
    expect(screen.getByText("项目校验通过。")).toBeVisible();
  });

  it("renders streamed Agent Markdown and supports editing or regenerating the latest turn", async () => {
    useAppStore.setState({ activeNavigation: "agent" });
    let streamListener: ((event: import("../domain/models").AgentStreamEvent) => void) | undefined;
    vi.spyOn(desktopGateway, "listenAgentStream").mockImplementation(async (_requestId, listener) => {
      streamListener = listener;
      return () => {};
    });
    let resolveFirst!: (value: Awaited<ReturnType<typeof desktopGateway.runAgentTurn>>) => void;
    const result = { message: "# 实时结果\n\nMarkdown 已完成。", tools: [], usage: { promptTokens: 30, completionTokens: 12 }, durationMs: 80 };
    const runAgent = vi.spyOn(desktopGateway, "runAgentTurn")
      .mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve; }))
      .mockResolvedValue(result);
    render(<App />);

    const composer = await screen.findByPlaceholderText(/询问 RPAZ/);
    fireEvent.change(composer, { target: { value: "生成 Markdown" } });
    fireEvent.click(screen.getByRole("button", { name: "发送消息" }));
    await waitFor(() => expect(streamListener).toBeDefined());
    act(() => {
      streamListener?.({ type: "roundStarted", round: 1 });
      streamListener?.({ type: "delta", content: "# 实时结果\n\n" });
      streamListener?.({ type: "delta", content: "正在生成" });
    });

    expect(await screen.findByRole("heading", { name: "实时结果" })).toBeVisible();
    expect(screen.getByText("正在生成")).toBeVisible();
    await act(async () => { resolveFirst(result); });
    await waitFor(() => expect(screen.getByText("Markdown 已完成。")).toBeVisible());
    expect(runAgent).toHaveBeenCalledWith(expect.objectContaining({
      stream: true,
      contextWindow: 128000,
      maxOutputTokens: 4096,
      temperature: 0.2,
      requestId: expect.stringMatching(/^req-/),
    }));

    fireEvent.click(screen.getByRole("button", { name: "编辑最新消息" }));
    const editor = screen.getByLabelText("编辑最新用户消息");
    fireEvent.change(editor, { target: { value: "修改后的 Markdown 请求" } });
    fireEvent.click(screen.getByRole("button", { name: "保存并重新生成" }));
    await waitFor(() => expect(runAgent).toHaveBeenCalledTimes(2));
    expect(runAgent.mock.calls[1][0].messages.at(-1)?.content).toBe("修改后的 Markdown 请求");
    await waitFor(() => expect(screen.getByRole("button", { name: "重新生成回复" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "重新生成回复" }));
    await waitFor(() => expect(runAgent).toHaveBeenCalledTimes(3));
  });

  it("opens the local Markdown knowledge library, follows links, edits and creates inline", async () => {
    const write = vi.spyOn(desktopGateway, "writeKnowledgeFile");
    const create = vi.spyOn(desktopGateway, "createKnowledgeEntry");
    const rename = vi.spyOn(desktopGateway, "renameKnowledgeEntry");
    const remove = vi.spyOn(desktopGateway, "deleteKnowledgeEntry");
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "知识文档" }));

    expect(await screen.findByRole("heading", { name: "知识文档" })).toBeVisible();
    expect(await screen.findByRole("heading", { name: "RPAZ 开发指南" })).toBeVisible();
    fireEvent.click(screen.getByRole("link", { name: "快速开始" }));
    expect(await screen.findByRole("heading", { name: "快速开始" })).toBeVisible();

    fireEvent.click(screen.getByTitle("编辑"));
    const editor = screen.getByLabelText("Markdown 编辑器");
    fireEvent.change(editor, { target: { value: "# 已修改\n" } });
    fireEvent.keyDown(window, { key: "s", ctrlKey: true });
    await waitFor(() => expect(write).toHaveBeenCalledWith("RPAZ 开发指南/01_快速开始.md", "# 已修改\n"));

    fireEvent.click(screen.getByRole("button", { name: "新建文档" }));
    const nameInput = screen.getByLabelText("新文档名称");
    fireEvent.change(nameInput, { target: { value: "测试笔记" } });
    fireEvent.keyDown(nameInput, { key: "Enter" });
    await waitFor(() => expect(create).toHaveBeenCalledWith("RPAZ 开发指南/测试笔记.md", "file"));

    fireEvent.click(await screen.findByRole("button", { name: "测试笔记.md 菜单" }));
    fireEvent.click(screen.getByRole("button", { name: "重命名" }));
    const renameInput = screen.getByLabelText("重命名知识条目");
    fireEvent.change(renameInput, { target: { value: "已重命名" } });
    fireEvent.keyDown(renameInput, { key: "Enter" });
    await waitFor(() => expect(rename).toHaveBeenCalledWith("RPAZ 开发指南/测试笔记.md", "RPAZ 开发指南/已重命名.md"));

    fireEvent.click(await screen.findByRole("button", { name: "已重命名.md 菜单" }));
    fireEvent.click(screen.getByRole("button", { name: "删除" }));
    expect(screen.getByText("删除文档？")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "删除" }));
    await waitFor(() => expect(remove).toHaveBeenCalledWith("RPAZ 开发指南/已重命名.md"));
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

  it("opens the RPAZ build directory after exporting", async () => {
    const project = { id: "project-000000000000000000000001", name: "导出测试", files: ["main.py"] };
    vi.spyOn(desktopGateway, "listStudioProjects").mockResolvedValue([project]);
    vi.spyOn(desktopGateway, "readProjectFile").mockResolvedValue("def main(ctx): pass\n");
    vi.spyOn(desktopGateway, "buildStudioProject").mockResolvedValue("G:\\workspace\\build\\test.rpaz");
    const openBuild = vi.spyOn(desktopGateway, "openBuildOutputDirectory").mockResolvedValue();
    render(<StudioPage />);

    expect(await screen.findByText("导出测试")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "导出 RPAZ" }));

    await waitFor(() => expect(openBuild).toHaveBeenCalledOnce());
    expect(await screen.findByText(/RPAZ 已导出并打开所在目录/)).toBeVisible();
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
    const executeCell = vi.spyOn(desktopGateway, "executeStudioCell").mockImplementation(() => new Promise((resolve) => { resolveExecution = resolve; }));
    const { container } = render(<StudioPage />);

    expect(await screen.findByText("Notebook 压力测试")).toBeVisible();
    await waitFor(() => expect(container.querySelectorAll(".notebook-cell")).toHaveLength(30));
    expect(container.querySelector(".notebook-scroll")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "运行单元格 1" }));
    expect(await screen.findByText("正在运行")).toBeVisible();
    await waitFor(() => expect(executeCell).toHaveBeenCalledOnce());

    resolveExecution({ executionCount: 1, stdout: "", stderr: "", result: "0", traceback: [], outputs: [], variables: [], durationMs: 8 });
    await waitFor(() => expect(screen.getByText("Kernel 就绪")).toBeVisible());
  });

  it("runs Markdown notebook cells as a render-and-save action", async () => {
    const project = { id: "project-000000000000000000000001", name: "Markdown Notebook", files: ["notebook.ipynb"] };
    const notebook = JSON.stringify({
      cells: [{ cell_type: "markdown", execution_count: null, metadata: {}, outputs: [], source: "# Markdown 可运行\n\n- 实时预览" }],
      metadata: {}, nbformat: 4, nbformat_minor: 5,
    });
    vi.spyOn(desktopGateway, "listStudioProjects").mockResolvedValue([project]);
    vi.spyOn(desktopGateway, "readProjectFile").mockResolvedValue(notebook);
    vi.spyOn(desktopGateway, "prepareStudioKernel").mockResolvedValue();
    const writeFile = vi.spyOn(desktopGateway, "writeProjectFile").mockResolvedValue();
    render(<StudioPage />);

    expect(await screen.findByText("Markdown Notebook")).toBeVisible();
    const runMarkdown = await screen.findByRole("button", { name: "运行单元格 1" });
    expect(runMarkdown).toBeEnabled();
    fireEvent.click(runMarkdown);

    expect(await screen.findByRole("heading", { name: "Markdown 可运行" })).toBeVisible();
    await waitFor(() => expect(writeFile).toHaveBeenCalledWith(project.id, "notebook.ipynb", expect.stringContaining("Markdown 可运行")));
  });

  it("polls workspace snapshots while a task is running so logs and progress stay live", async () => {
    const initial = await desktopGateway.getWorkspaceSnapshot();
    useAppStore.setState({
      snapshot: initial,
      selectedPackageId: "com.drpa.invoice-hub",
      selectedProfileId: "monthly",
    });
    vi.spyOn(desktopGateway, "startRun").mockResolvedValue("run-live");
    const running = structuredClone(initial);
    running.runs = [{ id: "run-live", packageName: "发票中心", profileName: "月度结算", status: "running", startedAt: "10:00:00", duration: "—", progress: 35 }];
    running.logs = [...running.logs, { id: 9001, time: "10:00:01", level: "info", scope: "runtime", message: "第一段实时日志" }];
    const completed = structuredClone(running);
    completed.runs[0] = { ...completed.runs[0], status: "success", duration: "1.2 s", progress: 100 };
    completed.logs = [...completed.logs, { id: 9002, time: "10:00:02", level: "success", scope: "runtime", message: "第二段实时日志" }];
    const snapshots = vi.spyOn(desktopGateway, "getWorkspaceSnapshot")
      .mockResolvedValueOnce(running)
      .mockResolvedValueOnce(completed);
    render(<WorkbenchPage />);

    fireEvent.click(screen.getByRole("button", { name: "运行任务" }));

    expect(await screen.findByText("第一段实时日志")).toBeVisible();
    expect(await screen.findByText("第二段实时日志")).toBeVisible();
    await waitFor(() => expect(snapshots).toHaveBeenCalledTimes(2));
    expect(screen.getByText("100%")).toBeVisible();
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
    const startRun = vi.spyOn(desktopGateway, "startRun").mockResolvedValue("run-regression");
    render(<App />);
    await screen.findByRole("heading", { name: "月度结算", level: 1 });

    fireEvent.click(screen.getByRole("button", { name: "创建任务配置" }));
    const profileName = screen.getByLabelText("新任务配置名称");
    fireEvent.change(profileName, { target: { value: "回归测试配置" } });
    fireEvent.keyDown(profileName, { key: "Enter" });

    expect(await screen.findByRole("heading", { name: "回归测试配置", level: 1 })).toBeVisible();
    expect(screen.getByText("已创建任务配置：回归测试配置")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "预检并保存" }));
    expect(screen.getByText(/预检通过并已保存/)).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "运行任务" }));
    await waitFor(() => expect(startRun).toHaveBeenCalledWith("com.drpa.invoice-hub", "monthly", {}));

    const profile = screen.getByRole("button", { name: /回归测试配置/ });
    fireEvent.contextMenu(profile);
    fireEvent.click(screen.getByRole("menuitem", { name: "重命名" }));
    const rename = screen.getByLabelText("任务配置名称");
    fireEvent.change(rename, { target: { value: "已重命名配置" } });
    fireEvent.keyDown(rename, { key: "Enter" });
    expect(await screen.findByRole("heading", { name: "已重命名配置", level: 1 })).toBeVisible();

    fireEvent.contextMenu(screen.getByRole("button", { name: /已重命名配置/ }));
    fireEvent.click(screen.getByRole("menuitem", { name: "删除任务配置" }));
    expect(screen.getByRole("dialog", { name: "确认删除任务配置" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "确认删除" }));
    expect(await screen.findByRole("heading", { name: "月度结算", level: 1 })).toBeVisible();
  });
});
