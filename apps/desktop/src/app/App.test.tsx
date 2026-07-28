import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { parsePythonSignature, StudioPage } from "../pages/StudioPage";
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
vi.mock("monaco-editor/esm/vs/basic-languages/sql/sql.contribution.js", () => ({}));


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
      agentMaxRounds: 64,
      agentTemperature: 0.2,
      agentProjectId: "",
      agentInspectorOpen: true,
      activeWorkspaceId: "personal",
      workspaceScopeLoaded: false,
      agentWorkspaceStates: {},
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

  it("creates and enters an isolated workspace from the title bar", async () => {
    const personal = { id: "personal", name: "个人工作区", path: "D:\\DRPA\\data", active: true, createdAt: 1 };
    const created = {
      id: "workspace-00000000000000000000000000000001",
      name: "客户 A",
      path: "D:\\DRPA\\data\\.drpa\\workspaces\\workspace-00000000000000000000000000000001",
      active: false,
      createdAt: 2,
    };
    vi.spyOn(desktopGateway, "listWorkspaces")
      .mockResolvedValueOnce([personal])
      .mockResolvedValueOnce([{ ...personal, active: false }, { ...created, active: true }]);
    const createWorkspace = vi.spyOn(desktopGateway, "createWorkspace").mockResolvedValue(created);
    const switchWorkspace = vi.spyOn(desktopGateway, "switchWorkspace").mockResolvedValue();
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: /个人工作区/ }));
    expect(screen.getByRole("dialog", { name: "切换工作区" })).toBeVisible();
    fireEvent.click(screen.getAllByRole("button", { name: "新建工作区" })[0]);
    fireEvent.change(screen.getByLabelText("新建隔离工作区"), { target: { value: "客户 A" } });
    fireEvent.click(screen.getByRole("button", { name: "创建并进入" }));

    await waitFor(() => expect(createWorkspace).toHaveBeenCalledWith("客户 A"));
    await waitFor(() => expect(switchWorkspace).toHaveBeenCalledWith(created.id));
    await waitFor(() => expect(screen.getByRole("button", { name: /客户 A/ })).toBeVisible());
    expect(localStorage.getItem("drpa-active-workspace-id")).toBe(created.id);
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

  it("parses nested Python signatures for Monaco parameter hints", () => {
    expect(parsePythonSignature("Signature: ctx.sql(query: str, params: tuple[int, str] = ())\n\n执行参数化 SQL。")).toEqual({
      label: "ctx.sql(query: str, params: tuple[int, str] = ())",
      parameters: ["query: str", "params: tuple[int, str] = ()"],
      documentation: "执行参数化 SQL。",
    });
  });

  it("opens the local data workbench and loads the SQLite schema", async () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "数据工作台" }));

    expect(await screen.findByRole("heading", { name: "数据工作台" })).toBeVisible();
    expect(await screen.findByText("example_tasks")).toBeVisible();
    expect(screen.getByText(/workspace\.sqlite3/)).toBeVisible();
  });

  it("develops and streams a Local Dify app, then exposes its compatible API", async () => {
    const runApp = vi.spyOn(desktopGateway, "runLocalDifyApp");
    const startService = vi.spyOn(desktopGateway, "startLocalDifyService");
    useAppStore.setState({ activeNavigation: "localDify" });
    render(<App />);

    expect(await screen.findByRole("heading", { name: "AI 应用" })).toBeVisible();
    expect(await screen.findByRole("heading", { name: "本地 Dify 调试应用" })).toBeVisible();
    const input = screen.getByLabelText("Local Dify 调试输入");
    fireEvent.change(input, { target: { value: "验证本地流式输出" } });
    fireEvent.click(screen.getByRole("button", { name: "运行 Local Dify 应用" }));

    await waitFor(() => expect(runApp).toHaveBeenCalledWith(expect.objectContaining({
      appId: "app-browser-preview",
      query: "验证本地流式输出",
      stream: true,
    })));
    expect(await screen.findByRole("heading", { name: "Local Dify 调试结果" })).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "API 与导出" }));
    const startButton = await screen.findByRole("button", { name: "启动服务" });
    await waitFor(() => expect(startButton).toBeEnabled());
    fireEvent.click(startButton);
    await waitFor(() => expect(startService).toHaveBeenCalledWith(34130));
    expect((await screen.findAllByText("http://127.0.0.1:34130/v1"))[0]).toBeVisible();
  });

  it("creates and edits a visual Local Dify workflow", async () => {
    const validate = vi.spyOn(desktopGateway, "validateLocalDifyWorkflow");
    const save = vi.spyOn(desktopGateway, "saveLocalDifyApp");
    useAppStore.setState({ activeNavigation: "localDify" });
    const { container } = render(<App />);

    expect(await screen.findByRole("heading", { name: "AI 应用" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "新建应用" }));
    expect(document.querySelector(".dify-create-dialog")).toBeVisible();
    expect(document.querySelector(".dify-create-dialog > header")).toBeVisible();
    expect(document.querySelector(".dify-create-dialog > footer")).toBeVisible();
    fireEvent.change(screen.getByLabelText("新建 AI 应用名称"), { target: { value: "可视化工作流" } });
    fireEvent.click(screen.getByRole("button", { name: /Workflow.*自动化与批处理工作流/ }));
    fireEvent.click(screen.getByRole("button", { name: "创建应用" }));

    expect(await screen.findByText("工作流编排")).toBeVisible();
    await waitFor(() => expect(container.querySelectorAll(".workflow-node")).toHaveLength(3));
    fireEvent.click(screen.getByRole("button", { name: /模板转换.*组合变量与文本/ }));
    await waitFor(() => expect(container.querySelectorAll(".workflow-node")).toHaveLength(4));
    fireEvent.click(screen.getByRole("button", { name: "校验" }));
    await waitFor(() => expect(validate).toHaveBeenCalled());
    expect(await screen.findByText("工作流有效")).toBeVisible();
    const saveWorkflow = screen.getByRole("button", { name: "保存工作流" });
    await waitFor(() => expect(saveWorkflow).toBeEnabled());
    fireEvent.click(saveWorkflow);
    await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({
      name: "可视化工作流",
      mode: "workflow",
      workflow: expect.objectContaining({ nodes: expect.arrayContaining([expect.objectContaining({ kind: "template-transform" })]) }),
    })));
  });

  it("generates SQL with the configured Agent and inserts it without executing", async () => {
    const runAgent = vi.spyOn(desktopGateway, "runAgentTurn");
    const executeSql = vi.spyOn(desktopGateway, "executeDatabaseSql");
    useAppStore.setState({ activeNavigation: "data", agentStreamEnabled: false });
    render(<App />);

    expect(await screen.findByRole("heading", { name: "数据工作台" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "AI 写 SQL" }));
    const prompt = await screen.findByLabelText("描述查询需求");
    fireEvent.change(prompt, { target: { value: "查询所有待处理任务" } });
    fireEvent.click(screen.getByRole("button", { name: /生成 SQL/ }));

    await waitFor(() => expect(runAgent).toHaveBeenCalledWith(expect.objectContaining({ mode: "sql", projectId: "" })));
    expect(await screen.findByText("SQL 已就绪")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "替换编辑器" }));

    expect((screen.getByLabelText("mock-editor") as HTMLTextAreaElement).value).toContain("FROM example_tasks");
    expect(executeSql).not.toHaveBeenCalled();
  });

  it("creates and connects a remote PostgreSQL profile", async () => {
    const saveProfile = vi.spyOn(desktopGateway, "saveRemoteDatabaseProfile");
    const testConnection = vi.spyOn(desktopGateway, "testRemoteDatabaseConnection");
    const listTables = vi.spyOn(desktopGateway, "listRemoteDatabaseTables");
    useAppStore.setState({ activeNavigation: "data" });
    render(<App />);

    expect(await screen.findByRole("heading", { name: "数据工作台" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "新建数据库连接" }));
    fireEvent.change(screen.getByLabelText("连接名称"), { target: { value: "分析库" } });
    fireEvent.change(screen.getByLabelText("数据库"), { target: { value: "analytics" } });
    fireEvent.change(screen.getByLabelText("用户名"), { target: { value: "reporter" } });
    fireEvent.change(screen.getByLabelText("密码"), { target: { value: "session-secret" } });
    fireEvent.click(screen.getByRole("button", { name: "保存并连接" }));

    await waitFor(() => expect(testConnection).toHaveBeenCalled());
    await waitFor(() => expect(saveProfile).toHaveBeenCalledWith(expect.objectContaining({ engine: "postgresql", database: "analytics" })));
    await waitFor(() => expect(listTables).toHaveBeenCalledWith(expect.any(String), "session-secret"));
    expect(await screen.findByText("public.remote_tasks")).toBeVisible();
  });

  it("creates SQLite and Excel file data sources from the workbench", async () => {
    const saveProfile = vi.spyOn(desktopGateway, "saveRemoteDatabaseProfile");
    useAppStore.setState({ activeNavigation: "data" });
    render(<App />);

    expect(await screen.findByRole("heading", { name: "数据工作台" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "新建数据库连接" }));
    fireEvent.change(screen.getByLabelText("数据源类型"), { target: { value: "sqlite" } });
    fireEvent.change(screen.getByLabelText("连接名称"), { target: { value: "本地分析库" } });
    fireEvent.change(screen.getByLabelText("SQLite 数据库文件"), { target: { value: "D:\\data\\analytics.sqlite3" } });
    expect(screen.queryByLabelText("密码")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "保存并连接" }));
    await waitFor(() => expect(saveProfile).toHaveBeenCalledWith(expect.objectContaining({
      engine: "sqlite",
      database: "D:\\data\\analytics.sqlite3",
    })));

    fireEvent.click(screen.getByRole("button", { name: "新建数据库连接" }));
    fireEvent.change(screen.getByLabelText("数据源类型"), { target: { value: "excel" } });
    expect(screen.getByLabelText("工作簿文件")).toBeVisible();
    expect(screen.getByText(/每个工作表映射为一张只读表/)).toBeVisible();
  });

  it("loads persisted run details when opening the run history", async () => {
    const getRunDetail = vi.spyOn(desktopGateway, "getRunDetail");
    useAppStore.setState({ activeNavigation: "runs" });
    render(<App />);

    expect(await screen.findByRole("heading", { name: "运行记录" })).toBeVisible();
    await waitFor(() => expect(getRunDetail).toHaveBeenCalled());
    expect(await screen.findByRole("button", { name: /日志/ })).toBeVisible();
    expect(screen.getByRole("button", { name: "输出目录" })).toBeVisible();
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

  it("manages local service plugins and exposes a provider endpoint", async () => {
    const startPlugin = vi.spyOn(desktopGateway, "startPlugin");
    const testPluginConnection = vi.spyOn(desktopGateway, "testPluginConnection");
    useAppStore.setState({ activeNavigation: "plugins" });
    render(<App />);

    expect(await screen.findByRole("heading", { name: "插件" })).toBeVisible();
    expect((await screen.findAllByText("Dify Loves Hermes"))[0]).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "启动" }));
    await waitFor(() => expect(startPlugin).toHaveBeenCalledWith("dify-loves-hermes"));
    expect(screen.getAllByText(/Dify App API 转换为 OpenAI 兼容接口/)[0]).toBeVisible();
    expect(screen.getByText("http://127.0.0.1:34121/v1")).toBeVisible();
    fireEvent.click(await screen.findByRole("button", { name: "测试 Dify 连接" }));
    await waitFor(() => expect(testPluginConnection).toHaveBeenCalledWith("dify-loves-hermes"));
    expect(await screen.findByText(/Dify App API 连接成功/)).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "插件开发" }));
    fireEvent.change(screen.getByLabelText("插件项目 ID"), { target: { value: "example-tool" } });
    fireEvent.change(screen.getByLabelText("插件项目名称"), { target: { value: "Example Tool" } });
    fireEvent.click(screen.getByRole("button", { name: "创建项目" }));
    expect(await screen.findByText("Example Tool")).toBeVisible();
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

    const maxRounds = await screen.findByLabelText("Agent 最大模型工具循环");
    expect(maxRounds).toHaveValue(64);
    fireEvent.change(maxRounds, { target: { value: "96" } });
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
      maxRounds: 96,
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

  it("runs the shared AI Agent from the Studio right panel with the selected project", async () => {
    const project = { id: "project-000000000000000000000042", name: "右侧 Agent 项目", files: ["main.py"] };
    vi.spyOn(desktopGateway, "listStudioProjects").mockResolvedValue([project]);
    vi.spyOn(desktopGateway, "readProjectFile").mockResolvedValue("def main(ctx): pass\n");
    const runAgent = vi.spyOn(desktopGateway, "runAgentTurn").mockResolvedValue({
      message: "已检查当前项目。",
      tools: [],
      usage: { promptTokens: 10, completionTokens: 6 },
      durationMs: 12,
    });
    render(<StudioPage />);

    expect(await screen.findByRole("region", { name: "开发工作室 AI Agent" })).toBeVisible();
    const composer = await screen.findByPlaceholderText(/向 Agent 描述“右侧 Agent 项目”/);
    fireEvent.change(composer, { target: { value: "检查 main.py" } });
    fireEvent.click(screen.getByRole("button", { name: "发送工作室 Agent 消息" }));

    await waitFor(() => expect(runAgent).toHaveBeenCalledWith(expect.objectContaining({
      projectId: project.id,
      maxRounds: 64,
      messages: [expect.objectContaining({ role: "user", content: "检查 main.py" })],
    })));
    expect(await screen.findByText("已检查当前项目。")).toBeVisible();
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
