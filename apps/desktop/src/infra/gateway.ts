import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { mockSnapshot } from "../data/mockSnapshot";
import type {
  AgentTurnRequest,
  AgentTurnResult,
  AgentStreamEvent,
  AgentWorkspaceConfig,
  AgentSkillPackage,
  CurrentUser,
  DatabaseColumn,
  DatabaseInfo,
  DatabaseQueryResult,
  DatabaseTable,
  RemoteConnectionTest,
  RemoteDatabaseProfile,
  KnowledgeEntry,
  LocalDifyApp,
  LocalDifyAppMode,
  LocalDifyProvider,
  LocalDifyProviderInput,
  LocalDifyProviderTest,
  LocalDifyRunRequest,
  LocalDifyRunResult,
  LocalDifyRunSummary,
  LocalDifyServiceStatus,
  LocalDifyStreamEvent,
  LocalDifyWorkflowGraph,
  LocalDifyWorkflowNode,
  LocalDifyWorkflowValidationReport,
  DifyCompatibilityReport,
  PackageSummary,
  PlatformCapabilities,
  PluginLogLine,
  PluginConnectionTest,
  PluginProjectSummary,
  PluginSummary,
  RuntimeStatus,
  RunDetail,
  StudioCellResult,
  StudioCompletionResult,
  StudioInspectResult,
  StudioProject,
  WindowsUpdateSession,
  WindowsUpdateStatus,
  WorkspaceSnapshot,
} from "../domain/models";

export interface DesktopGateway {
  getWorkspaceSnapshot(): Promise<WorkspaceSnapshot>;
  reportUiReady(): Promise<void>;
  reportUiInputReady(): Promise<void>;
  installPackage(archivePath: string): Promise<PackageSummary>;
  uninstallPackage(packageId: string): Promise<void>;
  startRun(packageId: string, profileId: string, parameters: Record<string, unknown>): Promise<string>;
  cancelRun(runId: string): Promise<void>;
  getRunDetail(runId: string): Promise<RunDetail>;
  openRunOutputDirectory(runId: string): Promise<void>;
  listStudioProjects(): Promise<StudioProject[]>;
  createStudioProject(name: string): Promise<StudioProject>;
  openInstalledPackage(packageId: string): Promise<StudioProject>;
  readProjectFile(projectId: string, relativePath: string): Promise<string>;
  writeProjectFile(projectId: string, relativePath: string, content: string): Promise<void>;
  createProjectDirectory(projectId: string, relativePath: string): Promise<void>;
  renameProjectEntry(projectId: string, relativePath: string, targetPath: string): Promise<void>;
  deleteProjectEntry(projectId: string, relativePath: string): Promise<void>;
  deleteStudioProject(projectId: string): Promise<void>;
  importProjectFile(projectId: string, sourcePath: string, targetDirectory: string): Promise<string>;
  buildStudioProject(projectId: string): Promise<string>;
  openBuildOutputDirectory(): Promise<void>;
  runStudioProject(projectId: string, parameters: Record<string, unknown>): Promise<string>;
  executeStudioCell(projectId: string, code: string): Promise<StudioCellResult>;
  completeStudioPython(projectId: string, code: string, cursorPos: number): Promise<StudioCompletionResult>;
  inspectStudioPython(projectId: string, code: string, cursorPos: number, detailLevel: number): Promise<StudioInspectResult>;
  prepareStudioKernel(projectId: string): Promise<void>;
  restartStudioKernel(projectId: string): Promise<void>;
  getWorkspaceDatabaseInfo(): Promise<DatabaseInfo>;
  listDatabaseTables(): Promise<DatabaseTable[]>;
  describeDatabaseTable(tableName: string): Promise<DatabaseColumn[]>;
  executeDatabaseSql(sql: string): Promise<DatabaseQueryResult>;
  getDatabaseSchemaContext(): Promise<string>;
  openWorkspaceDatabaseDirectory(): Promise<void>;
  listRemoteDatabaseProfiles(): Promise<RemoteDatabaseProfile[]>;
  saveRemoteDatabaseProfile(profile: RemoteDatabaseProfile): Promise<RemoteDatabaseProfile>;
  deleteRemoteDatabaseProfile(profileId: string): Promise<void>;
  testRemoteDatabaseConnection(profile: RemoteDatabaseProfile, password: string): Promise<RemoteConnectionTest>;
  listRemoteDatabaseTables(profileId: string, password: string): Promise<DatabaseTable[]>;
  describeRemoteDatabaseTable(profileId: string, password: string, tableName: string): Promise<DatabaseColumn[]>;
  executeRemoteDatabaseSql(profileId: string, password: string, sql: string): Promise<DatabaseQueryResult>;
  getRemoteDatabaseSchemaContext(profileId: string, password: string): Promise<string>;
  listLocalDifyApps(): Promise<LocalDifyApp[]>;
  createLocalDifyApp(name: string, mode: LocalDifyAppMode): Promise<LocalDifyApp>;
  saveLocalDifyApp(app: LocalDifyApp): Promise<LocalDifyApp>;
  validateLocalDifyWorkflow(graph: LocalDifyWorkflowGraph, mode: LocalDifyAppMode): Promise<LocalDifyWorkflowValidationReport>;
  createLocalDifyWorkflowNode(kind: string, x: number, y: number): Promise<LocalDifyWorkflowNode>;
  deleteLocalDifyApp(appId: string): Promise<void>;
  listLocalDifyProviders(): Promise<LocalDifyProvider[]>;
  saveLocalDifyProvider(input: LocalDifyProviderInput): Promise<LocalDifyProvider>;
  deleteLocalDifyProvider(providerId: string): Promise<void>;
  testLocalDifyProvider(providerId: string): Promise<LocalDifyProviderTest>;
  runLocalDifyApp(request: LocalDifyRunRequest): Promise<LocalDifyRunResult>;
  listenLocalDifyStream(requestId: string, onEvent: (event: LocalDifyStreamEvent) => void): Promise<() => void>;
  listLocalDifyRuns(appId?: string, limit?: number): Promise<LocalDifyRunSummary[]>;
  publishLocalDifyApp(appId: string): Promise<LocalDifyApp>;
  getLocalDifyAppApiToken(appId: string): Promise<string>;
  checkLocalDifyCompatibility(appId: string): Promise<DifyCompatibilityReport>;
  importLocalDifyDsl(sourcePath: string): Promise<LocalDifyApp>;
  exportLocalDifyDsl(appId: string, targetPath: string): Promise<string>;
  getLocalDifyServiceStatus(): Promise<LocalDifyServiceStatus>;
  startLocalDifyService(port: number): Promise<LocalDifyServiceStatus>;
  stopLocalDifyService(): Promise<LocalDifyServiceStatus>;
  runAgentTurn(request: AgentTurnRequest): Promise<AgentTurnResult>;
  listenAgentStream(requestId: string, onEvent: (event: AgentStreamEvent) => void): Promise<() => void>;
  getRuntimeStatus(): Promise<RuntimeStatus>;
  getPlatformCapabilities(): Promise<PlatformCapabilities>;
  initializeRuntime(): Promise<RuntimeStatus>;
  repairRuntime(): Promise<RuntimeStatus>;
  applyWindowsUpdate(packagePath: string): Promise<WindowsUpdateSession>;
  getWindowsUpdateStatus(sessionId: string): Promise<WindowsUpdateStatus>;
  getLatestWindowsUpdateStatus(): Promise<WindowsUpdateStatus | null>;
  restartForWindowsUpdate(sessionId: string): Promise<void>;
  getDataDirectory(): Promise<string>;
  openWorkspaceDataDirectory(): Promise<void>;
  getCurrentUser(): Promise<CurrentUser>;
  getAgentWorkspaceConfig(): Promise<AgentWorkspaceConfig>;
  writeAgentWorkspaceDocument(document: "agents" | "memory", content: string): Promise<void>;
  readAgentSkill(name: string): Promise<string>;
  readAgentSkillPackage(name: string): Promise<AgentSkillPackage>;
  writeAgentSkill(name: string, content: string): Promise<void>;
  writeAgentSkillPackage(name: string, manifestYaml: string, instructionsMarkdown: string): Promise<void>;
  readAgentSkillFile(name: string, relativePath: string): Promise<string>;
  writeAgentSkillFile(name: string, relativePath: string, content: string): Promise<void>;
  createAgentSkillDirectory(name: string, relativePath: string): Promise<void>;
  renameAgentSkillPath(name: string, relativePath: string, newRelativePath: string): Promise<void>;
  deleteAgentSkillPath(name: string, relativePath: string): Promise<void>;
  deleteAgentSkill(name: string): Promise<void>;
  listPlugins(): Promise<PluginSummary[]>;
  installPlugin(packagePath: string): Promise<PluginSummary>;
  savePluginConfig(pluginId: string, config: Record<string, unknown>, autostart: boolean): Promise<void>;
  setPluginEnabled(pluginId: string, enabled: boolean): Promise<void>;
  startPlugin(pluginId: string): Promise<void>;
  stopPlugin(pluginId: string): Promise<void>;
  uninstallPlugin(pluginId: string): Promise<void>;
  getPluginLogs(pluginId: string): Promise<PluginLogLine[]>;
  testPluginConnection(pluginId: string): Promise<PluginConnectionTest>;
  listPluginProjects(): Promise<PluginProjectSummary[]>;
  createPluginProject(pluginId: string, name: string, projectType: "tool" | "service"): Promise<PluginProjectSummary>;
  validatePluginProject(pluginId: string): Promise<PluginProjectSummary>;
  buildPluginProject(pluginId: string): Promise<string>;
  listKnowledgeEntries(): Promise<KnowledgeEntry[]>;
  readKnowledgeFile(relativePath: string): Promise<string>;
  writeKnowledgeFile(relativePath: string, content: string): Promise<void>;
  createKnowledgeEntry(relativePath: string, kind: "file" | "directory"): Promise<void>;
  renameKnowledgeEntry(relativePath: string, targetPath: string): Promise<void>;
  deleteKnowledgeEntry(relativePath: string): Promise<void>;
  importKnowledgeFiles(sourcePaths: string[], targetDirectory: string): Promise<string[]>;
  exportKnowledgeFile(relativePath: string, targetPath: string): Promise<string>;
}

function isTauriHost(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

const mockStudioProjects: StudioProject[] = [];
const mockStudioContents = new Map<string, Map<string, string>>();
const mockKnowledgeDirectories = new Set(["RPAZ 开发指南"]);
const mockKnowledgeContents = new Map<string, string>([
  ["RPAZ 开发指南/00_阅读指南.md", "# RPAZ 开发指南\n\n这是浏览器预览知识库。桌面 Host 会提供完整的默认文档、导入、导出和本地文件管理。\n\n- [快速开始](./01_快速开始.md)\n- [Context](./03_ctx上下文与默认配置.md)\n"],
  ["RPAZ 开发指南/01_快速开始.md", "# 快速开始\n\n在开发工作室新建项目，编辑 `manifest.yaml` 和 `main.py`，直接运行后再导出 `.rpaz`。\n"],
  ["RPAZ 开发指南/03_ctx上下文与默认配置.md", "# ctx 上下文\n\n入口函数使用 `def main(ctx)`。通过 `ctx.params`、`ctx.log`、`ctx.progress()`、`ctx.output_file()` 和 `ctx.browser()` 访问 Host 能力。\n"],
]);
let mockAgentAgentsMarkdown = "# DRPA Agent 工作约定\n\n- 修改项目后运行 `rpaz_validate`。\n";
let mockAgentMemoryMarkdown = "# Agent Memory\n\n记录稳定事实与偏好。\n";
let mockRemoteDatabaseProfiles: RemoteDatabaseProfile[] = [];
let mockLocalDifyProviders: LocalDifyProvider[] = [{
  id: "provider-browser-preview",
  name: "OpenAI 兼容 Provider",
  baseUrl: "http://127.0.0.1:34121/v1",
  model: "dify-app",
  contextWindow: 128000,
  maxOutputTokens: 4096,
  temperature: 0.2,
  streaming: true,
  supportsTools: true,
  supportsJson: true,
  supportsVision: false,
  timeoutSeconds: 120,
  customHeaders: {},
  difyProvider: "langgenius/openai/openai",
  difyModel: "gpt-4o-mini",
  hasApiKey: false,
  updatedAt: Date.now(),
}];
let mockLocalDifyApps: LocalDifyApp[] = [{
  schema: 2,
  id: "app-browser-preview",
  name: "本地 Dify 调试应用",
  description: "通过 OpenAI 兼容 Provider 调试提示词并导出 Dify DSL。",
  mode: "chat",
  providerId: "provider-browser-preview",
  systemPrompt: "你是 DRPA Local Dify 中的开发助手。回答应准确、简洁，并说明关键步骤。",
  openingStatement: "你好，这是一个本地 Dify 调试应用。",
  inputKey: "query",
  temperature: 0.2,
  maxOutputTokens: 4096,
  workflow: { schema: 1, viewport: { x: 80, y: 120, zoom: 1 }, nodes: [], edges: [] },
  publishedVersion: 1,
  apiEnabled: true,
  createdAt: Date.now() - 3600000,
  updatedAt: Date.now(),
}];
let mockLocalDifyRuns: LocalDifyRunSummary[] = [];
let mockLocalDifyService: LocalDifyServiceStatus = {
  running: false,
  port: 34130,
  endpoint: "http://127.0.0.1:34130/v1",
  lastError: "",
};
const mockLocalDifyStreamListeners = new Map<string, (event: LocalDifyStreamEvent) => void>();

function mockWorkflowNode(kind: string, x: number, y: number): LocalDifyWorkflowNode {
  const id = `node-${Date.now()}-${Math.random().toString(16).slice(2)}`;
  const templates: Record<string, { title: string; height: number; config: Record<string, unknown> }> = {
    start: { title: "开始", height: 84, config: { variables: [{ label: "query", variable: "query", type: "paragraph", required: true }] } },
    llm: { title: "LLM", height: 104, config: { prompt_template: [{ role: "user", text: "{{#start.query#}}" }], model: { provider: "", name: "", mode: "chat", completion_params: {} } } },
    "template-transform": { title: "模板转换", height: 96, config: { template: "{{ input }}", variables: [] } },
    "if-else": { title: "条件分支", height: 112, config: { cases: [{ case_id: "true", logical_operator: "and", conditions: [{ variable_selector: ["start", "query"], comparison_operator: "contains", value: "" }] }] } },
    "http-request": { title: "HTTP 请求", height: 108, config: { method: "get", url: "https://example.com", headers: "", body: { type: "none", data: [] } } },
    code: { title: "代码执行", height: 112, config: { code_language: "python3", code: "def main(input: str):\n    return {'result': input}\n", variables: [], outputs: { result: { type: "string" } } } },
    answer: { title: "直接回复", height: 84, config: { answer: "{{#llm.text#}}" } },
    end: { title: "结束", height: 84, config: { outputs: [{ variable: "answer", value_selector: ["llm", "text"] }] } },
  };
  const template = templates[kind] ?? templates.end!;
  return { id, kind, title: template.title, x, y, width: 220, height: template.height, config: structuredClone(template.config) };
}

function mockWorkflowGraph(mode: LocalDifyAppMode, inputKey = "query"): LocalDifyWorkflowGraph {
  if (mode !== "workflow" && mode !== "advanced-chat") return { schema: 1, viewport: { x: 80, y: 120, zoom: 1 }, nodes: [], edges: [] };
  const start = { ...mockWorkflowNode("start", 80, 210), id: "start", config: { variables: [{ label: inputKey, variable: inputKey, type: "paragraph", required: true }] } };
  const llm = { ...mockWorkflowNode("llm", 390, 210), id: "llm", config: { prompt_template: [{ role: "user", text: `{{#start.${inputKey}#}}` }], model: { provider: "", name: "", mode: "chat", completion_params: {} } } };
  const terminalKind = mode === "advanced-chat" ? "answer" : "end";
  const terminal = { ...mockWorkflowNode(terminalKind, 700, 210), id: terminalKind };
  return {
    schema: 1,
    viewport: { x: 80, y: 120, zoom: 1 },
    nodes: [start, llm, terminal],
    edges: [
      { id: "edge-start-llm", source: "start", target: "llm", sourceHandle: "source", targetHandle: "target", label: "", data: {} },
      { id: `edge-llm-${terminalKind}`, source: "llm", target: terminalKind, sourceHandle: "source", targetHandle: "target", label: "", data: {} },
    ],
  };
}
const mockAgentSkills = new Map<string, string>([[
  "rpaz-development",
  "---\nname: rpaz-development\ndescription: 创建、修改、校验或构建 RPAZ 脚本包时使用。\n---\n\n# RPAZ Development\n",
]]);
const mockAgentSkillManifests = new Map<string, string>([[
  "rpaz-development",
  "schema: 2\nid: rpaz-development\nname: RPAZ Development\nversion: 2.0.0\ndescription: 创建、修改、校验或构建 RPAZ 脚本包时使用。\ntools: []\nlibraries: []\n",
]]);
const mockAgentSkillFiles = new Map<string, string>();
const mockAgentSkillDirectories = new Set<string>();
let mockPluginProjects: PluginProjectSummary[] = [];
let mockPluginRunning = false;
let mockPluginEnabled = false;
let mockPluginConfig: Record<string, unknown> = {
  base_url: "http://127.0.0.1:5001/v1",
  api_key: "",
  app_type: "chat",
  input_key: "query",
  model: "dify-app",
  port: 34121,
  tool_bridge: true,
};

function mockPlugins(): PluginSummary[] {
  return [{
    id: "dify-loves-hermes",
    name: "Dify Loves Hermes",
    version: "0.3.0",
    description: "将本地或远程 Dify App API 转换为 OpenAI 兼容接口，并补充工具调用与 Provider 链路追踪。",
    types: ["provider-adapter", "service"],
    enabled: mockPluginEnabled,
    autostart: false,
    status: mockPluginRunning ? "running" : mockPluginEnabled ? "stopped" : "disabled",
    endpoint: `http://127.0.0.1:${Number(mockPluginConfig.port ?? 34121)}/v1`,
    toolCount: 0,
    config: mockPluginConfig,
    configSchema: { type: "object", properties: {} },
    directory: "浏览器预览数据/plugins/dify-loves-hermes",
    lastError: "",
  }];
}
const mockAgentStreamListeners = new Map<string, (event: AgentStreamEvent) => void>();

function mockAgentWorkspaceConfig(): AgentWorkspaceConfig {
  return {
    agentsMarkdown: mockAgentAgentsMarkdown,
    memoryMarkdown: mockAgentMemoryMarkdown,
    skills: Array.from(mockAgentSkills, ([name, content]) => ({
      name,
      displayName: name,
      version: "2.0.0",
      description: content.match(/^description:\s*(.+)$/m)?.[1] ?? "未填写描述",
      format: "skill-v2",
      toolCount: 0,
      libraryCount: 0,
      modifiedAt: Date.now(),
    })),
    rootDirectory: "浏览器预览数据/agent",
  };
}

function mockKnowledgeEntries(): KnowledgeEntry[] {
  const now = Date.now();
  return [
    ...Array.from(mockKnowledgeDirectories, (path): KnowledgeEntry => ({ path, name: path.split("/").at(-1) ?? path, kind: "directory", size: 0, modifiedAt: now })),
    ...Array.from(mockKnowledgeContents, ([path, content]): KnowledgeEntry => ({ path, name: path.split("/").at(-1) ?? path, kind: "file", size: new Blob([content]).size, modifiedAt: now })),
  ].sort((left, right) => left.path.localeCompare(right.path, "zh-CN"));
}

function addMockProject(project: StudioProject): StudioProject {
  const existing = mockStudioProjects.find((item) => item.id === project.id);
  if (!existing) mockStudioProjects.push(project);
  if (!mockStudioContents.has(project.id)) {
    mockStudioContents.set(project.id, new Map([
      ["manifest.yaml", "schema: 2\nid: local.browser-preview\nname: Browser Preview\nversion: 0.1.0\nentrypoint:\n  runtime: python\n  module: main.py\n  callable: main\n"],
      ["main.py", "def main(ctx):\n    ctx.log.info('你好，DRPA')\n"],
      ["notebook.ipynb", '{"cells":[],"metadata":{},"nbformat":4,"nbformat_minor":5}\n'],
    ]));
  }
  return structuredClone(existing ?? project);
}

const mockGateway: DesktopGateway = {
  async getWorkspaceSnapshot() {
    return structuredClone(mockSnapshot);
  },
  async reportUiReady() {},
  async reportUiInputReady() {},
  async installPackage() {
    throw new Error("浏览器预览模式不能读取本地 rpaz，请在桌面应用中测试安装。 ");
  },
  async uninstallPackage() {},
  async startRun() {
    await new Promise((resolve) => window.setTimeout(resolve, 260));
    return `run-${Date.now()}`;
  },
  async cancelRun() {
    await new Promise((resolve) => window.setTimeout(resolve, 180));
  },
  async getRunDetail(runId) {
    const summary = mockSnapshot.runs.find((run) => run.id === runId);
    if (!summary) throw new Error("运行记录不存在");
    return {
      summary: structuredClone(summary),
      parameters: { source: "mock", limit: 20 },
      outputDir: `C:\\DRPA\\data\\runs\\${runId}\\outputs`,
      events: mockSnapshot.logs.map((entry) => ({
        id: entry.id,
        runId,
        recordedAt: `2026-07-18T${entry.time}Z`,
        eventType: "log",
        level: entry.level,
        scope: entry.scope,
        message: entry.message,
        payload: {},
      })),
      artifacts: [],
    };
  },
  async openRunOutputDirectory() {},
  async listStudioProjects() {
    return structuredClone(mockStudioProjects);
  },
  async createStudioProject(name) {
    return addMockProject({ id: `project-${Date.now().toString(16).padStart(24, "0").slice(-24)}`, name, files: ["main.py", "manifest.yaml", "notebook.ipynb"] });
  },
  async openInstalledPackage(packageId) {
    const item = mockSnapshot.packages.find((candidate) => candidate.id === packageId);
    return addMockProject({ id: `project-${Date.now().toString(16).padStart(24, "0").slice(-24)}`, name: item?.name ?? packageId, files: ["main.py", "manifest.yaml", "notebook.ipynb"] });
  },
  async readProjectFile(projectId, relativePath) {
    return mockStudioContents.get(projectId)?.get(relativePath) ?? "";
  },
  async writeProjectFile(projectId, relativePath, content) {
    const project = mockStudioProjects.find((item) => item.id === projectId);
    if (project && !project.files.includes(relativePath)) project.files.push(relativePath);
    mockStudioContents.get(projectId)?.set(relativePath, content);
  },
  async createProjectDirectory(projectId, relativePath) {
    const project = mockStudioProjects.find((item) => item.id === projectId);
    const directory = `${relativePath.replace(/\/$/, "")}/`;
    if (project && !project.files.includes(directory)) project.files.push(directory);
  },
  async renameProjectEntry(projectId, relativePath, targetPath) {
    const project = mockStudioProjects.find((item) => item.id === projectId);
    if (!project) return;
    const source = relativePath.replace(/\/$/, "");
    project.files = project.files.map((path) => {
      const normalized = path.replace(/\/$/, "");
      return normalized === source || normalized.startsWith(`${source}/`)
        ? `${targetPath}${normalized.slice(source.length)}${path.endsWith("/") ? "/" : ""}`
        : path;
    });
    const contents = mockStudioContents.get(projectId);
    if (contents?.has(source)) {
      contents.set(targetPath, contents.get(source) ?? "");
      contents.delete(source);
    }
  },
  async deleteProjectEntry(projectId, relativePath) {
    const project = mockStudioProjects.find((item) => item.id === projectId);
    if (!project) return;
    const source = relativePath.replace(/\/$/, "");
    project.files = project.files.filter((path) => !path.replace(/\/$/, "").startsWith(source));
    mockStudioContents.get(projectId)?.delete(source);
  },
  async deleteStudioProject(projectId) {
    const index = mockStudioProjects.findIndex((item) => item.id === projectId);
    if (index >= 0) mockStudioProjects.splice(index, 1);
    mockStudioContents.delete(projectId);
  },
  async importProjectFile(_projectId, sourcePath, targetDirectory) {
    const fileName = sourcePath.split(/[\\/]/).pop() ?? "imported.file";
    return targetDirectory ? `${targetDirectory}/${fileName}` : fileName;
  },
  async buildStudioProject(projectId) {
    return `${projectId}.rpaz`;
  },
  async openBuildOutputDirectory() {},
  async runStudioProject() {
    return `run-${Date.now()}`;
  },
  async executeStudioCell(_projectId, code) {
    return { executionCount: 1, stdout: "", stderr: "", result: `预览模式：${code.length} 个字符`, traceback: [], outputs: [], variables: [], durationMs: 1 };
  },
  async completeStudioPython(_projectId, code, cursorPos) {
    const start = code.slice(0, cursorPos).search(/[A-Za-z_][A-Za-z0-9_]*$/);
    return { matches: ["ctx", "print", "range"], cursorStart: start < 0 ? cursorPos : start, cursorEnd: cursorPos, metadata: {}, status: "ok" };
  },
  async inspectStudioPython() {
    return { found: true, data: { "text/plain": "Signature: ctx.progress(value: int, message: str = '')\n\n更新当前任务的进度。" }, metadata: {}, status: "ok" };
  },
  async prepareStudioKernel() {},
  async restartStudioKernel() {},
  async getWorkspaceDatabaseInfo() {
    return { name: "工作区数据库", engine: "SQLite", path: "data/databases/workspace.sqlite3", sizeBytes: 24_576 };
  },
  async listDatabaseTables() {
    return [{ name: "example_tasks", kind: "table", rowCount: 2 }];
  },
  async describeDatabaseTable() {
    return [
      { ordinal: 0, name: "id", dataType: "INTEGER", notNull: false, primaryKey: true },
      { ordinal: 1, name: "name", dataType: "TEXT", notNull: true, primaryKey: false },
    ];
  },
  async executeDatabaseSql(sql) {
    return { columns: ["preview", "characters"], rows: [["浏览器预览", sql.length]], affectedRows: 0, durationMs: 1, truncated: false, statementType: "SELECT" };
  },
  async getDatabaseSchemaContext() {
    return "-- SQLite 工作区数据库结构\n\nCREATE TABLE example_tasks (id INTEGER PRIMARY KEY, title TEXT NOT NULL, status TEXT);\n";
  },
  async openWorkspaceDatabaseDirectory() {},
  async listRemoteDatabaseProfiles() { return mockRemoteDatabaseProfiles; },
  async saveRemoteDatabaseProfile(profile) {
    const saved = { ...profile, id: profile.id || `database-${Date.now()}` };
    mockRemoteDatabaseProfiles = [...mockRemoteDatabaseProfiles.filter((item) => item.id !== saved.id), saved];
    return saved;
  },
  async deleteRemoteDatabaseProfile(profileId) {
    mockRemoteDatabaseProfiles = mockRemoteDatabaseProfiles.filter((item) => item.id !== profileId);
  },
  async testRemoteDatabaseConnection(profile) {
    return { serverVersion: profile.engine === "postgresql" ? "PostgreSQL 17.2" : "MySQL 8.4", latencyMs: 12 };
  },
  async listRemoteDatabaseTables() {
    return [{ name: "public.remote_tasks", kind: "table", rowCount: 4 }];
  },
  async describeRemoteDatabaseTable() {
    return [{ ordinal: 0, name: "id", dataType: "bigint", notNull: true, primaryKey: false }];
  },
  async executeRemoteDatabaseSql(_profileId, _password, sql) {
    return { columns: ["remote", "characters"], rows: [[true, sql.length]], affectedRows: 0, durationMs: 12, truncated: false, statementType: "SELECT" };
  },
  async getRemoteDatabaseSchemaContext() {
    return "-- PostgreSQL 数据库结构\n\nCREATE TABLE public.remote_tasks (id bigint NOT NULL);\n";
  },
  async listLocalDifyApps() {
    return structuredClone(mockLocalDifyApps);
  },
  async createLocalDifyApp(name, mode) {
    const now = Date.now();
    const app: LocalDifyApp = {
      schema: 2,
      id: `app-${now}`,
      name,
      description: "用于本地测试与 Dify DSL 导出的 AI 应用。",
      mode,
      providerId: mockLocalDifyProviders[0]?.id ?? "",
      systemPrompt: "你是一个准确、简洁的 AI 助手。",
      openingStatement: "你好，我是本地 AI 应用。",
      inputKey: "query",
      temperature: 0.2,
      maxOutputTokens: 4096,
      workflow: mockWorkflowGraph(mode),
      publishedVersion: 0,
      apiEnabled: false,
      createdAt: now,
      updatedAt: now,
    };
    mockLocalDifyApps = [app, ...mockLocalDifyApps];
    return structuredClone(app);
  },
  async saveLocalDifyApp(app) {
    const saved = { ...app, updatedAt: Date.now() };
    mockLocalDifyApps = mockLocalDifyApps.map((item) => item.id === saved.id ? saved : item);
    return structuredClone(saved);
  },
  async validateLocalDifyWorkflow(graph, mode) {
    const issues: LocalDifyWorkflowValidationReport["issues"] = [];
    const starts = graph.nodes.filter((node) => node.kind === "start");
    if (starts.length !== 1) issues.push({ level: "error", code: "start-count", message: "工作流需要且只允许一个开始节点" });
    const terminal = mode === "advanced-chat" ? "answer" : "end";
    if (!graph.nodes.some((node) => node.kind === terminal)) issues.push({ level: "error", code: "terminal-missing", message: `当前模式至少需要一个 ${terminal} 节点` });
    for (const edge of graph.edges) {
      if (!graph.nodes.some((node) => node.id === edge.source) || !graph.nodes.some((node) => node.id === edge.target)) {
        issues.push({ level: "error", code: "dangling-edge", message: "连线引用了不存在的节点", edgeId: edge.id });
      }
    }
    return { valid: !issues.some((issue) => issue.level === "error"), nodeCount: graph.nodes.length, edgeCount: graph.edges.length, issues };
  },
  async createLocalDifyWorkflowNode(kind, x, y) {
    return structuredClone(mockWorkflowNode(kind, x, y));
  },
  async deleteLocalDifyApp(appId) {
    mockLocalDifyApps = mockLocalDifyApps.filter((item) => item.id !== appId);
  },
  async listLocalDifyProviders() {
    return structuredClone(mockLocalDifyProviders);
  },
  async saveLocalDifyProvider(input) {
    const provider: LocalDifyProvider = {
      ...input,
      id: input.id || `provider-${Date.now()}`,
      hasApiKey: input.apiKey.length > 0 || mockLocalDifyProviders.some((item) => item.id === input.id && item.hasApiKey),
      updatedAt: Date.now(),
    };
    const { apiKey: _apiKey, ...saved } = provider as LocalDifyProvider & { apiKey?: string };
    mockLocalDifyProviders = [...mockLocalDifyProviders.filter((item) => item.id !== saved.id), saved];
    return structuredClone(saved);
  },
  async deleteLocalDifyProvider(providerId) {
    mockLocalDifyProviders = mockLocalDifyProviders.filter((item) => item.id !== providerId);
  },
  async testLocalDifyProvider(providerId) {
    const provider = mockLocalDifyProviders.find((item) => item.id === providerId);
    if (!provider) throw new Error("Provider 不存在");
    await new Promise((resolve) => window.setTimeout(resolve, 120));
    return { ok: true, message: "Provider 连接成功：OK", model: provider.model, durationMs: 42 };
  },
  async runLocalDifyApp(request) {
    const app = mockLocalDifyApps.find((item) => item.id === request.appId);
    const provider = mockLocalDifyProviders.find((item) => item.id === app?.providerId);
    if (!app || !provider) throw new Error("应用或 Provider 不存在");
    const answer = `## Local Dify 调试结果\n\n已通过 **${provider.name}** 处理：${request.query}`;
    const runId = `dify-run-${Date.now()}`;
    const listener = mockLocalDifyStreamListeners.get(request.requestId);
    listener?.({ type: "started", runId });
    if (request.stream) {
      for (const chunk of answer.match(/.{1,14}/gs) ?? [answer]) {
        await new Promise((resolve) => window.setTimeout(resolve, 12));
        listener?.({ type: "delta", content: chunk });
      }
    } else {
      await new Promise((resolve) => window.setTimeout(resolve, 160));
    }
    listener?.({ type: "completed", runId });
    const result: LocalDifyRunResult = {
      runId,
      appId: app.id,
      answer,
      conversationId: request.conversationId || `conversation-${Date.now()}`,
      providerId: provider.id,
      model: provider.model,
      usage: { promptTokens: 18, completionTokens: 28, totalTokens: 46 },
      durationMs: 164,
    };
    mockLocalDifyRuns = [{
      id: runId,
      appId: app.id,
      appName: app.name,
      status: "success",
      query: request.query,
      answer,
      providerId: provider.id,
      model: provider.model,
      promptTokens: 18,
      completionTokens: 28,
      durationMs: 164,
      error: "",
      createdAt: Date.now(),
    }, ...mockLocalDifyRuns];
    return result;
  },
  async listenLocalDifyStream(requestId, onEvent) {
    mockLocalDifyStreamListeners.set(requestId, onEvent);
    return () => { mockLocalDifyStreamListeners.delete(requestId); };
  },
  async listLocalDifyRuns(appId, limit = 100) {
    return structuredClone(mockLocalDifyRuns.filter((item) => !appId || item.appId === appId).slice(0, limit));
  },
  async publishLocalDifyApp(appId) {
    const app = mockLocalDifyApps.find((item) => item.id === appId);
    if (!app) throw new Error("应用不存在");
    return this.saveLocalDifyApp({ ...app, apiEnabled: true, publishedVersion: app.publishedVersion + 1 });
  },
  async getLocalDifyAppApiToken() {
    return "app-browser-preview-token";
  },
  async checkLocalDifyCompatibility(appId) {
    const app = mockLocalDifyApps.find((item) => item.id === appId);
    return {
      compatible: Boolean(app?.providerId),
      targetVersion: "0.3.1",
      issues: app?.providerId ? [] : [{ level: "error", code: "provider-missing", message: "应用尚未选择 Provider。" }],
    };
  },
  async importLocalDifyDsl() {
    return this.createLocalDifyApp("导入的 Dify 应用", "chat");
  },
  async exportLocalDifyDsl(appId, targetPath) {
    return targetPath || `${appId}.yml`;
  },
  async getLocalDifyServiceStatus() {
    return structuredClone(mockLocalDifyService);
  },
  async startLocalDifyService(port) {
    mockLocalDifyService = { running: true, port, endpoint: `http://127.0.0.1:${port}/v1`, startedAt: Date.now(), lastError: "" };
    return structuredClone(mockLocalDifyService);
  },
  async stopLocalDifyService() {
    mockLocalDifyService = { ...mockLocalDifyService, running: false, startedAt: undefined };
    return structuredClone(mockLocalDifyService);
  },
  async runAgentTurn(request) {
    const prompt = request.messages.at(-1)?.content ?? "";
    const message = request.mode === "sql"
      ? "已根据当前结构生成查询。\n\n```sql\nSELECT id, title, status FROM example_tasks LIMIT 100;\n```"
      : request.projectId
      ? `已连接浏览器预览 Agent。当前问题：${prompt}`
      : `已收到问题：${prompt}\n选择一个开发项目后可启用 RPAZ 工具。`;
    const listener = mockAgentStreamListeners.get(request.requestId);
    if (request.stream && listener) {
      listener({ type: "roundStarted", round: 1 });
      for (const chunk of message.match(/.{1,12}/gs) ?? [message]) {
        await new Promise((resolve) => window.setTimeout(resolve, 18));
        listener({ type: "delta", content: chunk });
      }
    } else {
      await new Promise((resolve) => window.setTimeout(resolve, 220));
    }
    return {
      message,
      tools: [],
      usage: { promptTokens: 18, completionTokens: 24 },
      durationMs: 220,
    };
  },
  async listenAgentStream(requestId, onEvent) {
    mockAgentStreamListeners.set(requestId, onEvent);
    return () => { mockAgentStreamListeners.delete(requestId); };
  },
  async getRuntimeStatus() {
    return { state: "ready", bundleVersion: "浏览器预览", pythonVersion: "3.11.9", runtimeRoot: "内存预览", environmentRoot: "内存预览", browserExecutable: "内存预览", message: "浏览器预览使用模拟运行环境" };
  },
  async getPlatformCapabilities() {
    return {
      os: "windows",
      displayName: "Windows x64",
      runtimeTarget: "windows-x86_64",
      supportsWindowsUpdates: true,
      fileManagerName: "资源管理器",
      dataDirectoryPolicy: "安装目录 data",
    };
  },
  async initializeRuntime() { return this.getRuntimeStatus(); },
  async repairRuntime() { return this.getRuntimeStatus(); },
  async applyWindowsUpdate() { throw new Error("浏览器预览不能应用 Windows 更新包"); },
  async getWindowsUpdateStatus() { throw new Error("浏览器预览没有更新会话"); },
  async getLatestWindowsUpdateStatus() { return null; },
  async restartForWindowsUpdate() {},
  async getDataDirectory() {
    return "浏览器预览数据（内存）";
  },
  async openWorkspaceDataDirectory() {},
  async getCurrentUser() {
    return { displayName: "本地用户", accountName: "browser-preview", initials: "本地" };
  },
  async listKnowledgeEntries() {
    return structuredClone(mockKnowledgeEntries());
  },
  async readKnowledgeFile(relativePath) {
    const content = mockKnowledgeContents.get(relativePath);
    if (content === undefined) throw new Error("知识文档不存在");
    return content;
  },
  async writeKnowledgeFile(relativePath, content) {
    mockKnowledgeContents.set(relativePath, content);
  },
  async createKnowledgeEntry(relativePath, kind) {
    if (mockKnowledgeContents.has(relativePath) || mockKnowledgeDirectories.has(relativePath)) throw new Error("同名条目已存在");
    if (kind === "directory") mockKnowledgeDirectories.add(relativePath);
    else mockKnowledgeContents.set(relativePath, "# 新文档\n\n在这里开始记录。\n");
  },
  async renameKnowledgeEntry(relativePath, targetPath) {
    if (mockKnowledgeContents.has(relativePath)) {
      mockKnowledgeContents.set(targetPath, mockKnowledgeContents.get(relativePath) ?? "");
      mockKnowledgeContents.delete(relativePath);
      return;
    }
    const affectedDirectories = Array.from(mockKnowledgeDirectories).filter((path) => path === relativePath || path.startsWith(`${relativePath}/`));
    const affectedFiles = Array.from(mockKnowledgeContents).filter(([path]) => path.startsWith(`${relativePath}/`));
    affectedDirectories.forEach((path) => { mockKnowledgeDirectories.delete(path); mockKnowledgeDirectories.add(`${targetPath}${path.slice(relativePath.length)}`); });
    affectedFiles.forEach(([path, content]) => { mockKnowledgeContents.delete(path); mockKnowledgeContents.set(`${targetPath}${path.slice(relativePath.length)}`, content); });
  },
  async deleteKnowledgeEntry(relativePath) {
    mockKnowledgeContents.delete(relativePath);
    Array.from(mockKnowledgeContents.keys()).filter((path) => path.startsWith(`${relativePath}/`)).forEach((path) => mockKnowledgeContents.delete(path));
    Array.from(mockKnowledgeDirectories).filter((path) => path === relativePath || path.startsWith(`${relativePath}/`)).forEach((path) => mockKnowledgeDirectories.delete(path));
  },
  async importKnowledgeFiles(sourcePaths, targetDirectory) {
    return sourcePaths.map((path) => `${targetDirectory ? `${targetDirectory}/` : ""}${path.split(/[\\/]/).at(-1) ?? "imported.md"}`);
  },
  async exportKnowledgeFile(_relativePath, targetPath) {
    return targetPath;
  },
  async getAgentWorkspaceConfig() {
    return mockAgentWorkspaceConfig();
  },
  async writeAgentWorkspaceDocument(document, content) {
    if (document === "agents") mockAgentAgentsMarkdown = content;
    else mockAgentMemoryMarkdown = content;
  },
  async readAgentSkill(name) {
    const content = mockAgentSkills.get(name);
    if (!content) throw new Error("Skill 不存在");
    return content;
  },
  async readAgentSkillPackage(name) {
    const instructionsMarkdown = mockAgentSkills.get(name);
    if (!instructionsMarkdown) throw new Error("Skill 不存在");
    const extraFiles = [...mockAgentSkillFiles.keys()]
      .filter((key) => key.startsWith(`${name}:`))
      .map((key) => key.slice(name.length + 1));
    const directories = [...mockAgentSkillDirectories]
      .filter((key) => key.startsWith(`${name}:`))
      .map((key) => key.slice(name.length + 1));
    const files = ["skill.yaml", "instructions.md", ...extraFiles];
    return {
      name,
      manifestYaml: mockAgentSkillManifests.get(name) ?? `schema: 2\nid: ${name}\nname: ${name}\nversion: 1.0.0\ndescription: Skill\ntools: []\nlibraries: []\n`,
      instructionsMarkdown,
      files,
      entries: [
        ...directories.map((path) => ({ path, kind: "directory" as const })),
        ...files.map((path) => ({ path, kind: "file" as const })),
      ],
    };
  },
  async writeAgentSkill(name, content) {
    mockAgentSkills.set(name, content);
  },
  async writeAgentSkillPackage(name, manifestYaml, instructionsMarkdown) {
    mockAgentSkillManifests.set(name, manifestYaml);
    mockAgentSkills.set(name, instructionsMarkdown);
  },
  async readAgentSkillFile(name, relativePath) {
    if (relativePath === "skill.yaml") return mockAgentSkillManifests.get(name) ?? "";
    if (relativePath === "instructions.md") return mockAgentSkills.get(name) ?? "";
    return mockAgentSkillFiles.get(`${name}:${relativePath}`) ?? "";
  },
  async writeAgentSkillFile(name, relativePath, content) {
    if (relativePath === "skill.yaml") mockAgentSkillManifests.set(name, content);
    if (relativePath === "instructions.md") mockAgentSkills.set(name, content);
    if (!['skill.yaml', 'instructions.md'].includes(relativePath)) mockAgentSkillFiles.set(`${name}:${relativePath}`, content);
  },
  async createAgentSkillDirectory(name, relativePath) {
    mockAgentSkillDirectories.add(`${name}:${relativePath}`);
  },
  async renameAgentSkillPath(name, relativePath, newRelativePath) {
    const fileKey = `${name}:${relativePath}`;
    if (mockAgentSkillFiles.has(fileKey)) {
      const content = mockAgentSkillFiles.get(fileKey) ?? "";
      mockAgentSkillFiles.delete(fileKey);
      mockAgentSkillFiles.set(`${name}:${newRelativePath}`, content);
    }
    const directoryKey = `${name}:${relativePath}`;
    if (mockAgentSkillDirectories.delete(directoryKey)) mockAgentSkillDirectories.add(`${name}:${newRelativePath}`);
  },
  async deleteAgentSkillPath(name, relativePath) {
    mockAgentSkillFiles.delete(`${name}:${relativePath}`);
    mockAgentSkillDirectories.delete(`${name}:${relativePath}`);
  },
  async deleteAgentSkill(name) {
    mockAgentSkills.delete(name);
    mockAgentSkillManifests.delete(name);
  },
  async listPlugins() { return mockPlugins(); },
  async installPlugin() { return mockPlugins()[0]; },
  async savePluginConfig(_pluginId, config) { mockPluginConfig = config; },
  async setPluginEnabled(_pluginId, enabled) { mockPluginEnabled = enabled; if (!enabled) mockPluginRunning = false; },
  async startPlugin() { mockPluginEnabled = true; mockPluginRunning = true; },
  async stopPlugin() { mockPluginRunning = false; },
  async uninstallPlugin() { mockPluginEnabled = false; mockPluginRunning = false; },
  async getPluginLogs() { return mockPluginRunning ? [{ timestamp: Date.now(), stream: "stderr", message: "listening on http://127.0.0.1:34121/v1" }] : []; },
  async testPluginConnection() { return { ok: true, message: "Dify App API 连接成功", duration_ms: 18, details: { input_fields: 1 } }; },
  async listPluginProjects() { return mockPluginProjects; },
  async createPluginProject(pluginId, name, projectType) {
    const project = { id: pluginId, name, version: "0.1.0", description: "DRPA 插件项目", types: [projectType === "tool" ? "tool-provider" : "provider-adapter", ...(projectType === "service" ? ["service"] : [])], directory: `浏览器预览数据/plugin-projects/${pluginId}`, valid: true, validationMessage: "插件清单与入口文件有效" };
    mockPluginProjects = [...mockPluginProjects, project];
    return project;
  },
  async validatePluginProject(pluginId) { return mockPluginProjects.find((item) => item.id === pluginId) ?? Promise.reject(new Error("插件项目不存在")); },
  async buildPluginProject(pluginId) { return `浏览器预览数据/build/plugins/${pluginId}-0.1.0.drpa-plugin`; },
};

const tauriGateway: DesktopGateway = {
  getWorkspaceSnapshot: () => invoke<WorkspaceSnapshot>("get_workspace_snapshot"),
  reportUiReady: () => invoke<void>("report_ui_ready"),
  reportUiInputReady: () => invoke<void>("report_ui_input_ready"),
  installPackage: (archivePath) => invoke<PackageSummary>("install_package", { archivePath }),
  uninstallPackage: (packageId) => invoke<void>("uninstall_package", { packageId }),
  startRun: (packageId, profileId, parameters) => invoke<string>("start_run", { packageId, profileId, parameters }),
  cancelRun: (runId) => invoke<void>("cancel_run", { runId }),
  getRunDetail: (runId) => invoke<RunDetail>("get_run_detail", { runId }),
  openRunOutputDirectory: (runId) => invoke<void>("open_run_output_directory", { runId }),
  listStudioProjects: () => invoke<StudioProject[]>("list_studio_projects"),
  createStudioProject: (name) => invoke<StudioProject>("create_studio_project", { name }),
  openInstalledPackage: (packageId) => invoke<StudioProject>("open_installed_package", { packageId }),
  readProjectFile: (projectId, relativePath) => invoke<string>("read_project_file", { projectId, relativePath }),
  writeProjectFile: (projectId, relativePath, content) => invoke<void>("write_project_file", { projectId, relativePath, content }),
  createProjectDirectory: (projectId, relativePath) => invoke<void>("create_project_directory", { projectId, relativePath }),
  renameProjectEntry: (projectId, relativePath, targetPath) => invoke<void>("rename_project_entry", { projectId, relativePath, targetPath }),
  deleteProjectEntry: (projectId, relativePath) => invoke<void>("delete_project_entry", { projectId, relativePath }),
  deleteStudioProject: (projectId) => invoke<void>("delete_studio_project", { projectId }),
  importProjectFile: (projectId, sourcePath, targetDirectory) => invoke<string>("import_project_file", { projectId, sourcePath, targetDirectory }),
  buildStudioProject: (projectId) => invoke<string>("build_studio_project", { projectId }),
  openBuildOutputDirectory: () => invoke<void>("open_build_output_directory"),
  runStudioProject: (projectId, parameters) => invoke<string>("run_studio_project", { projectId, parameters }),
  executeStudioCell: (projectId, code) => invoke<StudioCellResult>("execute_studio_cell", { projectId, code }),
  completeStudioPython: (projectId, code, cursorPos) => invoke<StudioCompletionResult>("complete_studio_python", { projectId, code, cursorPos }),
  inspectStudioPython: (projectId, code, cursorPos, detailLevel) => invoke<StudioInspectResult>("inspect_studio_python", { projectId, code, cursorPos, detailLevel }),
  prepareStudioKernel: (projectId) => invoke<void>("prepare_studio_kernel", { projectId }),
  restartStudioKernel: (projectId) => invoke<void>("restart_studio_kernel", { projectId }),
  getWorkspaceDatabaseInfo: () => invoke<DatabaseInfo>("get_workspace_database_info"),
  listDatabaseTables: () => invoke<DatabaseTable[]>("list_database_tables"),
  describeDatabaseTable: (tableName) => invoke<DatabaseColumn[]>("describe_database_table", { tableName }),
  executeDatabaseSql: (sql) => invoke<DatabaseQueryResult>("execute_database_sql", { sql }),
  getDatabaseSchemaContext: () => invoke<string>("get_database_schema_context"),
  openWorkspaceDatabaseDirectory: () => invoke<void>("open_workspace_database_directory"),
  listRemoteDatabaseProfiles: () => invoke<RemoteDatabaseProfile[]>("list_remote_database_profiles"),
  saveRemoteDatabaseProfile: (profile) => invoke<RemoteDatabaseProfile>("save_remote_database_profile", { profile }),
  deleteRemoteDatabaseProfile: (profileId) => invoke<void>("delete_remote_database_profile", { profileId }),
  testRemoteDatabaseConnection: (profile, password) => invoke<RemoteConnectionTest>("test_remote_database_connection", { profile, password }),
  listRemoteDatabaseTables: (profileId, password) => invoke<DatabaseTable[]>("list_remote_database_tables", { profileId, password }),
  describeRemoteDatabaseTable: (profileId, password, tableName) => invoke<DatabaseColumn[]>("describe_remote_database_table", { profileId, password, tableName }),
  executeRemoteDatabaseSql: (profileId, password, sql) => invoke<DatabaseQueryResult>("execute_remote_database_sql", { profileId, password, sql }),
  getRemoteDatabaseSchemaContext: (profileId, password) => invoke<string>("get_remote_database_schema_context", { profileId, password }),
  listLocalDifyApps: () => invoke<LocalDifyApp[]>("list_local_dify_apps"),
  createLocalDifyApp: (name, mode) => invoke<LocalDifyApp>("create_local_dify_app", { input: { name, mode } }),
  saveLocalDifyApp: (app) => invoke<LocalDifyApp>("save_local_dify_app", { app }),
  validateLocalDifyWorkflow: (graph, mode) => invoke<LocalDifyWorkflowValidationReport>("validate_local_dify_workflow", { graph, mode }),
  createLocalDifyWorkflowNode: (kind, x, y) => invoke<LocalDifyWorkflowNode>("create_local_dify_workflow_node", { kind, x, y }),
  deleteLocalDifyApp: (appId) => invoke<void>("delete_local_dify_app", { appId }),
  listLocalDifyProviders: () => invoke<LocalDifyProvider[]>("list_local_dify_providers"),
  saveLocalDifyProvider: (input) => invoke<LocalDifyProvider>("save_local_dify_provider", { input }),
  deleteLocalDifyProvider: (providerId) => invoke<void>("delete_local_dify_provider", { providerId }),
  testLocalDifyProvider: (providerId) => invoke<LocalDifyProviderTest>("test_local_dify_provider", { providerId }),
  runLocalDifyApp: (request) => invoke<LocalDifyRunResult>("run_local_dify_app", { request }),
  listenLocalDifyStream: async (requestId, onEvent) => listen<LocalDifyStreamEvent>(`local-dify-stream-${requestId}`, (event) => onEvent(event.payload)),
  listLocalDifyRuns: (appId, limit) => invoke<LocalDifyRunSummary[]>("list_local_dify_runs", { appId, limit }),
  publishLocalDifyApp: (appId) => invoke<LocalDifyApp>("publish_local_dify_app", { appId }),
  getLocalDifyAppApiToken: (appId) => invoke<string>("get_local_dify_app_api_token", { appId }),
  checkLocalDifyCompatibility: (appId) => invoke<DifyCompatibilityReport>("check_local_dify_compatibility", { appId }),
  importLocalDifyDsl: (sourcePath) => invoke<LocalDifyApp>("import_local_dify_dsl", { sourcePath }),
  exportLocalDifyDsl: (appId, targetPath) => invoke<string>("export_local_dify_dsl", { appId, targetPath }),
  getLocalDifyServiceStatus: () => invoke<LocalDifyServiceStatus>("get_local_dify_service_status"),
  startLocalDifyService: (port) => invoke<LocalDifyServiceStatus>("start_local_dify_service", { port }),
  stopLocalDifyService: () => invoke<LocalDifyServiceStatus>("stop_local_dify_service"),
  runAgentTurn: (request) => invoke<AgentTurnResult>("run_agent_turn", { request }),
  listenAgentStream: async (requestId, onEvent) => listen<AgentStreamEvent>(`agent-stream-${requestId}`, (event) => onEvent(event.payload)),
  getRuntimeStatus: () => invoke<RuntimeStatus>("get_runtime_status"),
  getPlatformCapabilities: () => invoke<PlatformCapabilities>("get_platform_capabilities"),
  initializeRuntime: () => invoke<RuntimeStatus>("initialize_runtime"),
  repairRuntime: () => invoke<RuntimeStatus>("repair_runtime"),
  applyWindowsUpdate: (packagePath) => invoke<WindowsUpdateSession>("apply_windows_update", { packagePath }),
  getWindowsUpdateStatus: (sessionId) => invoke<WindowsUpdateStatus>("get_windows_update_status", { sessionId }),
  getLatestWindowsUpdateStatus: () => invoke<WindowsUpdateStatus | null>("get_latest_windows_update_status"),
  restartForWindowsUpdate: (sessionId) => invoke<void>("restart_for_windows_update", { sessionId }),
  getDataDirectory: () => invoke<string>("get_data_directory"),
  openWorkspaceDataDirectory: () => invoke<void>("open_workspace_data_directory"),
  getCurrentUser: () => invoke<CurrentUser>("get_current_user"),
  getAgentWorkspaceConfig: () => invoke<AgentWorkspaceConfig>("get_agent_workspace_config"),
  writeAgentWorkspaceDocument: (document, content) => invoke<void>("write_agent_workspace_document", { document, content }),
  readAgentSkill: (name) => invoke<string>("read_agent_skill", { name }),
  readAgentSkillPackage: (name) => invoke<AgentSkillPackage>("read_agent_skill_package", { name }),
  writeAgentSkill: (name, content) => invoke<void>("write_agent_skill", { name, content }),
  writeAgentSkillPackage: (name, manifestYaml, instructionsMarkdown) => invoke<void>("write_agent_skill_package", { name, manifestYaml, instructionsMarkdown }),
  readAgentSkillFile: (name, relativePath) => invoke<string>("read_agent_skill_file", { name, relativePath }),
  writeAgentSkillFile: (name, relativePath, content) => invoke<void>("write_agent_skill_file", { name, relativePath, content }),
  createAgentSkillDirectory: (name, relativePath) => invoke<void>("create_agent_skill_directory", { name, relativePath }),
  renameAgentSkillPath: (name, relativePath, newRelativePath) => invoke<void>("rename_agent_skill_path", { name, relativePath, newRelativePath }),
  deleteAgentSkillPath: (name, relativePath) => invoke<void>("delete_agent_skill_path", { name, relativePath }),
  deleteAgentSkill: (name) => invoke<void>("delete_agent_skill", { name }),
  listPlugins: () => invoke<PluginSummary[]>("list_plugins"),
  installPlugin: (packagePath) => invoke<PluginSummary>("install_plugin", { packagePath }),
  savePluginConfig: (pluginId, config, autostart) => invoke<void>("save_plugin_config", { pluginId, config, autostart }),
  setPluginEnabled: (pluginId, enabled) => invoke<void>("set_plugin_enabled", { pluginId, enabled }),
  startPlugin: (pluginId) => invoke<void>("start_plugin", { pluginId }),
  stopPlugin: (pluginId) => invoke<void>("stop_plugin", { pluginId }),
  uninstallPlugin: (pluginId) => invoke<void>("uninstall_plugin", { pluginId }),
  getPluginLogs: (pluginId) => invoke<PluginLogLine[]>("get_plugin_logs", { pluginId }),
  testPluginConnection: (pluginId) => invoke<PluginConnectionTest>("test_plugin_connection", { pluginId }),
  listPluginProjects: () => invoke<PluginProjectSummary[]>("list_plugin_projects"),
  createPluginProject: (pluginId, name, projectType) => invoke<PluginProjectSummary>("create_plugin_project", { pluginId, name, projectType }),
  validatePluginProject: (pluginId) => invoke<PluginProjectSummary>("validate_plugin_project", { pluginId }),
  buildPluginProject: (pluginId) => invoke<string>("build_plugin_project", { pluginId }),
  listKnowledgeEntries: () => invoke<KnowledgeEntry[]>("list_knowledge_entries"),
  readKnowledgeFile: (relativePath) => invoke<string>("read_knowledge_file", { relativePath }),
  writeKnowledgeFile: (relativePath, content) => invoke<void>("write_knowledge_file", { relativePath, content }),
  createKnowledgeEntry: (relativePath, kind) => invoke<void>("create_knowledge_entry", { relativePath, kind }),
  renameKnowledgeEntry: (relativePath, targetPath) => invoke<void>("rename_knowledge_entry", { relativePath, targetPath }),
  deleteKnowledgeEntry: (relativePath) => invoke<void>("delete_knowledge_entry", { relativePath }),
  importKnowledgeFiles: (sourcePaths, targetDirectory) => invoke<string[]>("import_knowledge_files", { sourcePaths, targetDirectory }),
  exportKnowledgeFile: (relativePath, targetPath) => invoke<string>("export_knowledge_file", { relativePath, targetPath }),
};

export const desktopGateway: DesktopGateway = isTauriHost() ? tauriGateway : mockGateway;
