import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { parsePythonSignature, StudioPage } from "../pages/StudioPage";
import { AgentPage, visibleAssistantMessageContent } from "../pages/AgentPage";
import { WorkbenchPage } from "../pages/WorkbenchPage";
import { RuntimePage } from "../pages/RuntimePage";
import { SecretsPage } from "../pages/SecretsPage";
import { SettingsPage } from "../pages/SettingsPage";
import { desktopGateway } from "../infra/gateway";
import { resetAgentWorkspaceRuntimesForTests } from "../features/agent/AgentWorkspaceRuntime";
import { App } from "./App";
import { CONFIGURABLE_NAVIGATION_IDS } from "./navigation";
import { useAppStore } from "./store";

const dialogMocks = vi.hoisted(() => ({
  confirm: vi.fn(),
  open: vi.fn(),
  save: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => dialogMocks);
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
vi.mock("qrcode", () => ({ default: { toDataURL: vi.fn().mockResolvedValue("data:image/png;base64,dmF1bHQ=") } }));


describe("DRPA Next desktop shell", () => {
  it("keeps reasoning tags out of assistant markdown, including old sessions", () => {
    expect(visibleAssistantMessageContent("<think>private</think>Final answer")).toBe("Final answer");
    expect(visibleAssistantMessageContent("<mm:think>private</mm:think>Visible")).toBe("Visible");
    expect(visibleAssistantMessageContent("prefix<think>still streaming")).toBe("prefix");
  });

  afterEach(() => {
    cleanup();
    delete (window as typeof window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    delete document.documentElement.dataset.reducedVisualEffects;
    vi.restoreAllMocks();
  });

  beforeEach(async () => {
    resetAgentWorkspaceRuntimesForTests();
    localStorage.clear();
    for (const session of await desktopGateway.listAgentSessions()) {
      await desktopGateway.deleteAgentSession(session.id);
    }
    dialogMocks.confirm.mockReset();
    dialogMocks.open.mockReset();
    dialogMocks.save.mockReset();
    const agentSession = { id: "agent-test-session", title: "新对话", projectId: "", createdAt: 1, updatedAt: 1, messages: [], selectedSkillIds: [], messageCount: 0 };
    useAppStore.setState({
      activeNavigation: "workbench",
      commandOpen: false,
      theme: "light",
      fontScale: 100,
      language: "zh-CN",
      uiDensity: "compact",
      hidePageHeaders: false,
      navigationMode: "developer",
      customVisibleNavigationIds: [...CONFIGURABLE_NAVIGATION_IDS],
      collapsedSidebars: {},
      inspectorOpen: true,
      selectedPackageId: "com.drpa.invoice-hub",
      selectedProfileId: "monthly",
      snapshot: null,
      agentBaseUrl: "http://127.0.0.1/v1",
      agentModel: "deepseek-v4-flash",
      agentApiKey: "",
      agentProviderRef: null,
      agentMode: "rpaz",
      agentStreamEnabled: true,
      agentContextWindow: 393216,
      agentMaxOutputTokens: 98304,
      agentMaxRounds: 64,
      agentTemperature: 0.2,
      agentPythonTimeoutSeconds: 300,
      agentToolPolicy: {
        enabled: true,
        databaseRead: true,
        databaseConnections: true,
        arbitraryFileRead: true,
        knowledgeBaseRead: true,
        documentRead: true,
        documentWrite: true,
        documentConvert: true,
        projectWrite: true,
        python: true,
        workspaceWrite: true,
        extensions: true,
        browser: true,
        rpazRuns: true,
        runRecords: true,
        vaultRead: true,
        vaultWrite: true,
      },
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
    expect(screen.getByText("本地 RPAZ 包")).toBeVisible();
    expect(screen.getByRole("button", { name: "运行任务" })).toBeEnabled();
    await waitFor(() => expect(reportUiReady).toHaveBeenCalledOnce());
  });

  it("limits the native drag region to the empty title-bar surface", async () => {
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "月度结算", level: 1 });

    const titlebar = container.querySelector(".titlebar");
    const dragSurface = container.querySelector(".titlebar-spacer");
    expect(titlebar).not.toHaveAttribute("data-tauri-drag-region");
    expect(dragSurface).toHaveAttribute("data-tauri-drag-region");
  });

  it("renders localized RPAZ parameter labels while retaining stable ids", async () => {
    const snapshot = await desktopGateway.getWorkspaceSnapshot();
    snapshot.packages[0].parameters = [{
      id: "customer_name",
      label: "客户名称",
      description: "用于报表标题",
      kind: "string",
      required: true,
    }];
    useAppStore.setState({ snapshot, selectedPackageId: snapshot.packages[0].id, selectedProfileId: snapshot.packages[0].profiles[0].id });
    render(<WorkbenchPage />);

    expect(screen.getByText("客户名称")).toBeVisible();
    expect(screen.getByText("用于报表标题")).toBeVisible();
    expect(screen.getByPlaceholderText("请输入 客户名称")).toBeVisible();
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
    expect(screen.getByPlaceholderText("输入命令或搜索 RPAZ 包…")).toHaveFocus();
  });

  it("runs the first matching command from the command palette with Enter", async () => {
    render(<App />);
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    const input = await screen.findByPlaceholderText("输入命令或搜索 RPAZ 包…");

    fireEvent.change(input, { target: { value: "记录" } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(await screen.findByRole("heading", { name: "运行记录" })).toBeVisible();
  });

  it("navigates to the library without recreating host state", async () => {
    const { container } = render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "RPAZ 包" }));

    expect(await screen.findByRole("heading", { name: "RPAZ 包" })).toBeVisible();
    expect(within(container.querySelector('[data-navigation-page="library"]')!).getByRole("heading", { name: "发票中心" })).toBeVisible();
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
    const saveProvider = vi.spyOn(desktopGateway, "saveLocalDifyProvider");
    const saveApp = vi.spyOn(desktopGateway, "saveLocalDifyApp");
    const startService = vi.spyOn(desktopGateway, "startLocalDifyService");
    useAppStore.setState({ activeNavigation: "localDify" });
    render(<App />);

    expect(await screen.findByRole("heading", { name: "流程设计" })).toBeVisible();
    expect(await screen.findByRole("heading", { name: "本地 Dify 调试应用" })).toBeVisible();
    const input = screen.getByLabelText("流程调试输入");
    fireEvent.change(input, { target: { value: "验证本地流式输出" } });
    fireEvent.click(screen.getByRole("button", { name: "运行流程" }));

    await waitFor(() => expect(runApp).toHaveBeenCalledWith(expect.objectContaining({
      appId: "app-browser-preview",
      query: "验证本地流式输出",
      stream: true,
    })));
    expect(saveProvider).toHaveBeenCalledWith(expect.objectContaining({
      id: "provider-agent-settings",
      baseUrl: "http://127.0.0.1/v1",
      model: "deepseek-v4-flash",
    }));
    expect(saveApp).toHaveBeenCalledWith(expect.objectContaining({ providerId: "provider-agent-settings" }));
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

    expect(await screen.findByRole("heading", { name: "流程设计" })).toBeVisible();
    fireEvent.click(screen.getAllByRole("button", { name: "新建流程" })[0]!);
    expect(document.querySelector(".dify-create-dialog")).toBeVisible();
    expect(document.querySelector(".dify-create-dialog > header")).toBeVisible();
    expect(document.querySelector(".dify-create-dialog > footer")).toBeVisible();
    fireEvent.change(screen.getByLabelText("新建流程名称"), { target: { value: "可视化工作流" } });
    fireEvent.click(screen.getByRole("button", { name: /Workflow.*自动化与批处理工作流/ }));
    fireEvent.click(screen.getByRole("button", { name: "创建流程" }));

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
    })), { timeout: 5_000 });
  });

  it("creates a ready-to-run RPAZ workflow from the quick template", async () => {
    const save = vi.spyOn(desktopGateway, "saveLocalDifyApp");
    useAppStore.setState({ activeNavigation: "localDify" });
    const { container } = render(<App />);

    expect(await screen.findByRole("heading", { name: "流程设计" })).toBeVisible();
    fireEvent.click(screen.getAllByRole("button", { name: "新建流程" })[0]!);
    fireEvent.click(screen.getByRole("button", { name: /Workflow.*自动化与批处理工作流/ }));
    fireEvent.click(screen.getByRole("button", { name: "创建流程" }));

    const quickFlow = await screen.findByRole("button", { name: "RPAZ 快速流程" });
    await waitFor(() => expect(quickFlow).toBeEnabled());
    fireEvent.click(quickFlow);

    await waitFor(() => expect(container.querySelectorAll(".workflow-node")).toHaveLength(3));
    expect(screen.getByText(/已生成可运行流程/)).toBeVisible();
    expect(screen.getByRole("combobox", { name: "RPAZ 快速流程包" })).toHaveValue("com.drpa.invoice-hub");
    fireEvent.click(screen.getByRole("button", { name: "保存工作流" }));
    await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({
      workflow: expect.objectContaining({
        nodes: expect.arrayContaining([expect.objectContaining({
          kind: "rpaz-package",
          config: expect.objectContaining({ package_id: "com.drpa.invoice-hub" }),
        })]),
        edges: expect.arrayContaining([
          expect.objectContaining({ source: "start" }),
          expect.objectContaining({ target: "end" }),
        ]),
      }),
    })), { timeout: 5_000 });
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

    saveProfile.mockClear();
    window.dispatchEvent(new CustomEvent("drpa-data-file-drop", {
      detail: { paths: ["D:\\data\\dropped.xlsx"] },
    }));
    await waitFor(() => expect(saveProfile).toHaveBeenCalledWith(expect.objectContaining({
      name: "dropped",
      engine: "excel",
      database: "D:\\data\\dropped.xlsx",
    })));
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
    expect(await screen.findByRole("heading", { name: "关于 DRPA Next" })).toBeVisible();
    expect(screen.getByText("v3.0.0")).toBeVisible();
    const darkTheme = await screen.findByRole("radio", { name: "暗色" });
    fireEvent.click(darkTheme);

    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("dark"));
    fireEvent.change(screen.getByRole("slider", { name: "全局字号" }), { target: { value: "160" } });
    await waitFor(() => expect(document.documentElement.dataset.fontScale).toBe("160"));
    expect(document.documentElement.style.getPropertyValue("--font-scale")).toBe("1.6");
    fireEvent.click(screen.getByRole("button", { name: "在资源管理器中打开" }));
    await waitFor(() => expect(openWorkspace).toHaveBeenCalledOnce());
    expect(screen.queryByText("紧凑布局")).not.toBeInTheDocument();
  });

  it("switches navigation presets and supports custom page entries without disabling settings", async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "设置" }));

    fireEvent.click(await screen.findByRole("radio", { name: "普通用户" }));
    expect(useAppStore.getState().navigationMode).toBe("normal");
    expect(screen.getByRole("button", { name: "BI 主页" })).toBeVisible();
    expect(screen.getByRole("button", { name: "AI Agent" })).toBeVisible();
    expect(screen.getByRole("button", { name: /^运行工作台/ })).toBeVisible();
    expect(screen.getByRole("button", { name: "运行记录" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "开发工作室" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "数据工作台" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "设置" })).toBeVisible();

    fireEvent.click(screen.getByRole("radio", { name: "自定义" }));
    const dataWorkspaceSwitch = screen.getByRole("switch", { name: "数据工作台" });
    expect(dataWorkspaceSwitch).not.toBeChecked();
    fireEvent.click(dataWorkspaceSwitch);
    expect(useAppStore.getState().customVisibleNavigationIds).toContain("data");
    expect(screen.getByRole("button", { name: "数据工作台" })).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "搜索或运行命令" }));
    expect(screen.getByText("打开数据工作台")).toBeVisible();
    expect(screen.queryByText("打开开发工作室")).not.toBeInTheDocument();
  });

  it("renders the low-code BI homepage and exposes the widget palette", async () => {
    useAppStore.setState({ activeNavigation: "overview" });
    render(<App />);

    expect(await screen.findByRole("heading", { name: "业务总览", level: 1 })).toBeVisible();
    expect(screen.getByText("最近运行耗时")).toBeVisible();
    expect(screen.getByText("运行状态分布")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "编辑主页" }));
    expect(screen.getByRole("button", { name: "指标" })).toBeVisible();
    expect(screen.getByRole("button", { name: "折线图" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Markdown" })).toBeVisible();
  });

  it("switches the shell and settings copy through the i18n catalog", async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "设置" }));
    fireEvent.change(screen.getByRole("combobox", { name: "显示语言" }), { target: { value: "en-US" } });

    expect(await screen.findByRole("heading", { name: "Settings" })).toBeVisible();
    expect(screen.getByRole("button", { name: "RPAZ Packages" })).toBeVisible();
    expect(document.documentElement.lang).toBe("en-US");
  });

  it("keeps the compact default and controls Agent tools from the extensions page", async () => {
    render(<App />);
    await waitFor(() => expect(document.documentElement.dataset.uiDensity).toBe("compact"));
    fireEvent.click(screen.getByRole("button", { name: "设置" }));

    expect(screen.queryByRole("radio", { name: "紧凑" })).not.toBeInTheDocument();
    fireEvent.click(await screen.findByRole("switch", { name: "隐藏页面大标题" }));
    await waitFor(() => expect(document.documentElement.dataset.hidePageHeaders).toBe("true"));

    fireEvent.click(screen.getByRole("button", { name: "扩展工具" }));
    expect(await screen.findByRole("heading", { name: "扩展工具" })).toBeVisible();
    const masterToolSwitch = screen.getByRole("switch", { name: "启用 AI Agent 工具" });
    const databaseToolSwitch = screen.getByRole("switch", { name: "只读数据库" });
    expect(masterToolSwitch).toBeChecked();
    expect(databaseToolSwitch).toBeEnabled();
    const systemScope = screen.getByRole("button", { name: /整个操作系统/ });
    const projectScope = screen.getByRole("button", { name: /仅当前项目/ });
    expect(systemScope).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(projectScope);
    expect(useAppStore.getState().agentToolPolicy.fileReadScope).toBe("project");
    expect(projectScope).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(masterToolSwitch);
    expect(databaseToolSwitch).toBeDisabled();
    expect(systemScope).toBeDisabled();
  });

  it("collapses and restores the global and workbench sidebars", async () => {
    render(<App />);

    const hideNavigation = await screen.findByRole("button", { name: "隐藏主侧边栏" });
    fireEvent.click(hideNavigation);
    expect(screen.getByRole("button", { name: "展开主侧边栏" })).toBeVisible();
    expect(screen.getByRole("complementary", { name: "主导航图标栏" })).toBeVisible();
    expect(screen.getByRole("navigation", { name: "主导航" })).toBeVisible();
    expect(screen.getByRole("button", { name: "数据工作台" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "展开主侧边栏" }));
    expect(await screen.findByRole("navigation", { name: "主导航" })).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "数据工作台" }));
    const hideConnections = await screen.findByRole("button", { name: "隐藏数据连接侧边栏" });
    fireEvent.click(hideConnections);
    expect(screen.getByRole("button", { name: "展开数据连接侧边栏" })).toBeVisible();
    expect(screen.getByRole("button", { name: "查询 1" })).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "隐藏字段结构侧边栏" }));
    expect(screen.getByRole("button", { name: "展开字段结构侧边栏" })).toBeVisible();
  });

  it("uses Linux platform capabilities and hides the Windows updater", async () => {
    vi.spyOn(desktopGateway, "getPlatformCapabilities").mockResolvedValue({
      os: "linux",
      displayName: "Linux x86_64",
      runtimeTarget: "linux-x86_64",
      supportsWindowsUpdates: false,
      fileManagerName: "文件管理器",
      dataDirectoryPolicy: "XDG 本地数据目录",
      reducedVisualEffects: false,
    });
    render(<SettingsPage />);

    expect(await screen.findByRole("button", { name: "在文件管理器中打开" })).toBeVisible();
    expect(screen.queryByRole("heading", { name: "Windows 轻量热更新" })).not.toBeInTheDocument();
    expect(screen.getByText(/XDG 本地数据目录/)).toBeVisible();
  });

  it("exports and imports user data through native dialogs with visible feedback", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    const exportData = vi.spyOn(desktopGateway, "exportUserData").mockResolvedValue({
      path: "D:\\backups\\workspace.drpa-data",
      fileCount: 18,
      totalBytes: 65_536,
      workspaceName: "个人工作区",
      restartRequired: false,
    });
    const importData = vi.spyOn(desktopGateway, "importUserData").mockResolvedValue({
      path: "D:\\backups\\workspace.drpa-data",
      fileCount: 18,
      totalBytes: 65_536,
      workspaceName: "导入工作区",
      restartRequired: true,
    });
    dialogMocks.save.mockResolvedValue("D:\\backups\\workspace");
    dialogMocks.open.mockResolvedValue("D:\\backups\\workspace.drpa-data");
    dialogMocks.confirm.mockResolvedValue(true);
    render(<SettingsPage />);

    fireEvent.click(await screen.findByRole("button", { name: "导出用户数据" }));
    await waitFor(() => expect(dialogMocks.save).toHaveBeenCalledWith(expect.objectContaining({
      filters: [{ name: "DRPA 用户数据", extensions: ["drpa-data"] }],
    })));
    await waitFor(() => expect(exportData).toHaveBeenCalledWith("D:\\backups\\workspace.drpa-data"));
    expect(await screen.findByRole("status")).toHaveTextContent(/已导出 18 个文件/);

    fireEvent.click(screen.getByRole("button", { name: "导入用户数据" }));
    await waitFor(() => expect(dialogMocks.open).toHaveBeenCalled());
    await waitFor(() => expect(dialogMocks.confirm).toHaveBeenCalled());
    await waitFor(() => expect(importData).toHaveBeenCalledWith("D:\\backups\\workspace.drpa-data"));
    expect(await screen.findByRole("status")).toHaveTextContent(/已导入 18 个文件到“导入工作区”/);
  });

  it("reports a cancelled or failed user-data export", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    const exportData = vi.spyOn(desktopGateway, "exportUserData");
    dialogMocks.save.mockResolvedValueOnce(null).mockRejectedValueOnce(new Error("save dialog unavailable"));
    render(<SettingsPage />);

    fireEvent.click(await screen.findByRole("button", { name: "导出用户数据" }));
    expect(await screen.findByRole("status")).toHaveTextContent("已取消导出用户数据");
    expect(exportData).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "导出用户数据" }));
    expect(await screen.findByRole("status")).toHaveTextContent(/导出失败：.*save dialog unavailable/);
    expect(exportData).not.toHaveBeenCalled();
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
      profileId: "org.drpa.python-runtime",
      profileName: "Python 3.11 Full",
      features: ["jupyter"],
      profiles: [],
    });
    render(<RuntimePage />);

    fireEvent.click(await screen.findByRole("button", { name: "强制重建生成环境" }));

    expect(screen.getByRole("dialog", { name: "确认强制重建运行环境" })).toBeVisible();
    expect(repairRuntime).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "确认重建" }));
    await waitFor(() => expect(repairRuntime).toHaveBeenCalledOnce());
  });

  it("shows local CPU, memory, and disk metrics in the runtime page", async () => {
    vi.spyOn(desktopGateway, "getSystemMetrics").mockResolvedValue({
      sampledAt: Date.now(),
      cpu: { usagePercent: 25, logicalCores: 8 },
      memory: { usedBytes: 8 * 1024 ** 3, totalBytes: 16 * 1024 ** 3, usagePercent: 50 },
      disk: {
        usedBytes: 200 * 1024 ** 3,
        totalBytes: 500 * 1024 ** 3,
        usagePercent: 40,
        volumes: [{
          name: "本地磁盘 C:",
          mountPoint: "C:\\",
          usedBytes: 200 * 1024 ** 3,
          totalBytes: 500 * 1024 ** 3,
          usagePercent: 40,
        }],
      },
    });

    render(<RuntimePage />);

    expect(await screen.findByRole("region", { name: "本机资源监控" })).toBeVisible();
    expect(await screen.findByRole("progressbar", { name: "CPU使用率" })).toHaveAttribute("aria-valuenow", "25");
    expect(screen.getByRole("progressbar", { name: "内存使用率" })).toHaveAttribute("aria-valuenow", "50");
    expect(screen.getByRole("progressbar", { name: "磁盘使用率" })).toHaveAttribute("aria-valuenow", "40");
    expect(screen.getByText("C:\\")).toBeVisible();
  });

  it("manages Python packages in the selected runtime profile", async () => {
    const catalog = {
      profileId: "org.drpa.python-runtime",
      profileName: "Python 3.11 Full",
      pythonVersion: "3.11.9",
      backend: "uv",
      backendVersion: "uv 0.11.28",
      backendPath: "runtime/tools/uv.exe",
      overlayRoot: "data/runtime-package-overlays/full/site-packages",
      packages: [
        { name: "drpa-runner", version: "2.1.1", source: "runtime", location: "runtime", removable: false },
        { name: "rich", version: "14.0.0", source: "user", location: "overlay", removable: true },
      ],
    };
    vi.spyOn(desktopGateway, "listRuntimePythonPackages").mockResolvedValue(catalog);
    const installPackage = vi.spyOn(desktopGateway, "installRuntimePythonPackage").mockResolvedValue({
      ...catalog,
      packages: [...catalog.packages, { name: "polars", version: "1.32.0", source: "user", location: "overlay", removable: true }],
    });
    const uninstallPackage = vi.spyOn(desktopGateway, "uninstallRuntimePythonPackage").mockResolvedValue({
      ...catalog,
      packages: catalog.packages.filter((item) => item.name !== "rich"),
    });

    render(<RuntimePage />);

    expect(await screen.findByRole("region", { name: "Python 包管理" })).toBeVisible();
    expect(await screen.findByText("uv 0.11.28")).toBeVisible();
    expect(screen.getByText("Runtime 内置")).toBeVisible();
    expect(screen.getByRole("button", { name: "卸载 rich" })).toBeVisible();

    fireEvent.change(screen.getByRole("textbox", { name: "要安装的 Python 包" }), { target: { value: "polars==1.32.0" } });
    fireEvent.click(screen.getByRole("button", { name: "安装" }));
    await waitFor(() => expect(installPackage).toHaveBeenCalledWith("polars==1.32.0"));
    expect(await screen.findByText("polars")).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "卸载 rich" }));
    await waitFor(() => expect(uninstallPackage).toHaveBeenCalledWith("rich"));
  });

  it("shows the current system account instead of a hard-coded profile", async () => {
    vi.spyOn(desktopGateway, "getCurrentUser").mockResolvedValue({ displayName: "ci-runner", accountName: "CI\\ci-runner", initials: "CR" });
    render(<App />);

    expect(await screen.findByText("ci-runner")).toBeVisible();
    expect(screen.getByText("本机用户")).toBeVisible();
    expect(screen.getByText("CR")).toBeVisible();
  });

  it("switches between RPAZ and JCode Agents with persisted endpoint settings", async () => {
    const project = { id: "project-000000000000000000000001", name: "Agent 测试项目", files: ["manifest.yaml", "main.py"] };
    vi.spyOn(desktopGateway, "listStudioProjects").mockResolvedValue([project]);
    const runAgent = vi.spyOn(desktopGateway, "runAgentTurn").mockResolvedValue({
      message: "项目校验通过。",
      tools: [{ callId: "call-1", name: "rpaz_validate", status: "completed", summary: "manifest.yaml 校验通过", output: '{"ok":true}' }],
      usage: { promptTokens: 20, completionTokens: 10 },
      durationMs: 31,
      stopReason: "completed",
      rounds: 1,
      toolCalls: 1,
      retryCount: 0,
    });
    useAppStore.setState({ activeNavigation: "agent" });
    render(<App />);

    expect(await screen.findByRole("heading", { name: "AI Agent" })).toBeVisible();
    expect(await screen.findByText("SESSION.DB")).toBeVisible();
    fireEvent.change(screen.getByLabelText("Agent 开发项目"), { target: { value: project.id } });
    fireEvent.change(screen.getByLabelText("API Key"), { target: { value: "session-key" } });
    const composer = screen.getByPlaceholderText(/向 Agent 描述/);
    fireEvent.change(composer, { target: { value: "校验当前项目" } });
    fireEvent.keyDown(composer, { key: "Enter", ctrlKey: true });

    await waitFor(() => expect(runAgent).toHaveBeenCalledWith(expect.objectContaining({
      baseUrl: "http://127.0.0.1/v1",
      model: "deepseek-v4-flash",
      apiKey: "session-key",
      mode: "rpaz",
      projectId: project.id,
    })));
    expect(await screen.findByText("项目校验通过。")).toBeVisible();
    expect(screen.getByText("manifest.yaml 校验通过")).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "JCode 开发者" }));
    fireEvent.change(composer, { target: { value: "使用完整开发工具继续检查" } });
    fireEvent.click(screen.getByRole("button", { name: "发送消息" }));
    await waitFor(() => expect(runAgent).toHaveBeenCalledTimes(2));
    expect(runAgent.mock.calls[1][0]).toEqual(expect.objectContaining({
      mode: "developer",
      projectId: project.id,
    }));

    fireEvent.click(screen.getByRole("button", { name: "隐藏 Agent 配置" }));
    expect(screen.queryByLabelText("OpenAI 兼容 URL")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "显示 Agent 配置" }));
    expect(screen.getByLabelText("OpenAI 兼容 URL")).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "新建 Agent 对话" }));
    expect(await screen.findByText("JCode 开发者 Agent")).toBeVisible();
    fireEvent.click(screen.getAllByRole("button", { name: "删除对话 新对话" })[0]);
    expect(screen.getByRole("dialog", { name: "确认删除对话" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(screen.getAllByRole("button", { name: "打开对话 新对话" })[0]).toBeVisible();
    const previousSession = screen.getByRole("button", { name: "打开对话 校验当前项目" });
    fireEvent.click(previousSession);
    expect(screen.getAllByText("项目校验通过。")[0]).toBeVisible();
  });

  it("opens an arbitrary file as a persistent project conversation", async () => {
    const project = {
      id: "project-000000000000000000000088",
      name: "external-notes",
      path: "D:\\shared\\external-notes",
      createdAt: 1,
      updatedAt: 1,
      sessionCount: 1,
      source: "external" as const,
    };
    const session = {
      id: "agent-external-file",
      title: "notes.txt",
      projectId: project.id,
      createdAt: 1,
      updatedAt: 1,
      revision: 1,
      messages: [],
      selectedSkillIds: [],
      contextFiles: ["D:\\shared\\external-notes\\notes.txt"],
      messageCount: 0,
      bodyState: "ready" as const,
    };
    vi.spyOn(desktopGateway, "listAgentProjects").mockResolvedValue([project]);
    const openFile = vi.spyOn(desktopGateway, "openAgentFileSession").mockResolvedValue(session);
    useAppStore.setState({ activeNavigation: "agent", workspaceScopeLoaded: true });
    render(<AgentPage />);

    await screen.findByText("SESSION.DB");
    fireEvent.click(screen.getAllByRole("button", { name: "打开任意文件作为项目" })[0]);

    await waitFor(() => expect(openFile).toHaveBeenCalledOnce());
    expect(await screen.findByText("重点文件：notes.txt")).toBeVisible();
    expect(screen.getByText("外部文件项目")).toBeVisible();
    expect(useAppStore.getState().activeAgentSessionId).toBe(session.id);
  });

  it("migrates only legacy Agent sessions missing from session.db before clearing the cache", async () => {
    const existing = {
      id: "agent-existing",
      title: "数据库已有会话",
      projectId: "",
      createdAt: 1,
      updatedAt: 2,
      messages: [],
      selectedSkillIds: [],
      messageCount: 0,
    };
    const missing = {
      id: "agent-missing",
      title: "待迁移会话",
      projectId: "",
      createdAt: 3,
      updatedAt: 4,
      messages: [{ id: "legacy-message", role: "user" as const, content: "保留我" }],
      selectedSkillIds: ["data-analysis"],
      messageCount: 1,
    };
    vi.spyOn(desktopGateway, "listAgentSessions")
      .mockResolvedValueOnce([{
        id: existing.id,
        title: existing.title,
        projectId: null,
        createdAt: existing.createdAt,
        updatedAt: existing.updatedAt,
        messageCount: 0,
        selectedSkillIds: [],
      }])
      .mockResolvedValue([{
        id: existing.id,
        title: existing.title,
        projectId: null,
        createdAt: existing.createdAt,
        updatedAt: existing.updatedAt,
        messageCount: 0,
        selectedSkillIds: [],
      }, {
        id: missing.id,
        title: missing.title,
        projectId: null,
        createdAt: missing.createdAt,
        updatedAt: missing.updatedAt,
        messageCount: 1,
        selectedSkillIds: missing.selectedSkillIds,
      }]);
    const saveSession = vi.spyOn(desktopGateway, "saveAgentSession").mockResolvedValue(missing);
    useAppStore.setState({
      activeWorkspaceId: "personal",
      workspaceScopeLoaded: true,
      agentWorkspaceStates: {
        personal: {
          sessions: [existing, missing],
          activeSessionId: existing.id,
          projectId: "",
        },
      },
      agentSessions: [existing, missing],
      activeAgentSessionId: existing.id,
    });

    render(<AgentPage />);

    await waitFor(() => expect(saveSession).toHaveBeenCalledTimes(1));
    expect(saveSession).toHaveBeenCalledWith(expect.objectContaining({
      id: missing.id,
      messages: missing.messages,
    }));
    expect(saveSession).not.toHaveBeenCalledWith(expect.objectContaining({ id: existing.id }));
    await waitFor(() => expect(useAppStore.getState().agentWorkspaceStates.personal).toBeUndefined());
  });

  it("loads a shared Agent conversation body once across the main and embedded surfaces", async () => {
    const created = await desktopGateway.createAgentSession("shared-project", "共享会话");
    const stored = await desktopGateway.saveAgentSession({
      ...created,
      messages: [{ id: "preserved-message", role: "assistant", content: "这条正文不能被摘要覆盖" }],
      messageCount: 1,
    });
    const getSession = vi.spyOn(desktopGateway, "getAgentSession");
    useAppStore.setState({
      activeWorkspaceId: "personal",
      workspaceScopeLoaded: true,
      agentSessions: [],
      activeAgentSessionId: stored.id,
    });

    render(
      <>
        <AgentPage />
        <AgentPage embedded embeddedProjectId="shared-project" embeddedProjectName="共享项目" />
      </>,
    );

    await waitFor(() => {
      const session = useAppStore.getState().agentSessions.find((item) => item.id === stored.id);
      expect(session?.bodyState).toBe("ready");
      expect(session?.messages).toEqual(stored.messages);
    });
    expect(getSession).toHaveBeenCalledTimes(1);
  });

  it("starts a manifest-driven Dify2API plugin and runs its declared debugger endpoint", async () => {
    const setPluginEnabled = vi.spyOn(desktopGateway, "setPluginEnabled");
    const startPlugin = vi.spyOn(desktopGateway, "startPlugin");
    const runPluginDebugger = vi.spyOn(desktopGateway, "runPluginDebugger");
    const createPluginProject = vi.spyOn(desktopGateway, "createPluginProject");
    useAppStore.setState({ activeNavigation: "plugins" });
    render(<App />);

    expect(await screen.findByRole("heading", { name: "插件工作台" })).toBeVisible();
    expect((await screen.findAllByText("Dify2API 网关"))[0]).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "启用" }));
    await waitFor(() => expect(setPluginEnabled).toHaveBeenCalledWith("dify2api", true));
    await waitFor(() => expect(screen.getByRole("button", { name: "启动" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "启动" }));
    await waitFor(() => expect(startPlugin).toHaveBeenCalledWith("dify2api"));
    expect(screen.getAllByText(/将 Dify Agent 转换为 OpenAI 兼容服务/)[0]).toBeVisible();

    fireEvent.click(screen.getByRole("tab", { name: "服务 / Provider" }));
    expect((await screen.findAllByText("http://127.0.0.1:34123/v1"))[0]).toBeVisible();
    fireEvent.click(screen.getByRole("tab", { name: "API 调试" }));
    expect(await screen.findByRole("heading", { name: "API 调试器" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: /服务健康/ }));
    fireEvent.click(screen.getByRole("button", { name: "运行端点" }));
    await waitFor(() => expect(runPluginDebugger).toHaveBeenCalledWith("dify2api", "health", {}));
    expect(await screen.findByText("HTTP 200")).toBeVisible();
    expect(screen.getByText(/"service": "dify2api"/)).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "插件开发" }));
    fireEvent.change(screen.getByLabelText("插件项目 ID"), { target: { value: "example-tool" } });
    fireEvent.change(screen.getByLabelText("插件项目名称"), { target: { value: "Example Tool" } });
    fireEvent.click(screen.getByRole("button", { name: "创建" }));
    await waitFor(() => expect(createPluginProject).toHaveBeenCalledWith("example-tool", "Example Tool", "bundle"));
    expect(await screen.findByText("Example Tool")).toBeVisible();
  });

  it("connects an Agent to a plugin Provider through an opaque host credential reference", async () => {
    await desktopGateway.setPluginEnabled("dify2api", true);
    await desktopGateway.startPlugin("dify2api");
    useAppStore.setState({ activeNavigation: "plugins" });
    render(<App />);

    expect(await screen.findByRole("heading", { name: "插件工作台" })).toBeVisible();
    fireEvent.click(screen.getByRole("tab", { name: "服务 / Provider" }));
    fireEvent.click(await screen.findByRole("button", { name: "用于 AI Agent" }));

    await waitFor(() => expect(useAppStore.getState().activeNavigation).toBe("agent"));
    expect(useAppStore.getState().agentProviderRef).toEqual({
      pluginId: "dify2api",
      providerId: "openai",
    });
    expect(useAppStore.getState().agentApiKey).toBe("");
    expect(await screen.findByText(/密钥不会进入页面/)).toBeVisible();
  });

  it("renders streamed Agent Markdown and supports editing or regenerating the latest turn", async () => {
    useAppStore.setState({ activeNavigation: "agent" });
    let streamListener: ((event: import("../domain/models").AgentStreamEvent) => void) | undefined;
    vi.spyOn(desktopGateway, "listenAgentStream").mockImplementation(async (_requestId, listener) => {
      streamListener = listener;
      return () => {};
    });
    let resolveFirst!: (value: Awaited<ReturnType<typeof desktopGateway.runAgentTurn>>) => void;
    const result = { message: "# 实时结果\n\nMarkdown 已完成。", tools: [], usage: { promptTokens: 30, completionTokens: 12 }, durationMs: 80, stopReason: "completed", rounds: 1, toolCalls: 0, retryCount: 0 };
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
      streamListener?.({ type: "contentReplace", content: "# 实时结果\n\n正在生成" });
    });

    expect(await screen.findByRole("heading", { name: "实时结果" })).toBeVisible();
    expect(screen.getByText("正在生成")).toBeVisible();
    await act(async () => { resolveFirst(result); });
    await waitFor(() => expect(screen.getByText("Markdown 已完成。")).toBeVisible());
    expect(runAgent).toHaveBeenCalledWith(expect.objectContaining({
      stream: true,
      contextWindow: 393216,
      maxOutputTokens: 98304,
      maxRounds: 96,
      temperature: 0.2,
      toolPolicy: expect.objectContaining({
        enabled: true,
        databaseRead: true,
        arbitraryFileRead: true,
      }),
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

  it("persists a failed Agent trajectory and continues without deleting the failed attempt", async () => {
    useAppStore.setState({ activeNavigation: "agent" });
    let streamListener: ((event: import("../domain/models").AgentStreamEvent) => void) | undefined;
    vi.spyOn(desktopGateway, "listenAgentStream").mockImplementation(async (_requestId, listener) => {
      streamListener = listener;
      return () => undefined;
    });
    const saveSession = vi.spyOn(desktopGateway, "saveAgentSession");
    const runAgent = vi.spyOn(desktopGateway, "runAgentTurn")
      .mockImplementationOnce(async () => {
        streamListener?.({ type: "roundStarted", round: 1 });
        streamListener?.({ type: "delta", content: "已读取 manifest，" });
        streamListener?.({
          type: "tool",
          tool: { callId: "read-1", name: "read_file", status: "completed", summary: "读取完成", output: "schema: 2", ordinal: 1, round: 1 },
        });
        streamListener?.({ type: "retrying", round: 2, attempt: 2, maxAttempts: 4, delayMs: 1_000, error: "HTTP 503" });
        throw new Error("upstream HTTP 503");
      })
      .mockResolvedValue({
        message: "已从失败处继续并完成。",
        tools: [],
        usage: { promptTokens: 20, completionTokens: 8 },
        durationMs: 20,
        stopReason: "completed",
        rounds: 1,
        toolCalls: 0,
        retryCount: 0,
      });
    render(<App />);

    const composer = await screen.findByPlaceholderText(/询问 RPAZ/);
    fireEvent.change(composer, { target: { value: "检查项目" } });
    fireEvent.click(screen.getByRole("button", { name: "发送消息" }));

    expect(await screen.findByText("已读取 manifest，")).toBeVisible();
    expect((await screen.findAllByText(/upstream HTTP 503/)).length).toBeGreaterThan(0);
    const continueButton = await screen.findByRole("button", { name: "从失败处继续" });
    await waitFor(() => expect(saveSession.mock.calls.some(([session]) => (
      session.messages.at(-1)?.run?.status === "failed"
      && session.messages.at(-1)?.tools?.[0]?.callId === "read-1"
      && session.messages.at(-1)?.run?.retries?.[0]?.attempt === 2
    ))).toBe(true));

    fireEvent.click(continueButton);
    expect(await screen.findByText("已从失败处继续并完成。")).toBeVisible();
    expect(screen.getByText("已读取 manifest，")).toBeVisible();
    expect(runAgent).toHaveBeenCalledTimes(2);
    expect(runAgent.mock.calls[1][0].messages).toEqual([
      expect.objectContaining({ role: "user", content: "检查项目" }),
    ]);
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
    fireEvent.contextMenu(screen.getByRole("tree", { name: "知识库目录" }));
    fireEvent.click(screen.getByRole("button", { name: "新建根目录" }));
    const rootDirectoryInput = screen.getByLabelText("新目录名称");
    fireEvent.change(rootDirectoryInput, { target: { value: "根级目录" } });
    fireEvent.keyDown(rootDirectoryInput, { key: "Enter" });
    await waitFor(() => expect(create).toHaveBeenCalledWith("根级目录", "directory"));
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
    expect(await screen.findByText("月度发票归档")).toBeVisible();
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

  it("keeps multiple Studio files in tabs and supports save and close shortcuts", async () => {
    const project = { id: "project-tabbed-editor", name: "多页签项目", files: ["main.py", "worker.py", "manifest.yaml"] };
    vi.spyOn(desktopGateway, "listStudioProjects").mockResolvedValue([project]);
    vi.spyOn(desktopGateway, "readProjectFile").mockImplementation(async (_projectId, path) => path === "worker.py" ? "def work():\n    return 1\n" : path === "manifest.yaml" ? "name: tabbed\n" : "def main(ctx):\n    return {}\n");
    const writeFile = vi.spyOn(desktopGateway, "writeProjectFile").mockResolvedValue();
    render(<StudioPage />);

    expect(await screen.findByRole("tab", { name: "main.py" })).toHaveAttribute("aria-selected", "true");
    fireEvent.click(screen.getByRole("button", { name: "worker.py" }));
    expect(await screen.findByRole("tab", { name: "worker.py" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "main.py" })).toBeVisible();

    fireEvent.change(screen.getByLabelText("mock-editor"), { target: { value: "def work():\n    return 2\n" } });
    fireEvent.keyDown(window, { key: "s", ctrlKey: true });
    await waitFor(() => expect(writeFile).toHaveBeenCalledWith(project.id, "worker.py", "def work():\n    return 2\n"));

    fireEvent.keyDown(window, { key: "w", ctrlKey: true });
    await waitFor(() => expect(screen.queryByRole("tab", { name: "worker.py" })).not.toBeInTheDocument());
    expect(screen.getByRole("tab", { name: "main.py" })).toHaveAttribute("aria-selected", "true");
  });

  it("opens Python Flow for the active Python buffer and applies the graph back to the editor", async () => {
    const project = { id: "project-python-flow", name: "Python Flow 项目", files: ["main.py"] };
    const source = "import rpa as r\n\ndef main(ctx):\n    r.url('https://example.com')\n    return {'ok': True}\n";
    vi.spyOn(desktopGateway, "listStudioProjects").mockResolvedValue([project]);
    vi.spyOn(desktopGateway, "readProjectFile").mockResolvedValue(source);
    const parseFlow = vi.spyOn(desktopGateway, "parsePythonFlow");
    const renderFlow = vi.spyOn(desktopGateway, "renderPythonFlow");
    const validateFlow = vi.spyOn(desktopGateway, "validatePythonFlow");
    render(<StudioPage />);

    expect(await screen.findByLabelText("mock-editor")).toHaveValue(source);
    const flowButton = screen.getByRole("button", { name: /Python Flow Beta/ });
    expect(within(screen.getByRole("tablist", { name: "已打开文件" })).queryByRole("button", { name: /Python Flow Beta/ })).not.toBeInTheDocument();
    expect(flowButton.closest(".studio-editor-head")).toBeInTheDocument();
    fireEvent.click(flowButton);

    expect(await screen.findByLabelText("Python Flow 可视化编辑器")).toBeVisible();
    expect(screen.getByText("节点目录")).toBeVisible();
    expect(parseFlow).toHaveBeenCalledWith(source, "main.py");

    fireEvent.click(screen.getByRole("button", { name: /代码同步/ }));
    fireEvent.click(screen.getByRole("button", { name: /流程图 → 代码/ }));

    await waitFor(() => expect(validateFlow).toHaveBeenCalledOnce());
    await waitFor(() => expect(renderFlow).toHaveBeenCalledOnce());
    fireEvent.click(screen.getByRole("button", { name: /^代码$/ }));
    expect(await screen.findByLabelText("mock-editor")).toHaveValue(source);
  });

  it("keeps an unsaved Studio editor mounted while navigating to another workspace", async () => {
    const project = { id: "project-navigation-state", name: "状态保留项目", files: ["main.py"] };
    useAppStore.setState({ activeNavigation: "studio" });
    vi.spyOn(desktopGateway, "listStudioProjects").mockResolvedValue([project]);
    const readFile = vi.spyOn(desktopGateway, "readProjectFile").mockResolvedValue("def main():\n    return 1\n");
    const writeFile = vi.spyOn(desktopGateway, "writeProjectFile").mockResolvedValue();
    render(<App />);

    const editor = await screen.findByLabelText("mock-editor");
    fireEvent.change(editor, { target: { value: "def main():\n    return 99\n" } });
    fireEvent.click(screen.getByRole("button", { name: "知识文档" }));
    expect(await screen.findByRole("heading", { name: "知识文档" })).toBeVisible();
    const studioSurface = document.querySelector<HTMLElement>('[data-navigation-page="studio"]');
    expect(studioSurface).toHaveAttribute("hidden");
    expect(studioSurface).not.toHaveAttribute("inert");
    fireEvent.keyDown(window, { key: "s", ctrlKey: true });
    await act(async () => Promise.resolve());
    expect(writeFile).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "开发工作室" }));
    expect(await screen.findByLabelText("mock-editor")).toHaveValue("def main():\n    return 99\n");
    expect(readFile).toHaveBeenCalledTimes(1);
  });

  it("releases inactive transient pages instead of retaining every visited workspace", async () => {
    useAppStore.setState({ activeNavigation: "runtimes" });
    const { container } = render(<App />);

    expect(await screen.findByRole("heading", { name: "运行环境" })).toBeVisible();
    expect(container.querySelector('[data-navigation-page="runtimes"]')).toBeInTheDocument();

    act(() => useAppStore.getState().setActiveNavigation("settings"));
    expect(await screen.findByRole("heading", { name: "设置" })).toBeVisible();
    expect(container.querySelector('[data-navigation-page="runtimes"]')).not.toBeInTheDocument();
  });

  it("enables the low-cost visual profile reported by the UOS launcher", async () => {
    vi.spyOn(desktopGateway, "getPlatformCapabilities").mockResolvedValue({
      os: "linux",
      displayName: "UOS 20 x86_64",
      runtimeTarget: "linux-x86_64",
      supportsWindowsUpdates: false,
      fileManagerName: "文件管理器",
      dataDirectoryPolicy: "XDG 本地数据目录",
      reducedVisualEffects: true,
    });

    render(<App />);

    await waitFor(() => expect(document.documentElement).toHaveAttribute("data-reduced-visual-effects", "true"));
  });

  it("initializes the local vault with a TOTP QR code and presents the recovery code once", async () => {
    const lockedStatus = {
      initialized: false,
      unlocked: false,
      itemCount: 0,
      failedAttempts: 0,
      retryAfterSeconds: 0,
      service: { running: false, port: 34131, endpoint: "http://127.0.0.1:34131/v1/vault", lastError: "" },
    };
    const unlockedStatus = { ...lockedStatus, initialized: true, unlocked: true, unlockedUntil: Math.floor(Date.now() / 1000) + 86_400 };
    vi.spyOn(desktopGateway, "getVaultStatus").mockResolvedValue(lockedStatus);
    vi.spyOn(desktopGateway, "beginVaultSetup").mockResolvedValue({
      setupId: "setup-test",
      account: "developer@local",
      issuer: "DRPA",
      manualKey: "JBSWY3DPEHPK3PXP",
      otpAuthUri: "otpauth://totp/DRPA:developer?secret=JBSWY3DPEHPK3PXP&issuer=DRPA",
      expiresAt: Math.floor(Date.now() / 1000) + 600,
    });
    vi.spyOn(desktopGateway, "completeVaultSetup").mockResolvedValue({
      status: unlockedStatus,
      serviceToken: "runtime-token",
      recoveryCode: "ABCD-EFGH-IJKL-MNOP-QRST-UVWX-YZ23-4567",
    });
    vi.spyOn(desktopGateway, "listVaultCredentials").mockResolvedValue([]);
    useAppStore.setState({ activeNavigation: "secrets" });
    render(<SecretsPage />);

    expect(await screen.findByRole("heading", { name: "开发凭据保险箱" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "生成初始化二维码" }));
    expect(await screen.findByAltText("Google Authenticator 初始化二维码")).toHaveAttribute("src", expect.stringContaining("data:image/png"));
    fireEvent.change(screen.getByLabelText("6 位验证码"), { target: { value: "123456" } });
    fireEvent.click(screen.getByRole("button", { name: "验证并创建保险箱" }));

    expect(await screen.findByRole("heading", { name: "保存恢复码" })).toBeVisible();
    expect(screen.getByText("ABCD-EFGH-IJKL-MNOP-QRST-UVWX-YZ23-4567")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "我已妥善保存" }));
    expect((await screen.findAllByText("全部项目"))[0]).toBeVisible();
  });

  it("keeps the new vault credential form mounted while typing", async () => {
    const unlockedStatus = {
      initialized: true,
      unlocked: true,
      unlockedUntil: Math.floor(Date.now() / 1000) + 86_400,
      itemCount: 0,
      failedAttempts: 0,
      retryAfterSeconds: 0,
      service: { running: false, port: 34131, endpoint: "http://127.0.0.1:34131/v1/vault", lastError: "" },
    };
    vi.spyOn(desktopGateway, "getVaultStatus").mockResolvedValue(unlockedStatus);
    vi.spyOn(desktopGateway, "listVaultCredentials").mockResolvedValue([]);
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);

    useAppStore.setState({ activeNavigation: "secrets" });
    render(<SecretsPage />);
    fireEvent.click(await screen.findByRole("button", { name: "新增凭据" }));
    fireEvent.change(screen.getByLabelText("名称"), { target: { value: "生产环境 API" } });
    fireEvent.change(screen.getByLabelText("账号 / 标识"), { target: { value: "automation@example.com" } });
    fireEvent.change(screen.getByLabelText("密码 / Secret"), { target: { value: "local-secret" } });
    fireEvent.change(screen.getByLabelText("网站 / 服务地址"), { target: { value: "https://api.example.com/v1" } });
    fireEvent.change(screen.getByLabelText("标签"), { target: { value: "开发, 本地" } });
    fireEvent.change(screen.getByLabelText("安全备注"), { target: { value: "仅供本机测试" } });

    expect(screen.getByLabelText("名称")).toHaveValue("生产环境 API");
    expect(screen.getByLabelText("账号 / 标识")).toHaveValue("automation@example.com");
    expect(screen.getByLabelText("密码 / Secret")).toHaveValue("local-secret");
    expect(screen.getByRole("heading", { name: "生产环境 API" })).toBeVisible();
    expect(consoleError).not.toHaveBeenCalled();
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
      stopReason: "completed",
      rounds: 1,
      toolCalls: 0,
      retryCount: 0,
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

  it("saves a Studio project directly into the local RPAZ package library", async () => {
    const project = { id: "project-000000000000000000000002", name: "本地发布测试", files: ["main.py"] };
    vi.spyOn(desktopGateway, "listStudioProjects").mockResolvedValue([project]);
    vi.spyOn(desktopGateway, "readProjectFile").mockResolvedValue("def main(ctx): return {'ok': True}\n");
    const install = vi.spyOn(desktopGateway, "installStudioProject").mockResolvedValue({
      id: "local.publish-test",
      name: "本地发布测试",
      description: "",
      version: "0.1.0",
      runtime: "Python 3.11",
      trust: "local",
      accent: "#4f6ef7",
      initials: "本地",
      parameters: [],
      profiles: [],
    });
    render(<StudioPage />);

    expect(await screen.findByText("本地发布测试")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "保存到 RPAZ 包" }));

    await waitFor(() => expect(install).toHaveBeenCalledWith(project.id));
    expect(await screen.findByText(/已保存到 RPAZ 包库/)).toBeVisible();
  });

  it("renames a Studio project from its context menu", async () => {
    const project = { id: "project-000000000000000000000003", name: "旧项目名", files: ["main.py", "manifest.yaml"] };
    vi.spyOn(desktopGateway, "listStudioProjects").mockResolvedValue([project]);
    vi.spyOn(desktopGateway, "readProjectFile").mockResolvedValue("def main(ctx): pass\n");
    const renameProject = vi.spyOn(desktopGateway, "renameStudioProject").mockResolvedValue({ ...project, name: "中文项目名" });
    render(<StudioPage />);

    const projectButton = await screen.findByRole("button", { name: /旧项目名/ });
    fireEvent.contextMenu(projectButton);
    fireEvent.click(screen.getByRole("button", { name: "重命名项目" }));
    const input = screen.getByLabelText("项目新名称");
    fireEvent.change(input, { target: { value: "中文项目名" } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => expect(renameProject).toHaveBeenCalledWith(project.id, "中文项目名"));
    expect(await screen.findByText(/项目已重命名为：中文项目名/)).toBeVisible();
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

  it("stops a running workbench task from the run bar", async () => {
    const initial = await desktopGateway.getWorkspaceSnapshot();
    useAppStore.setState({ snapshot: initial, selectedPackageId: "com.drpa.invoice-hub", selectedProfileId: "monthly" });
    vi.spyOn(desktopGateway, "startRun").mockResolvedValue("run-stop-test");
    vi.spyOn(desktopGateway, "getWorkspaceSnapshot").mockImplementation(() => new Promise(() => undefined));
    const cancelRun = vi.spyOn(desktopGateway, "cancelRun").mockResolvedValue();
    render(<WorkbenchPage />);

    fireEvent.click(screen.getByRole("button", { name: "运行任务" }));
    fireEvent.click(await screen.findByRole("button", { name: "停止任务" }));

    await waitFor(() => expect(cancelRun).toHaveBeenCalledWith("run-stop-test"));
  });

  it("stops a development run from the Studio header", async () => {
    const project = { id: "project-000000000000000000000001", name: "可停止项目", files: ["main.py"] };
    vi.spyOn(desktopGateway, "listStudioProjects").mockResolvedValue([project]);
    vi.spyOn(desktopGateway, "readProjectFile").mockResolvedValue("def main(ctx):\n    return {}\n");
    vi.spyOn(desktopGateway, "writeProjectFile").mockResolvedValue();
    vi.spyOn(desktopGateway, "runStudioProject").mockResolvedValue("studio-run-stop-test");
    const cancelRun = vi.spyOn(desktopGateway, "cancelRun").mockResolvedValue();
    render(<StudioPage />);

    expect(await screen.findByText("可停止项目")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "直接运行" }));
    fireEvent.click(await screen.findByRole("button", { name: "停止运行" }));

    await waitFor(() => expect(cancelRun).toHaveBeenCalledWith("studio-run-stop-test"));
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
