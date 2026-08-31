import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

import { mockSnapshot } from "../data/mockSnapshot";
import type {
  AgentDocumentArtifact,
  AgentDocumentAttachment,
  AgentDocumentExport,
  AgentConversationProject,
  AgentConversationSession,
  AgentConversationSessionSummary,
  AgentExtensionSummary,
  AgentRunSnapshot,
  AgentTurnRequest,
  AgentTurnResult,
  AgentStreamEvent,
  AgentToolEvent,
  AgentWorkspaceConfig,
  AgentSkillPackage,
  CurrentUser,
  DatabaseColumn,
  DatabaseExportFormat,
  DatabaseExportResult,
  DatabaseInfo,
  DatabaseQueryResult,
  DatabaseTable,
  DashboardDocument,
  DashboardWidget,
  VaultCredential,
  VaultCredentialInput,
  VaultCredentialSummary,
  VaultServiceStatus,
  VaultSetup,
  VaultStatus,
  VaultUnlockResult,
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
  PluginDebuggerResponse,
  PluginJsonValue,
  PluginLogLine,
  PluginConnectionTest,
  PluginProjectSummary,
  PluginSummary,
  PluginToolDescriptor,
  PluginToolWorkbenchResult,
  SystemMetricsSnapshot,
  RuntimePythonPackageCatalog,
  RuntimeStatus,
  RunDetail,
  StudioCellResult,
  StudioCompletionResult,
  StudioInspectResult,
  PythonFlowGraph,
  PythonFlowValidationResult,
  StudioProject,
  WindowsUpdateSession,
  WindowsUpdateStatus,
  WorkspaceInfo,
  WorkspaceSnapshot,
  UserDataTransferResult,
  KnowledgeBaseSearchResult,
  KnowledgeBaseSource,
  KnowledgeBaseSummary,
  AutomationPlan,
  AutomationPlanInput,
  AutomationRun,
} from "../domain/models";

export interface DesktopGateway {
  listWorkspaces(): Promise<WorkspaceInfo[]>;
  createWorkspace(name: string): Promise<WorkspaceInfo>;
  switchWorkspace(workspaceId: string): Promise<void>;
  getWorkspaceSnapshot(): Promise<WorkspaceSnapshot>;
  reportUiReady(): Promise<void>;
  completeStartup(): Promise<void>;
  reportUiInputReady(): Promise<void>;
  installPackage(archivePath: string): Promise<PackageSummary>;
  uninstallPackage(packageId: string): Promise<void>;
  startRun(packageId: string, profileId: string, parameters: Record<string, unknown>): Promise<string>;
  cancelRun(runId: string): Promise<void>;
  saveOpticalReceivedFile(targetPath: string, payloadBase64: string): Promise<string>;
  getRunDetail(runId: string): Promise<RunDetail>;
  openRunOutputDirectory(runId: string): Promise<void>;
  listAutomationPlans(): Promise<AutomationPlan[]>;
  createAutomationPlan(input: AutomationPlanInput): Promise<AutomationPlan>;
  updateAutomationPlan(input: AutomationPlanInput): Promise<AutomationPlan>;
  deleteAutomationPlan(planId: string): Promise<void>;
  setAutomationPlanEnabled(planId: string, enabled: boolean): Promise<AutomationPlan>;
  runAutomationPlanNow(planId: string): Promise<AutomationRun>;
  listAutomationRuns(planId?: string, limit?: number): Promise<AutomationRun[]>;
  listStudioProjects(): Promise<StudioProject[]>;
  createStudioProject(name: string): Promise<StudioProject>;
  renameStudioProject(projectId: string, name: string): Promise<StudioProject>;
  openInstalledPackage(packageId: string): Promise<StudioProject>;
  readProjectFile(projectId: string, relativePath: string): Promise<string>;
  writeProjectFile(projectId: string, relativePath: string, content: string): Promise<void>;
  createProjectDirectory(projectId: string, relativePath: string): Promise<void>;
  renameProjectEntry(projectId: string, relativePath: string, targetPath: string): Promise<void>;
  deleteProjectEntry(projectId: string, relativePath: string): Promise<void>;
  deleteStudioProject(projectId: string): Promise<void>;
  importProjectFile(projectId: string, sourcePath: string, targetDirectory: string): Promise<string>;
  buildStudioProject(projectId: string): Promise<string>;
  installStudioProject(projectId: string): Promise<PackageSummary>;
  openBuildOutputDirectory(): Promise<void>;
  runStudioProject(projectId: string, parameters: Record<string, unknown>): Promise<string>;
  executeStudioCell(projectId: string, code: string): Promise<StudioCellResult>;
  completeStudioPython(projectId: string, code: string, cursorPos: number): Promise<StudioCompletionResult>;
  inspectStudioPython(projectId: string, code: string, cursorPos: number, detailLevel: number): Promise<StudioInspectResult>;
  parsePythonFlow(source: string, sourceName?: string): Promise<PythonFlowGraph>;
  renderPythonFlow(flow: PythonFlowGraph): Promise<string>;
  validatePythonFlow(flow: PythonFlowGraph): Promise<PythonFlowValidationResult>;
  prepareStudioKernel(projectId: string): Promise<void>;
  restartStudioKernel(projectId: string): Promise<void>;
  getWorkspaceDatabaseInfo(): Promise<DatabaseInfo>;
  listDatabaseTables(): Promise<DatabaseTable[]>;
  describeDatabaseTable(tableName: string): Promise<DatabaseColumn[]>;
  executeDatabaseSql(sql: string, offset?: number, limit?: number): Promise<DatabaseQueryResult>;
  selectDatabaseExportPath(format: DatabaseExportFormat, suggestedName: string): Promise<string | null>;
  exportDatabaseQueryResult(result: DatabaseQueryResult, format: DatabaseExportFormat, targetPath: string, tableName: string): Promise<DatabaseExportResult>;
  getDatabaseSchemaContext(): Promise<string>;
  openWorkspaceDatabaseDirectory(): Promise<void>;
  selectDatabaseSourceFile(engine: "sqlite" | "excel"): Promise<string | null>;
  listRemoteDatabaseProfiles(): Promise<RemoteDatabaseProfile[]>;
  saveRemoteDatabaseProfile(profile: RemoteDatabaseProfile): Promise<RemoteDatabaseProfile>;
  deleteRemoteDatabaseProfile(profileId: string): Promise<void>;
  testRemoteDatabaseConnection(profile: RemoteDatabaseProfile, password: string): Promise<RemoteConnectionTest>;
  listRemoteDatabaseTables(profileId: string, password: string): Promise<DatabaseTable[]>;
  describeRemoteDatabaseTable(profileId: string, password: string, tableName: string): Promise<DatabaseColumn[]>;
  executeRemoteDatabaseSql(profileId: string, password: string, sql: string, offset?: number, limit?: number): Promise<DatabaseQueryResult>;
  getRemoteDatabaseSchemaContext(profileId: string, password: string): Promise<string>;
  getBiDashboard(): Promise<DashboardDocument>;
  saveBiDashboard(document: DashboardDocument): Promise<DashboardDocument>;
  resetBiDashboard(): Promise<DashboardDocument>;
  executeDashboardDatabaseQuery(profileId: string, password: string, sql: string): Promise<DatabaseQueryResult>;
  getVaultStatus(): Promise<VaultStatus>;
  beginVaultSetup(): Promise<VaultSetup>;
  completeVaultSetup(setupId: string, code: string): Promise<VaultUnlockResult>;
  unlockVault(code: string): Promise<VaultUnlockResult>;
  unlockVaultWithRecovery(recoveryCode: string): Promise<VaultUnlockResult>;
  lockVault(): Promise<void>;
  listVaultCredentials(): Promise<VaultCredentialSummary[]>;
  getVaultCredential(id: string): Promise<VaultCredential>;
  saveVaultCredential(input: VaultCredentialInput): Promise<VaultCredential>;
  deleteVaultCredential(id: string): Promise<void>;
  startVaultService(port: number): Promise<VaultServiceStatus>;
  stopVaultService(): Promise<VaultServiceStatus>;
  exportVaultRecoveryCode(recoveryCode: string, targetPath: string): Promise<string>;
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
  cancelAgentRun(requestId: string): Promise<AgentRunSnapshot>;
  getAgentRun(requestId: string): Promise<AgentRunSnapshot>;
  selectAgentDocumentFiles(): Promise<string[]>;
  selectAgentArtifactExportPath(suggestedName: string): Promise<string | null>;
  importAgentDocument(sourcePath: string, sessionId: string): Promise<AgentDocumentAttachment>;
  listAgentAttachments(sessionId: string): Promise<AgentDocumentAttachment[]>;
  deleteAgentAttachment(sessionId: string, attachmentId: string): Promise<void>;
  listAgentArtifacts(sessionId: string): Promise<AgentDocumentArtifact[]>;
  exportAgentArtifact(sessionId: string, artifactId: string, destinationPath: string): Promise<AgentDocumentExport>;
  listAgentProjects(): Promise<AgentConversationProject[]>;
  createAgentProject(name: string): Promise<AgentConversationProject>;
  openAgentFileSession(selectedSkillIds?: string[]): Promise<AgentConversationSession | null>;
  renameAgentProject(projectId: string, name: string): Promise<AgentConversationProject>;
  listAgentSessions(projectId?: string): Promise<AgentConversationSessionSummary[]>;
  createAgentSession(projectId?: string, title?: string, selectedSkillIds?: string[]): Promise<AgentConversationSession>;
  getAgentSession(sessionId: string): Promise<AgentConversationSession>;
  saveAgentSession(session: AgentConversationSession): Promise<AgentConversationSession>;
  renameAgentSession(sessionId: string, title: string): Promise<AgentConversationSession>;
  moveAgentSession(sessionId: string, projectId?: string): Promise<AgentConversationSession>;
  deleteAgentSession(sessionId: string): Promise<void>;
  listAgentExtensions(): Promise<AgentExtensionSummary[]>;
  selectAgentExtensionPackage(): Promise<string | null>;
  installAgentExtension(packagePath: string): Promise<AgentExtensionSummary>;
  setAgentExtensionEnabled(extensionId: string, enabled: boolean): Promise<AgentExtensionSummary>;
  removeAgentExtension(extensionId: string): Promise<void>;
  getRuntimeStatus(): Promise<RuntimeStatus>;
  selectRuntimeProfile(profileId: string): Promise<RuntimeStatus>;
  listRuntimePythonPackages(): Promise<RuntimePythonPackageCatalog>;
  installRuntimePythonPackage(requirement: string): Promise<RuntimePythonPackageCatalog>;
  uninstallRuntimePythonPackage(packageName: string): Promise<RuntimePythonPackageCatalog>;
  getSystemMetrics(): Promise<SystemMetricsSnapshot>;
  getPlatformCapabilities(): Promise<PlatformCapabilities>;
  initializeRuntime(): Promise<RuntimeStatus>;
  repairRuntime(): Promise<RuntimeStatus>;
  applyWindowsUpdate(packagePath: string): Promise<WindowsUpdateSession>;
  getWindowsUpdateStatus(sessionId: string): Promise<WindowsUpdateStatus>;
  getLatestWindowsUpdateStatus(): Promise<WindowsUpdateStatus | null>;
  restartForWindowsUpdate(sessionId: string): Promise<void>;
  getDataDirectory(): Promise<string>;
  openWorkspaceDataDirectory(): Promise<void>;
  exportUserData(targetPath: string): Promise<UserDataTransferResult>;
  importUserData(sourcePath: string): Promise<UserDataTransferResult>;
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
  runPluginDebugger(pluginId: string, endpointId: string, request?: PluginJsonValue): Promise<PluginDebuggerResponse>;
  listPluginTools(pluginId: string): Promise<PluginToolDescriptor[]>;
  invokePluginTool(pluginId: string, toolName: string, input: Record<string, unknown>): Promise<PluginToolWorkbenchResult>;
  listPluginProjects(): Promise<PluginProjectSummary[]>;
  createPluginProject(pluginId: string, name: string, projectType: "tool" | "service" | "bundle"): Promise<PluginProjectSummary>;
  validatePluginProject(pluginId: string): Promise<PluginProjectSummary>;
  buildPluginProject(pluginId: string): Promise<string>;
  listKnowledgeEntries(): Promise<KnowledgeEntry[]>;
  listKnowledgeBases(): Promise<KnowledgeBaseSummary[]>;
  createKnowledgeBase(name: string, description: string): Promise<KnowledgeBaseSummary>;
  deleteKnowledgeBase(knowledgeBaseId: string): Promise<void>;
  listKnowledgeBaseSources(knowledgeBaseId: string): Promise<KnowledgeBaseSource[]>;
  importKnowledgeBaseFiles(knowledgeBaseId: string, paths: string[]): Promise<KnowledgeBaseSource[]>;
  importKnowledgeBaseDirectory(knowledgeBaseId: string, directoryPath: string): Promise<KnowledgeBaseSource[]>;
  addKnowledgeBaseText(knowledgeBaseId: string, title: string, content: string): Promise<KnowledgeBaseSource>;
  addKnowledgeBaseUrl(knowledgeBaseId: string, url: string): Promise<KnowledgeBaseSource>;
  deleteKnowledgeBaseSource(knowledgeBaseId: string, sourceId: string): Promise<void>;
  searchKnowledgeBase(knowledgeBaseIds: string[], query: string, limit: number): Promise<KnowledgeBaseSearchResult[]>;
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
let mockActiveWorkspaceId = "personal";
let mockWorkspaces: WorkspaceInfo[] = [{
  id: "personal",
  name: "个人工作区",
  path: "浏览器预览数据（内存）",
  active: true,
  createdAt: Date.now(),
}];
let mockAutomationPlans: AutomationPlan[] = [];
let mockAutomationRuns: AutomationRun[] = [];
let mockKnowledgeBases: KnowledgeBaseSummary[] = [{
  id: "kb-browser-preview",
  name: "产品资料",
  description: "浏览器预览中的向量知识库示例。",
  sourceCount: 1,
  chunkCount: 2,
  status: "ready",
  updatedAt: Date.now(),
}];
let mockKnowledgeBaseSources: KnowledgeBaseSource[] = [{
  id: "source-browser-preview",
  knowledgeBaseId: "kb-browser-preview",
  name: "DRPA 简介.md",
  kind: "file",
  status: "ready",
  chunkCount: 2,
  sizeBytes: 680,
  uri: "DRPA 简介.md",
  lastError: "",
  updatedAt: Date.now(),
}];
const mockAgentArtifacts = new Map<string, AgentDocumentArtifact[]>();
const mockAgentAttachments = new Map<string, AgentDocumentAttachment[]>();
let mockAgentProjects: AgentConversationProject[] = [];
let mockAgentSessions: AgentConversationSession[] = [];
let mockAgentExtensions: AgentExtensionSummary[] = [{
  id: "drpa-quickjs-example",
  name: "DRPA QuickJS Hostcall 示例",
  version: "1.0.0",
  description: "无 Node 依赖的 Pi 风格扩展，用于验证 registerTool 与受控 hostcall。",
  enabled: true,
  runtime: "QuickJS",
  source: "bundled",
  integrity: "browser-preview",
  directory: "浏览器预览数据/agent/extensions/drpa-quickjs-example",
  tools: [{
    name: "project_file_overview",
    exposedName: "ext__drpa_quickjs_example__project_file_overview",
    label: "项目文件概览",
    description: "通过 DRPA hostcall 快速统计当前项目匹配的文件。",
    parameters: { type: "object" },
  }],
}];
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
let mockVaultUnlocked = false;
let mockVaultInitialized = false;
let mockVaultService: VaultServiceStatus = { running: false, port: 34131, endpoint: "http://127.0.0.1:34131/v1/vault", lastError: "" };
let mockVaultCredentials: VaultCredential[] = [];
const mockBiDashboardSeed: DashboardDocument = {
  schema: 1,
  activeDashboardId: "home",
  dashboards: [{
    id: "home",
    title: "业务总览",
    description: "把 DRPA 运行数据与数据工作台查询组合成一个可编辑的 BI 主页。",
    columns: 12,
    rowHeight: 58,
    widgets: [
      dashboardMetric("active-runs", "活动任务", 0, "activeRuns", "number", "#4f6bed"),
      dashboardMetric("success-rate", "30 天成功率", 3, "successRate", "percent", "#159570"),
      dashboardMetric("package-count", "RPAZ 包", 6, "packages", "number", "#8b5cf6"),
      dashboardMetric("saved-hours", "预计节省时间", 9, "savedHours", "hours", "#d97706"),
      {
        id: "run-duration", title: "最近运行耗时", kind: "line", layout: { x: 0, y: 2, w: 8, h: 5 },
        source: { kind: "builtin", dataset: "runHistory" },
        encoding: { categoryField: "startedAt", valueField: "durationSeconds", seriesField: "" },
        options: { text: "", numberFormat: "number", color: "#4f6bed", showLegend: true, refreshSeconds: 0 },
      },
      {
        id: "run-status", title: "运行状态分布", kind: "pie", layout: { x: 8, y: 2, w: 4, h: 5 },
        source: { kind: "builtin", dataset: "runStatus" },
        encoding: { categoryField: "status", valueField: "count", seriesField: "" },
        options: { text: "", numberFormat: "number", color: "#4f6bed", showLegend: true, refreshSeconds: 0 },
      },
      {
        id: "recent-runs", title: "最近运行", kind: "table", layout: { x: 0, y: 7, w: 12, h: 5 },
        source: { kind: "builtin", dataset: "runHistory" },
        encoding: { categoryField: "", valueField: "", seriesField: "" },
        options: { text: "", numberFormat: "number", color: "#4f6bed", showLegend: true, refreshSeconds: 0 },
      },
    ],
  }],
};
let mockBiDashboard = structuredClone(mockBiDashboardSeed);
let mockLocalDifyProviders: LocalDifyProvider[] = [{
  id: "provider-openai-compatible",
  name: "OpenAI 兼容 Provider",
  baseUrl: "http://127.0.0.1/v1",
  model: "deepseek-v4-flash",
  contextWindow: 393216,
  maxOutputTokens: 98304,
  temperature: 0.2,
  streaming: true,
  supportsTools: true,
  supportsJson: true,
  supportsVision: false,
  timeoutSeconds: 120,
  customHeaders: {},
  difyProvider: "langgenius/openai/openai",
  difyModel: "deepseek-v4-flash",
  hasApiKey: false,
  updatedAt: Date.now(),
}];
let mockLocalDifyApps: LocalDifyApp[] = [{
  schema: 2,
  id: "app-browser-preview",
  name: "本地 Dify 调试应用",
  description: "通过 OpenAI 兼容 Provider 调试提示词并导出 Dify DSL。",
  mode: "chat",
  providerId: "provider-openai-compatible",
  systemPrompt: "你是 DRPA Local Dify 中的开发助手。回答应准确、简洁，并说明关键步骤。",
  openingStatement: "你好，这是一个本地 Dify 调试应用。",
  inputKey: "query",
  temperature: 0.2,
  maxOutputTokens: 98304,
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
    "question-classifier": { title: "问题分类器", height: 116, config: { query_variable_selector: ["start", "query"], classes: [{ id: "1", name: "类别 1" }, { id: "2", name: "类别 2" }] } },
    "parameter-extractor": { title: "参数提取器", height: 112, config: { query: ["start", "query"], parameters: [{ name: "result", type: "string", description: "需要提取的结果" }], instruction: "从输入文本中提取结构化参数。" } },
    "variable-aggregator": { title: "变量聚合器", height: 96, config: { variables: [["start", "query"]], output_type: "any" } },
    "list-operator": { title: "列表操作", height: 108, config: { variable: ["start", "query"], filter_by: { enabled: false, conditions: [] }, order_by: { enabled: false, key: "", value: "asc" }, limit: { enabled: true, size: 10 } } },
    "document-extractor": { title: "文档提取器", height: 92, config: { variable_selector: ["start", "file_path"], is_array_file: false } },
    "knowledge-retrieval": { title: "知识检索", height: 104, config: { query_variable_selector: ["start", "query"], top_k: 5 } },
    "rpaz-package": { title: "RPAZ 包", height: 96, config: { package_id: "", parameters: { input: "{{#start.query#}}" } } },
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
  "---\nname: rpaz-development\ndescription: 创建、修改、校验或构建 RPAZ 包时使用。\n---\n\n# RPAZ Development\n",
], [
  "data-analysis",
  "---\nname: data-analysis\ndescription: 使用只读查询、Python 与文档能力完成可复核的数据分析。\n---\n\n# 数据分析\n",
]]);
const mockAgentSkillManifests = new Map<string, string>([[
  "rpaz-development",
  "schema: 2\nid: rpaz-development\nname: RPAZ Development\nversion: 2.0.0\ndescription: 创建、修改、校验或构建 RPAZ 包时使用。\ntools: []\nlibraries: []\n",
], [
  "data-analysis",
  "schema: 2\nid: data-analysis\nname: 数据分析\nversion: 2.0.0\ndescription: 使用只读查询、Python 与文档能力完成可复核的数据分析。\ntools: []\nlibraries: []\n",
]]);
const mockAgentSkillFiles = new Map<string, string>();
const mockAgentSkillDirectories = new Set<string>();
let mockPluginProjects: PluginProjectSummary[] = [];
let mockPluginRunning = false;
let mockPluginEnabled = false;
let mockPluginConfig: Record<string, unknown> = {
  dify_base_url: "https://api.dify.ai/v1",
  dify_api_key: "",
  listen_addr: "127.0.0.1:34123",
  model_name: "dify-agent",
  proxy_api_key: "browser-preview-local-token",
  enable_tool_emu: true,
  strip_think_tags: true,
  default_user: "drpa",
  show_agent_thought: false,
};

function mockPlugins(): PluginSummary[] {
  const listenAddress = typeof mockPluginConfig.listen_addr === "string"
    ? mockPluginConfig.listen_addr
    : "127.0.0.1:34123";
  const endpoint = `http://${listenAddress}/v1`;
  const serviceStatus = mockPluginRunning ? "running" : mockPluginEnabled ? "stopped" : "disabled";
  return [{
    id: "dify2api",
    name: "Dify2API 网关",
    version: "1.0.0",
    description: "将 Dify Agent 转换为 OpenAI 兼容服务，并在 DRPA 内完成连通性、模型、对话与请求链路调试。",
    types: ["provider-adapter", "service", "debugger"],
    enabled: mockPluginEnabled,
    autostart: false,
    status: serviceStatus,
    endpoint,
    toolCount: 0,
    serviceCount: 1,
    toolProviderCount: 0,
    services: [{
      id: "gateway",
      title: "OpenAI 兼容网关",
      primary: true,
      transport: "http",
      status: serviceStatus,
      endpoint,
      healthcheck: `http://${listenAddress}/healthz`,
    }],
    providers: [{
      id: "openai",
      title: "Dify Agent（OpenAI 兼容）",
      protocol: "openai",
      serviceId: "gateway",
      endpoint,
      modelConfigKey: "model_name",
      apiKeyConfigKey: "proxy_api_key",
    }],
    debugger: {
      endpoints: [
        {
          id: "health",
          title: "服务健康",
          kind: "health",
          method: "GET",
          endpoint: `http://${listenAddress}/healthz`,
          bearerConfigKey: "",
          requestDefaults: null,
          timeoutSeconds: 5,
        },
        {
          id: "upstream",
          title: "测试 Dify 连接",
          kind: "connection",
          method: "GET",
          endpoint: `http://${listenAddress}/drpa/debug/upstream`,
          bearerConfigKey: "proxy_api_key",
          requestDefaults: null,
          timeoutSeconds: 25,
        },
        {
          id: "models",
          title: "查询兼容模型",
          kind: "openai-models",
          method: "GET",
          endpoint: `${endpoint}/models`,
          bearerConfigKey: "proxy_api_key",
          requestDefaults: null,
          timeoutSeconds: 10,
        },
        {
          id: "chat",
          title: "对话调试",
          kind: "openai-chat",
          method: "POST",
          endpoint: `${endpoint}/chat/completions`,
          bearerConfigKey: "proxy_api_key",
          requestDefaults: {
            model: typeof mockPluginConfig.model_name === "string" ? mockPluginConfig.model_name : "dify-agent",
            messages: [{ role: "user", content: "请简要介绍你自己。" }],
            stream: false,
          },
          timeoutSeconds: 30,
        },
        {
          id: "tool-call",
          title: "工具调用调试",
          kind: "openai-chat",
          method: "POST",
          endpoint: `${endpoint}/chat/completions`,
          bearerConfigKey: "proxy_api_key",
          requestDefaults: {
            model: typeof mockPluginConfig.model_name === "string" ? mockPluginConfig.model_name : "dify-agent",
            messages: [{ role: "user", content: "请查询上海当前时间，并调用可用工具。" }],
            tools: [{
              type: "function",
              function: {
                name: "get_time",
                description: "查询指定城市的当前时间",
                parameters: {
                  type: "object",
                  properties: { city: { type: "string" } },
                  required: ["city"],
                },
              },
            }],
            tool_choice: "auto",
            stream: false,
          },
          timeoutSeconds: 30,
        },
      ],
      panels: [
        {
          id: "traffic",
          title: "请求链路",
          kind: "structured-log",
          endpoint: "",
          config: { groupBy: "req_id" },
        },
        {
          id: "service-log",
          title: "服务日志",
          kind: "log",
          endpoint: "",
          config: { serviceId: "gateway" },
        },
      ],
    },
    config: Object.fromEntries(
      Object.entries(mockPluginConfig).filter(([key]) => !["dify_api_key", "proxy_api_key"].includes(key)),
    ),
    configuredSecrets: {
      dify_api_key: Boolean(mockPluginConfig.dify_api_key),
      proxy_api_key: Boolean(mockPluginConfig.proxy_api_key),
    },
    configSchema: {
      type: "object",
      properties: {
        dify_base_url: { type: "string", title: "Dify API 地址" },
        dify_api_key: { type: "string", title: "Dify App API Key", secret: true },
        listen_addr: { type: "string", title: "本地监听地址", pattern: "^127\\.0\\.0\\.1:[0-9]{2,5}$" },
        model_name: { type: "string", title: "对外模型名" },
        proxy_api_key: { type: "string", title: "本地代理 API Key", secret: true, minLength: 24 },
        enable_tool_emu: { type: "boolean", title: "启用工具调用适配" },
        strip_think_tags: { type: "boolean", title: "移除 think 标签" },
        default_user: { type: "string", title: "默认用户标识" },
        show_agent_thought: { type: "boolean", title: "显示 Agent Thought" },
      },
      required: ["dify_base_url", "dify_api_key", "listen_addr", "model_name", "proxy_api_key"],
    },
    directory: "浏览器预览数据/plugins/dify2api",
    lastError: "",
  }];
}
const mockAgentStreamListeners = new Map<string, (event: AgentStreamEvent) => void>();
const mockAgentRuns = new Map<string, AgentRunSnapshot>();

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
      ["README.md", studioProjectReadme(project.name)],
      ["manifest.yaml", `schema: 2\nid: local.browser-preview\nname: ${JSON.stringify(project.name.trim())}\nversion: 0.1.0\nentrypoint:\n  runtime: python\n  module: main.py\n  callable: main\n`],
      ["main.py", "def main(ctx):\n    ctx.log.info('你好，DRPA')\n"],
      ["notebook.ipynb", '{"cells":[],"metadata":{},"nbformat":4,"nbformat_minor":5}\n'],
    ]));
  }
  return structuredClone(existing ?? project);
}

function studioProjectReadme(projectName: string): string {
  const safeProjectName = escapeMarkdownText(projectName);
  return `# ${safeProjectName}

> DRPA Studio 为本项目生成的 AI 开发入口。代码、清单与本文档共同构成项目上下文。

## AI 开发入口

开始分析或修改前，按顺序阅读：\`manifest.yaml\` → \`main.py\` → \`README.md\`。先确认包契约和入口，再规划实现；不要只根据文件名猜测行为。

## RPAZ schema 2 约定

- \`manifest.yaml\` 必须保留 \`schema: 2\`，并维护稳定的 \`id\`、\`version\`、Python \`entrypoint\`、\`capabilities\` 与 \`parameters\`。
- \`main.py\` 必须提供清单声明的 callable，默认签名为 \`main(ctx)\`；模块导入阶段避免执行网络、浏览器、数据库或文件写入。
- 参数来自 manifest，代码通过 \`ctx.params\` 读取；新增能力时同步收紧或补充 capabilities。

## ctx 默认能力

- \`ctx.params\`：读取任务配置参数。
- \`ctx.log\`：输出结构化运行日志。
- \`ctx.progress(...)\`：实时报告 0–100 的进度和当前阶段。
- \`ctx.output_file(...)\`：在隔离输出目录创建并登记产物。
- \`ctx.open_output_directory()\`：按任务参数决定是否展示输出目录。
- \`ctx.sql\`：访问工作区 SQLite，使用参数化 SQL 和事务。
- \`ctx.browser(...)\`：连接 DRPA 管理的可复用 DrissionPage 浏览器会话。
- \`ctx.invoke(...)\`：按包 ID 调用已安装 RPAZ 包；保持参数和返回值可序列化。

## 离线依赖

- 目标用户环境默认离线。代码只使用 DRPA sealed runtime 中已经锁定并随平台交付的依赖，不在运行代码中调用 pip，也不依赖系统 Python。
- RPA for Python 已由平台运行时提供，自动化代码使用 \`import rpa as r\` 引入；项目自身不执行在线安装。
- 新增平台级 Python 依赖时，更新 DRPA 源码仓库中的 \`offline/requirements/runtime.txt\` 与对应运行时构建锁，然后重新构建、验证并发布全量 sealed runtime。
- Windows 与 UOS/Linux 均需验证 Python 3.11 ABI，代码应避免写死解释器、浏览器及工作区绝对路径。

## 测试与导出

1. 在 Studio 使用“直接运行”验证当前工作副本，修复 manifest、入口和参数问题。
2. 运行最小参数、边界参数与失败路径，检查实时日志、进度、产物和工作区 SQLite 变更。
3. 对纯函数和外部接口适配层补充测试；外部服务使用可替换夹具，确保离线测试可重复。
4. 点击“导出 RPAZ”，安装生成包；再到运行工作台创建任务配置，执行“预检并保存”和试运行。

## Python Flow 协作约定

- Python 源码是业务逻辑和执行行为的唯一事实源（SSOT）；Python Flow 只承载可视化编排、节点位置和源码映射元数据，不维护第二套隐式业务逻辑。
- 把可视化步骤拆成命名稳定、输入输出显式的函数，由 \`main(ctx)\` 负责薄编排；避免隐藏全局状态、动态 \`exec\` 和导入期副作用。
- AI 修改函数名、参数或返回结构时，同步更新对应节点映射；可视化编辑回写代码后，重新格式化、预检并运行测试。
- 手写代码与生成代码都应保留清晰边界。遇到无法映射的 Python 语义时保留为“代码节点”，不要静默丢弃行为。
`;
}

function escapeMarkdownText(value: string): string {
  const normalized = value
    .replace(/[\u0000-\u001f\u007f-\u009f\u200b-\u200f\u202a-\u202e\u2060-\u206f]/g, " ")
    .trim()
    .replace(/\s+/g, " ");
  return normalized.replace(/[\\`*_{}[\]<>()#+.!|\-]/g, "\\$&");
}

function mockPythonFlow(source: string, sourceName = "main.py"): PythonFlowGraph {
  const lines = source.split(/\r?\n/);
  const functionLine = lines.findIndex((line) => /^\s*(?:async\s+)?def\s+main\s*\(/.test(line));
  if (functionLine < 0) throw new Error("RPAZ 源码需要定义 main(ctx) 入口");
  const semantic = lines
    .map((line, index) => ({ line, index }))
    .filter(({ line, index }) => index > functionLine && /^\s+\S/.test(line));
  const nodes: PythonFlowGraph["nodes"] = [{
    id: "start-preview",
    type: "start",
    label: "Start",
    code: "",
    span: { startLine: functionLine + 1, startColumn: 0, endLine: functionLine + 1, endColumn: 0 },
    data: { entrypoint: "main" },
  }];
  const body: string[] = [];
  for (const { line, index } of semantic) {
    const code = line.trim();
    const id = `node-preview-${index + 1}`;
    const callName = code.match(/^(?:[A-Za-z_]\w*\s*=\s*)?([A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*)\s*\(/)?.[1] ?? "";
    const type: PythonFlowGraph["nodes"][number]["type"] = code.startsWith("return")
      ? "return"
      : callName.startsWith("ctx.")
        ? "ctx-call"
        : callName.startsWith("r.") || callName.startsWith("rpa.")
          ? "rpa-call"
          : callName
            ? "call"
            : /^[A-Za-z_]\w*\s*=/.test(code)
              ? "assign"
              : "raw-code";
    nodes.push({
      id,
      type,
      label: callName || code.slice(0, 72),
      code,
      span: { startLine: index + 1, startColumn: line.length - line.trimStart().length, endLine: index + 1, endColumn: line.length },
      data: { statement: code, callName, arguments: [], keywords: [], assignTargets: [] },
    });
    body.push(id);
  }
  nodes.push({
    id: "end-preview",
    type: "end",
    label: "End",
    code: "",
    span: { startLine: lines.length, startColumn: 0, endLine: lines.length, endColumn: 0 },
    data: {},
  });
  const sequence = ["start-preview", ...body, "end-preview"];
  const edges = sequence.slice(0, -1).map((id, index) => ({
    id: `edge-preview-${index}`,
    source: id,
    target: sequence[index + 1],
    kind: nodes.find((node) => node.id === id)?.type === "return" ? "return" : "next",
    label: "",
  }));
  let hash = 2166136261;
  for (const character of source) hash = Math.imul(hash ^ character.charCodeAt(0), 16777619);
  return {
    schemaVersion: 1,
    kind: "drpa.python-flow",
    source: { name: sourceName.split(/[\\/]/).at(-1) || "main.py", sha256: `preview-${(hash >>> 0).toString(16)}` },
    entrypoint: "main",
    nodes,
    edges,
    metadata: {
      body,
      moduleBefore: lines.slice(0, functionLine).join("\n") + (functionLine > 0 ? "\n" : ""),
      moduleAfter: "\n",
      functionHeader: lines[functionLine].trimEnd(),
      indent: "    ",
    },
  };
}

function renderMockPythonFlow(flow: PythonFlowGraph): string {
  const byId = new Map(flow.nodes.map((node) => [node.id, node]));
  const body = Array.isArray(flow.metadata.body) ? flow.metadata.body.filter((id): id is string => typeof id === "string") : [];
  const indent = typeof flow.metadata.indent === "string" ? flow.metadata.indent : "    ";
  const lines = body.map((id) => byId.get(id)?.code || "pass").flatMap((code) => code.split("\n").map((line) => `${indent}${line}`));
  return `${String(flow.metadata.moduleBefore ?? "")}${String(flow.metadata.functionHeader ?? `def ${flow.entrypoint}(ctx):`)}\n${(lines.length ? lines : [`${indent}pass`]).join("\n")}${String(flow.metadata.moduleAfter ?? "\n")}`;
}

function dashboardMetric(
  id: string,
  title: string,
  x: number,
  valueField: string,
  numberFormat: DashboardWidget["options"]["numberFormat"],
  color: string,
): DashboardWidget {
  return {
    id,
    title,
    kind: "metric",
    layout: { x, y: 0, w: 3, h: 2 },
    source: { kind: "builtin", dataset: "workspaceSummary" },
    encoding: { categoryField: "", valueField, seriesField: "" },
    options: { text: "", numberFormat, color, showLegend: true, refreshSeconds: 0 },
  };
}

function mockVaultStatus(): VaultStatus {
  return {
    initialized: mockVaultInitialized,
    unlocked: mockVaultUnlocked,
    unlockedUntil: mockVaultUnlocked ? Math.floor(Date.now() / 1000) + 86_400 : undefined,
    itemCount: mockVaultUnlocked ? mockVaultCredentials.length : 0,
    failedAttempts: 0,
    retryAfterSeconds: 0,
    service: structuredClone(mockVaultService),
  };
}

const mockGateway: DesktopGateway = {
  async listWorkspaces() {
    return structuredClone(mockWorkspaces.map((workspace) => ({
      ...workspace,
      active: workspace.id === mockActiveWorkspaceId,
    })));
  },
  async createWorkspace(name) {
    const normalized = name.trim();
    if (!normalized) throw new Error("请输入工作区名称");
    if (mockWorkspaces.some((workspace) => workspace.name === normalized)) throw new Error("已存在同名工作区");
    const workspace: WorkspaceInfo = {
      id: `workspace-${Date.now().toString(16).padStart(32, "0").slice(-32)}`,
      name: normalized,
      path: `浏览器预览数据（内存）/${normalized}`,
      active: false,
      createdAt: Date.now(),
    };
    mockWorkspaces = [...mockWorkspaces, workspace];
    return structuredClone(workspace);
  },
  async switchWorkspace(workspaceId) {
    if (!mockWorkspaces.some((workspace) => workspace.id === workspaceId)) throw new Error("目标工作区不存在");
    mockActiveWorkspaceId = workspaceId;
  },
  async getWorkspaceSnapshot() {
    return structuredClone(mockSnapshot);
  },
  async reportUiReady() {},
  async completeStartup() {},
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
  async saveOpticalReceivedFile(targetPath) {
    return targetPath;
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
  async listAutomationPlans() {
    return structuredClone(mockAutomationPlans);
  },
  async createAutomationPlan(input) {
    const now = Date.now();
    const plan: AutomationPlan = {
      ...structuredClone(input),
      id: `automation-${now}`,
      createdAt: now,
      updatedAt: now,
      nextRunAt: input.enabled ? now + 3_600_000 : undefined,
    };
    mockAutomationPlans = [...mockAutomationPlans, plan];
    return structuredClone(plan);
  },
  async updateAutomationPlan(input) {
    const current = mockAutomationPlans.find((plan) => plan.id === input.id);
    if (!current) throw new Error("自动化计划不存在");
    const updated: AutomationPlan = {
      ...current,
      ...structuredClone(input),
      updatedAt: Date.now(),
      nextRunAt: input.enabled ? Date.now() + 3_600_000 : undefined,
    };
    mockAutomationPlans = mockAutomationPlans.map((plan) => plan.id === updated.id ? updated : plan);
    return structuredClone(updated);
  },
  async deleteAutomationPlan(planId) {
    mockAutomationPlans = mockAutomationPlans.filter((plan) => plan.id !== planId);
  },
  async setAutomationPlanEnabled(planId, enabled) {
    const current = mockAutomationPlans.find((plan) => plan.id === planId);
    if (!current) throw new Error("自动化计划不存在");
    const updated = { ...current, enabled, updatedAt: Date.now(), nextRunAt: enabled ? Date.now() + 3_600_000 : undefined };
    mockAutomationPlans = mockAutomationPlans.map((plan) => plan.id === planId ? updated : plan);
    return structuredClone(updated);
  },
  async runAutomationPlanNow(planId) {
    const plan = mockAutomationPlans.find((item) => item.id === planId);
    if (!plan) throw new Error("自动化计划不存在");
    const now = Date.now();
    const run: AutomationRun = {
      id: `automation-run-${now}`,
      planId,
      planName: plan.name,
      trigger: "manual",
      status: "succeeded",
      queuedAt: now,
      startedAt: now,
      finishedAt: now + 240,
      attempt: 1,
      result: { preview: true },
      error: "",
      deliveryResults: plan.deliveryTargets.filter((target) => target.enabled).map((target) => ({
        targetId: target.id,
        targetName: target.name,
        status: "dispatched",
        error: "",
      })),
    };
    mockAutomationRuns = [run, ...mockAutomationRuns];
    return structuredClone(run);
  },
  async listAutomationRuns(planId, limit = 100) {
    return structuredClone(mockAutomationRuns.filter((run) => !planId || run.planId === planId).slice(0, limit));
  },
  async listStudioProjects() {
    return structuredClone(mockStudioProjects);
  },
  async createStudioProject(name) {
    return addMockProject({ id: `project-${Date.now().toString(16).padStart(24, "0").slice(-24)}`, name, files: ["README.md", "main.py", "manifest.yaml", "notebook.ipynb"] });
  },
  async renameStudioProject(projectId, name) {
    const project = mockStudioProjects.find((item) => item.id === projectId);
    if (!project) throw new Error("开发项目不存在");
    project.name = name.trim();
    const contents = mockStudioContents.get(projectId);
    const manifest = contents?.get("manifest.yaml");
    if (manifest) contents?.set("manifest.yaml", manifest.replace(/^name:.*$/m, `name: ${JSON.stringify(project.name)}`));
    return structuredClone(project);
  },
  async openInstalledPackage(packageId) {
    const item = mockSnapshot.packages.find((candidate) => candidate.id === packageId);
    return addMockProject({ id: `project-${Date.now().toString(16).padStart(24, "0").slice(-24)}`, name: item?.name ?? packageId, files: ["README.md", "main.py", "manifest.yaml", "notebook.ipynb"] });
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
  async installStudioProject(projectId) {
    const project = mockStudioProjects.find((item) => item.id === projectId);
    const installed: PackageSummary = {
      id: `local.${projectId}`,
      name: project?.name ?? projectId,
      description: "由开发工作室保存到本地 RPAZ 包库。",
      version: "0.1.0",
      runtime: "Python 3.11",
      trust: "local",
      accent: "#4f6ef7",
      initials: "RP",
      parameters: [],
      profiles: [],
    };
    mockSnapshot.packages = [installed, ...mockSnapshot.packages.filter((item) => item.id !== installed.id)];
    return structuredClone(installed);
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
  async parsePythonFlow(source, sourceName) {
    return mockPythonFlow(source, sourceName);
  },
  async renderPythonFlow(flow) {
    return renderMockPythonFlow(flow);
  },
  async validatePythonFlow(flow) {
    const ids = new Set(flow.nodes.map((node) => node.id));
    if (flow.nodes.filter((node) => node.type === "start").length !== 1) throw new Error("流程需要一个入口节点");
    if (flow.nodes.filter((node) => node.type === "end").length !== 1) throw new Error("流程需要一个结束节点");
    if (flow.edges.some((edge) => !ids.has(edge.source) || !ids.has(edge.target))) throw new Error("流程中存在悬空连线");
    return { ok: true, valid: true };
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
  async executeDatabaseSql(sql, offset = 0, limit = 1_000) {
    return { columns: ["preview", "characters"], rows: [["浏览器预览", sql.length]], affectedRows: 0, durationMs: 1, truncated: false, statementType: "SELECT", offset, limit, hasMore: false };
  },
  async selectDatabaseExportPath() { return null; },
  async exportDatabaseQueryResult(result, format, targetPath) { return { path: targetPath, format, rowCount: result.rows.length }; },
  async getDatabaseSchemaContext() {
    return "-- SQLite 工作区数据库结构\n\nCREATE TABLE example_tasks (id INTEGER PRIMARY KEY, title TEXT NOT NULL, status TEXT);\n";
  },
  async openWorkspaceDatabaseDirectory() {},
  async selectDatabaseSourceFile() { return null; },
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
    const versions: Record<RemoteDatabaseProfile["engine"], string> = {
      postgresql: "PostgreSQL 17.2",
      mysql: "MySQL 8.4",
      sqlite: "SQLite 3.49",
      excel: "Excel 工作簿 · 2 个工作表（只读）",
    };
    return { serverVersion: versions[profile.engine], latencyMs: 12 };
  },
  async listRemoteDatabaseTables() {
    return [{ name: "public.remote_tasks", kind: "table", rowCount: 4 }];
  },
  async describeRemoteDatabaseTable() {
    return [{ ordinal: 0, name: "id", dataType: "bigint", notNull: true, primaryKey: false }];
  },
  async executeRemoteDatabaseSql(_profileId, _password, sql, offset = 0, limit = 1_000) {
    return { columns: ["remote", "characters"], rows: [[true, sql.length]], affectedRows: 0, durationMs: 12, truncated: false, statementType: "SELECT", offset, limit, hasMore: false };
  },
  async getRemoteDatabaseSchemaContext() {
    return "-- PostgreSQL 数据库结构\n\nCREATE TABLE public.remote_tasks (id bigint NOT NULL);\n";
  },
  async getBiDashboard() {
    return structuredClone(mockBiDashboard);
  },
  async saveBiDashboard(document) {
    mockBiDashboard = structuredClone(document);
    return structuredClone(mockBiDashboard);
  },
  async resetBiDashboard() {
    mockBiDashboard = structuredClone(mockBiDashboardSeed);
    return structuredClone(mockBiDashboard);
  },
  async executeDashboardDatabaseQuery(_profileId, _password, sql) {
    return {
      columns: ["category", "value"],
      rows: [["预览", sql.length], ["示例", Math.max(1, Math.round(sql.length / 2))]],
      affectedRows: 0,
      durationMs: 3,
      truncated: false,
      statementType: "SELECT",
      offset: 0,
      limit: 1_000,
      hasMore: false,
    };
  },
  async getVaultStatus() { return mockVaultStatus(); },
  async beginVaultSetup() {
    return { setupId: "setup-browser-preview", account: "developer@browser", issuer: "DRPA", manualKey: "JBSWY3DPEHPK3PXP", otpAuthUri: "otpauth://totp/DRPA%3Adeveloper%40browser?secret=JBSWY3DPEHPK3PXP&issuer=DRPA&algorithm=SHA1&digits=6&period=30", expiresAt: Math.floor(Date.now() / 1000) + 600 };
  },
  async completeVaultSetup(_setupId, code) {
    if (!/^\d{6}$/.test(code)) throw new Error("请输入 6 位验证码");
    mockVaultInitialized = true;
    mockVaultUnlocked = true;
    return { status: mockVaultStatus(), serviceToken: "browser-runtime-token", recoveryCode: "ABCD-EFGH-IJKL-MNOP-QRST-UVWX-YZ23-4567" };
  },
  async unlockVault(code) {
    if (!/^\d{6}$/.test(code)) throw new Error("请输入 6 位验证码");
    mockVaultUnlocked = true;
    return { status: mockVaultStatus(), serviceToken: "browser-runtime-token" };
  },
  async unlockVaultWithRecovery(recoveryCode) {
    if (!recoveryCode.trim()) throw new Error("请输入恢复码");
    mockVaultUnlocked = true;
    return { status: mockVaultStatus(), serviceToken: "browser-runtime-token", recoveryCode: "NEW2-RECO-VERY2-CODE-ABCD-EFGH-IJKL-MNOP" };
  },
  async lockVault() { mockVaultUnlocked = false; },
  async listVaultCredentials() {
    if (!mockVaultUnlocked) throw new Error("凭据保险箱已锁定");
    return structuredClone(mockVaultCredentials.map(({ secret, notes: _notes, createdAt: _createdAt, ...item }) => ({ ...item, hasSecret: Boolean(secret) })));
  },
  async getVaultCredential(id) {
    if (!mockVaultUnlocked) throw new Error("凭据保险箱已锁定");
    const item = mockVaultCredentials.find((credential) => credential.id === id);
    if (!item) throw new Error("凭据不存在");
    return structuredClone(item);
  },
  async saveVaultCredential(input) {
    if (!mockVaultUnlocked) throw new Error("凭据保险箱已锁定");
    const now = Math.floor(Date.now() / 1000);
    const existing = mockVaultCredentials.find((credential) => credential.id === input.id);
    const saved: VaultCredential = { ...input, id: input.id || `credential-${Date.now()}`, hasSecret: Boolean(input.secret), createdAt: existing?.createdAt ?? now, updatedAt: now };
    mockVaultCredentials = existing ? mockVaultCredentials.map((item) => item.id === saved.id ? saved : item) : [saved, ...mockVaultCredentials];
    return structuredClone(saved);
  },
  async deleteVaultCredential(id) { mockVaultCredentials = mockVaultCredentials.filter((item) => item.id !== id); },
  async startVaultService(port) { mockVaultService = { running: true, port, endpoint: `http://127.0.0.1:${port}/v1/vault`, startedAt: Math.floor(Date.now() / 1000), lastError: "" }; return structuredClone(mockVaultService); },
  async stopVaultService() { mockVaultService = { ...mockVaultService, running: false, startedAt: undefined }; return structuredClone(mockVaultService); },
  async exportVaultRecoveryCode(_recoveryCode, targetPath) { return targetPath; },
  async listLocalDifyApps() {
    return structuredClone(mockLocalDifyApps);
  },
  async createLocalDifyApp(name, mode) {
    const now = Date.now();
    const app: LocalDifyApp = {
      schema: 2,
      id: `app-${now}`,
      name,
      description: "用于本地测试与 Dify DSL 导出的流程。",
      mode,
      providerId: mockLocalDifyProviders[0]?.id ?? "",
      systemPrompt: "你是一个准确、简洁的 AI 助手。",
      openingStatement: "你好，我是本地流程助手。",
      inputKey: "query",
      temperature: 0.2,
      maxOutputTokens: 98304,
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
    const startedAt = Date.now();
    const run: AgentRunSnapshot = {
      requestId: request.requestId,
      sessionId: request.sessionId ?? "",
      status: "running",
      createdAt: startedAt,
      startedAt,
      stopReason: "",
      error: "",
      usage: { promptTokens: 0, completionTokens: 0 },
      durationMs: 0,
      events: [],
    };
    mockAgentRuns.set(request.requestId, run);
    const prompt = request.messages.at(-1)?.content ?? "";
    const message = request.mode === "sql"
      ? "已根据当前结构生成查询。\n\n```sql\nSELECT id, title, status FROM example_tasks LIMIT 100;\n```"
      : request.projectId
      ? `已连接浏览器预览 Agent。当前问题：${prompt}`
      : `已收到问题：${prompt}\n选择一个开发项目后可启用 RPAZ 工具。`;
    const listener = mockAgentStreamListeners.get(request.requestId);
    const tools: AgentToolEvent[] = request.projectId ? [{
      callId: `${request.requestId}-tool-1`,
      name: "search_text",
      status: "completed",
      summary: "已完成项目检索",
      input: JSON.stringify({ query: prompt.slice(0, 80) }),
      output: JSON.stringify({ ok: true, matches: 3 }),
      ordinal: 1,
      round: 1,
      durationMs: 84,
    }] : [];
    if (listener) {
      listener({ type: "started", runId: request.requestId, sessionId: request.sessionId ?? "" });
      listener({ type: "roundStarted", round: 1 });
      listener({ type: "contextAssembled", round: 1, estimatedTokens: 1_240, omittedMessages: 0, omittedTools: 0 });
      if (tools[0]) {
        listener({ type: "tool", tool: { ...tools[0], status: "running", summary: "正在检索项目", output: "", durationMs: undefined } });
        await new Promise((resolve) => window.setTimeout(resolve, 84));
        listener({ type: "tool", tool: tools[0] });
        listener({ type: "roundStarted", round: 2 });
        listener({ type: "contextAssembled", round: 2, estimatedTokens: 1_410, omittedMessages: 0, omittedTools: 0 });
      }
    }
    if (request.stream && listener) {
      for (const chunk of message.match(/.{1,12}/gs) ?? [message]) {
        await new Promise((resolve) => window.setTimeout(resolve, 18));
        if (mockAgentRuns.get(request.requestId)?.status === "cancelling") {
          const cancelled = mockAgentRuns.get(request.requestId)!;
          Object.assign(cancelled, { status: "cancelled", stopReason: "cancelled", finishedAt: Date.now() });
          listener({ type: "cancelled", runId: request.requestId });
          throw new Error("Agent 运行已取消");
        }
        listener({ type: "delta", content: chunk });
      }
    } else {
      await new Promise((resolve) => window.setTimeout(resolve, 220));
    }
    const result: AgentTurnResult = {
      message,
      tools,
      usage: { promptTokens: 18, completionTokens: 24 },
      durationMs: 220,
      stopReason: "completed",
      rounds: tools.length > 0 ? 2 : 1,
      toolCalls: tools.length,
      retryCount: 0,
    };
    Object.assign(run, {
      status: "completed",
      stopReason: result.stopReason,
      finishedAt: Date.now(),
      usage: result.usage,
      durationMs: result.durationMs,
    });
    listener?.({
      type: "completed",
      runId: request.requestId,
      usage: result.usage,
      durationMs: result.durationMs,
      stopReason: result.stopReason,
    });
    return result;
  },
  async listenAgentStream(requestId, onEvent) {
    mockAgentStreamListeners.set(requestId, onEvent);
    return () => { mockAgentStreamListeners.delete(requestId); };
  },
  async cancelAgentRun(requestId) {
    const run = mockAgentRuns.get(requestId);
    if (!run) throw new Error("Agent Run 不存在");
    if (run.status === "running" || run.status === "queued") {
      run.status = "cancelling";
      run.stopReason = "cancellation-requested";
    }
    return structuredClone(run);
  },
  async getAgentRun(requestId) {
    const run = mockAgentRuns.get(requestId);
    if (!run) throw new Error("Agent Run 不存在");
    return structuredClone(run);
  },
  async selectAgentDocumentFiles() {
    return [];
  },
  async selectAgentArtifactExportPath(suggestedName) {
    return suggestedName;
  },
  async importAgentDocument(sourcePath, sessionId) {
    const name = sourcePath.split(/[\\/]/).at(-1) ?? "document.pdf";
    const attachment = {
      id: `att-${Date.now()}`,
      name,
      format: name.split(".").at(-1)?.toLowerCase() ?? "",
      sizeBytes: 1024,
      importedAt: new Date().toISOString(),
    };
    mockAgentAttachments.set(sessionId, [
      ...(mockAgentAttachments.get(sessionId) ?? []),
      attachment,
    ]);
    return attachment;
  },
  async listAgentAttachments(sessionId) {
    return structuredClone(mockAgentAttachments.get(sessionId) ?? []);
  },
  async deleteAgentAttachment(sessionId, attachmentId) {
    mockAgentAttachments.set(
      sessionId,
      (mockAgentAttachments.get(sessionId) ?? []).filter((attachment) => attachment.id !== attachmentId),
    );
  },
  async listAgentArtifacts(sessionId) {
    return structuredClone(mockAgentArtifacts.get(sessionId) ?? []);
  },
  async exportAgentArtifact(_sessionId, artifactId, destinationPath) {
    return { artifactId, path: destinationPath, sizeBytes: 1024 };
  },
  async listAgentProjects() {
    return structuredClone(mockAgentProjects.map((project) => ({
      ...project,
      sessionCount: mockAgentSessions.filter((session) => session.projectId === project.id).length,
    })));
  },
  async createAgentProject(name) {
    const now = Date.now();
    const project: AgentConversationProject = {
      id: `project-${Math.random().toString(16).slice(2, 26)}`,
      name: name.trim() || "新项目",
      path: `浏览器预览数据/projects/${now}`,
      createdAt: now,
      updatedAt: now,
      sessionCount: 0,
      source: "managed",
    };
    mockAgentProjects = [...mockAgentProjects, project];
    return structuredClone(project);
  },
  async openAgentFileSession(selectedSkillIds = []) {
    const now = Date.now();
    const project: AgentConversationProject = {
      id: `project-${Math.random().toString(16).slice(2, 26).padEnd(24, "0")}`,
      name: "示例文件项目",
      path: "浏览器预览数据/example",
      createdAt: now,
      updatedAt: now,
      sessionCount: 1,
      source: "external",
    };
    mockAgentProjects = [...mockAgentProjects, project];
    const session: AgentConversationSession = {
      id: `agent-${now}-${Math.random().toString(16).slice(2)}`,
      title: "example.txt",
      projectId: project.id,
      createdAt: now,
      updatedAt: now,
      revision: 1,
      messages: [],
      selectedSkillIds,
      contextFiles: ["浏览器预览数据/example/example.txt"],
      messageCount: 0,
      bodyState: "ready",
    };
    mockAgentSessions = [session, ...mockAgentSessions];
    return structuredClone(session);
  },
  async renameAgentProject(projectId, name) {
    const project = mockAgentProjects.find((item) => item.id === projectId);
    if (!project) throw new Error("Agent 项目不存在");
    Object.assign(project, { name: name.trim() || project.name, updatedAt: Date.now() });
    return structuredClone(project);
  },
  async listAgentSessions(projectId) {
    return structuredClone(mockAgentSessions
      .filter((session) => projectId === undefined
        || (projectId === "" ? !session.projectId : session.projectId === projectId))
      .sort((left, right) => right.updatedAt - left.updatedAt)
      .map((session) => ({
        id: session.id,
        title: session.title,
        projectId: session.projectId || null,
        createdAt: session.createdAt,
        updatedAt: session.updatedAt,
        revision: session.revision ?? 1,
        messageCount: session.messages.length,
        selectedSkillIds: session.selectedSkillIds,
      })));
  },
  async createAgentSession(projectId = "", title = "新对话", selectedSkillIds = []) {
    const now = Date.now();
    const session: AgentConversationSession = {
      id: `agent-${now}-${Math.random().toString(16).slice(2)}`,
      title,
      projectId,
      createdAt: now,
      updatedAt: now,
      revision: 1,
      messages: [],
      selectedSkillIds,
      contextFiles: [],
      messageCount: 0,
      bodyState: "ready",
    };
    mockAgentSessions = [session, ...mockAgentSessions];
    return structuredClone(session);
  },
  async getAgentSession(sessionId) {
    const session = mockAgentSessions.find((item) => item.id === sessionId);
    if (!session) throw new Error("Agent 会话不存在");
    return structuredClone(session);
  },
  async saveAgentSession(session) {
    const existing = mockAgentSessions.find((item) => item.id === session.id);
    if (existing && (session.revision ?? 0) !== (existing.revision ?? 1)) {
      throw new Error(`Agent 会话版本冲突：客户端 revision=${session.revision ?? 0}，数据库 revision=${existing.revision ?? 1}`);
    }
    if (!existing && (session.revision ?? 0) > 0) {
      throw new Error("Agent 会话已删除，已阻止旧快照重新创建会话");
    }
    const normalized = {
      ...structuredClone(session),
      projectId: session.projectId ?? "",
      selectedSkillIds: session.selectedSkillIds ?? [],
      contextFiles: session.contextFiles ?? [],
      revision: (existing?.revision ?? 0) + 1,
      messageCount: session.messages.length,
      bodyState: "ready" as const,
    };
    mockAgentSessions = mockAgentSessions.some((item) => item.id === session.id)
      ? mockAgentSessions.map((item) => item.id === session.id ? normalized : item)
      : [normalized, ...mockAgentSessions];
    return structuredClone(normalized);
  },
  async renameAgentSession(sessionId, title) {
    const session = await this.getAgentSession(sessionId);
    return this.saveAgentSession({ ...session, title, updatedAt: Date.now() });
  },
  async moveAgentSession(sessionId, projectId = "") {
    const session = await this.getAgentSession(sessionId);
    return this.saveAgentSession({ ...session, projectId, contextFiles: [], updatedAt: Date.now() });
  },
  async deleteAgentSession(sessionId) {
    mockAgentSessions = mockAgentSessions.filter((session) => session.id !== sessionId);
    mockAgentAttachments.delete(sessionId);
    mockAgentArtifacts.delete(sessionId);
  },
  async listAgentExtensions() {
    return structuredClone(mockAgentExtensions);
  },
  async selectAgentExtensionPackage() {
    return null;
  },
  async installAgentExtension(packagePath) {
    const fileName = packagePath.split(/[\\/]/).at(-1) ?? "local-extension.js";
    const id = fileName.replace(/\.(?:m?js|tgz)$/i, "").toLowerCase().replace(/[^a-z0-9]+/g, "-");
    const extension: AgentExtensionSummary = {
      id,
      name: fileName,
      version: "local",
      description: "浏览器预览中的本地 QuickJS 扩展",
      enabled: true,
      runtime: "QuickJS",
      source: fileName.endsWith(".tgz") ? "npm-tgz-offline" : "local-js",
      integrity: "browser-preview",
      directory: `浏览器预览数据/agent/extensions/${id}`,
      tools: [],
    };
    mockAgentExtensions = [...mockAgentExtensions.filter((item) => item.id !== id), extension];
    return structuredClone(extension);
  },
  async setAgentExtensionEnabled(extensionId, enabled) {
    const extension = mockAgentExtensions.find((item) => item.id === extensionId);
    if (!extension) throw new Error("扩展不存在");
    extension.enabled = enabled;
    return structuredClone(extension);
  },
  async removeAgentExtension(extensionId) {
    if (extensionId === "drpa-quickjs-example") throw new Error("内置扩展不能卸载");
    mockAgentExtensions = mockAgentExtensions.filter((item) => item.id !== extensionId);
  },
  async getRuntimeStatus() {
    return {
      state: "ready", bundleVersion: "浏览器预览", pythonVersion: "3.11.9",
      runtimeRoot: "内存预览", environmentRoot: "内存预览", browserExecutable: "内存预览",
      message: "浏览器预览使用模拟运行环境", profileId: "org.drpa.python-runtime",
      profileName: "Python 3.11 Full", features: ["jupyter", "documents", "browser.drissionpage"],
      profiles: [{
        id: "org.drpa.python-runtime", name: "Python 3.11 Full", componentVersion: "preview",
        pythonVersion: "3.11.9", environmentMode: "materialized", features: ["jupyter", "documents", "browser.drissionpage"],
        selected: true, ready: true, inUse: 0, runtimeRoot: "内存预览", environmentRoot: "内存预览",
      }],
    };
  },
  async selectRuntimeProfile() { return this.getRuntimeStatus(); },
  async listRuntimePythonPackages() {
    return {
      profileId: "org.drpa.python-runtime",
      profileName: "Python 3.11 Full",
      pythonVersion: "3.11.9",
      backend: "uv",
      backendVersion: "uv 0.11.28",
      backendPath: "runtime/tools/uv",
      overlayRoot: "浏览器预览数据/runtime-package-overlays/full/site-packages",
      packages: [
        { name: "pandas", version: "2.3.0", source: "runtime", location: "runtime", removable: false },
        { name: "rich", version: "14.0.0", source: "user", location: "用户包层", removable: true },
      ],
    };
  },
  async installRuntimePythonPackage(requirement) {
    const catalog = await this.listRuntimePythonPackages();
    const name = requirement.split(/[<>=!~\s[]/, 1)[0] || requirement;
    catalog.packages.unshift({ name, version: "preview", source: "user", location: catalog.overlayRoot, removable: true });
    return catalog;
  },
  async uninstallRuntimePythonPackage(packageName) {
    const catalog = await this.listRuntimePythonPackages();
    catalog.packages = catalog.packages.filter((item) => item.name !== packageName);
    return catalog;
  },
  async getSystemMetrics() {
    return {
      sampledAt: Date.now(),
      cpu: { usagePercent: 18.4, logicalCores: 8 },
      memory: { usedBytes: 6_442_450_944, totalBytes: 17_179_869_184, usagePercent: 37.5 },
      disk: {
        usedBytes: 214_748_364_800,
        totalBytes: 536_870_912_000,
        usagePercent: 40,
        volumes: [{
          name: "本地磁盘 C:",
          mountPoint: "C:\\",
          usedBytes: 214_748_364_800,
          totalBytes: 536_870_912_000,
          usagePercent: 40,
        }],
      },
    };
  },
  async getPlatformCapabilities() {
    return {
      os: "windows",
      displayName: "Windows x64",
      runtimeTarget: "windows-x86_64",
      supportsWindowsUpdates: false,
      fileManagerName: "资源管理器",
      dataDirectoryPolicy: "安装目录 data",
      reducedVisualEffects: false,
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
  async exportUserData(targetPath) {
    return { path: targetPath, fileCount: 12, totalBytes: 65_536, workspaceName: "浏览器预览", restartRequired: false };
  },
  async importUserData(sourcePath) {
    return { path: sourcePath, fileCount: 12, totalBytes: 65_536, workspaceName: "导入 · 浏览器预览", restartRequired: true };
  },
  async getCurrentUser() {
    return { displayName: "本地用户", accountName: "browser-preview", initials: "本地" };
  },
  async listKnowledgeEntries() {
    return structuredClone(mockKnowledgeEntries());
  },
  async listKnowledgeBases() {
    return structuredClone(mockKnowledgeBases);
  },
  async createKnowledgeBase(name, description) {
    const now = Date.now();
    const library: KnowledgeBaseSummary = {
      id: `kb-${now}`,
      name,
      description,
      sourceCount: 0,
      chunkCount: 0,
      status: "ready",
      updatedAt: now,
    };
    mockKnowledgeBases = [library, ...mockKnowledgeBases];
    return structuredClone(library);
  },
  async deleteKnowledgeBase(knowledgeBaseId) {
    mockKnowledgeBases = mockKnowledgeBases.filter((library) => library.id !== knowledgeBaseId);
    mockKnowledgeBaseSources = mockKnowledgeBaseSources.filter((source) => source.knowledgeBaseId !== knowledgeBaseId);
  },
  async listKnowledgeBaseSources(knowledgeBaseId) {
    return structuredClone(mockKnowledgeBaseSources.filter((source) => source.knowledgeBaseId === knowledgeBaseId));
  },
  async importKnowledgeBaseFiles(knowledgeBaseId, paths) {
    const now = Date.now();
    const imported = paths.map((path, index): KnowledgeBaseSource => ({
      id: `source-${now}-${index}`,
      knowledgeBaseId,
      name: path.split(/[\\/]/).at(-1) ?? "资料",
      kind: "file",
      status: "ready",
      chunkCount: 1,
      sizeBytes: 1024,
      uri: path,
      lastError: "",
      updatedAt: now,
    }));
    mockKnowledgeBaseSources = [...imported, ...mockKnowledgeBaseSources];
    mockKnowledgeBases = mockKnowledgeBases.map((library) => library.id === knowledgeBaseId ? {
      ...library,
      sourceCount: library.sourceCount + imported.length,
      chunkCount: library.chunkCount + imported.length,
      updatedAt: now,
    } : library);
    return structuredClone(imported);
  },
  async importKnowledgeBaseDirectory(knowledgeBaseId, directoryPath) {
    return this.importKnowledgeBaseFiles(knowledgeBaseId, [`${directoryPath}/资料.md`]);
  },
  async addKnowledgeBaseText(knowledgeBaseId, title, content) {
    const [source] = await this.importKnowledgeBaseFiles(knowledgeBaseId, [title]);
    return { ...source!, kind: "text", sizeBytes: new Blob([content]).size };
  },
  async addKnowledgeBaseUrl(knowledgeBaseId, url) {
    const [source] = await this.importKnowledgeBaseFiles(knowledgeBaseId, [url]);
    return { ...source!, kind: "url", uri: url };
  },
  async deleteKnowledgeBaseSource(knowledgeBaseId, sourceId) {
    mockKnowledgeBaseSources = mockKnowledgeBaseSources.filter((source) => !(source.knowledgeBaseId === knowledgeBaseId && source.id === sourceId));
  },
  async searchKnowledgeBase(knowledgeBaseIds, query, limit) {
    return mockKnowledgeBaseSources
      .filter((source) => knowledgeBaseIds.includes(source.knowledgeBaseId))
      .slice(0, limit)
      .map((source, index): KnowledgeBaseSearchResult => {
        const library = mockKnowledgeBases.find((item) => item.id === source.knowledgeBaseId);
        return {
          knowledgeBaseId: source.knowledgeBaseId,
          knowledgeBaseName: library?.name ?? "知识库",
          sourceId: source.id,
          sourceName: source.name,
          chunkId: `${source.id}-chunk-${index + 1}`,
          content: `这是与“${query}”相关的浏览器预览知识片段。`,
          citation: `${library?.name ?? "知识库"} / ${source.name} #${index + 1}`,
          score: 0.86 - index * 0.03,
          vectorScore: 0.88 - index * 0.03,
          keywordScore: 0.8 - index * 0.03,
        };
      });
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
  async savePluginConfig(_pluginId, config) {
    mockPluginConfig = {
      ...config,
      dify_api_key: typeof config.dify_api_key === "string" && config.dify_api_key
        ? config.dify_api_key
        : mockPluginConfig.dify_api_key,
      proxy_api_key: typeof config.proxy_api_key === "string" && config.proxy_api_key
        ? config.proxy_api_key
        : mockPluginConfig.proxy_api_key,
    };
  },
  async setPluginEnabled(_pluginId, enabled) { mockPluginEnabled = enabled; if (!enabled) mockPluginRunning = false; },
  async startPlugin() { mockPluginEnabled = true; mockPluginRunning = true; },
  async stopPlugin() { mockPluginRunning = false; },
  async uninstallPlugin() { mockPluginEnabled = false; mockPluginRunning = false; },
  async getPluginLogs() {
    return mockPluginRunning
      ? [{
          timestamp: Date.now(),
          stream: "stdout",
          message: "Dify2API gateway is listening",
          serviceId: "gateway",
          event: {
            kind: "service.ready",
            title: "Dify2API 已就绪",
            detail: { endpoint: mockPlugins()[0].endpoint },
          },
        }]
      : [];
  },
  async testPluginConnection() {
    return {
      ok: true,
      message: "Dify 上游连接成功",
      duration_ms: 18,
      details: { service: "dify2api", model: mockPluginConfig.model_name ?? "dify-agent" },
    };
  },
  async runPluginDebugger(_pluginId, endpointId, request): Promise<PluginDebuggerResponse> {
    const model = typeof mockPluginConfig.model_name === "string"
      ? mockPluginConfig.model_name
      : "dify-agent";
    const common = {
      endpointId,
      status: 200,
      durationMs: 12,
      contentType: "application/json",
      truncated: false,
    };
    if (endpointId === "health") {
      return {
        ...common,
        body: { status: "ok", service: "dify2api", model, tool_emulation: true },
      };
    }
    if (endpointId === "upstream") {
      return {
        ...common,
        body: { ok: true, message: "Dify API is reachable", duration_ms: 18 },
      };
    }
    if (endpointId === "models") {
      return {
        ...common,
        body: { object: "list", data: [{ id: model, object: "model", owned_by: "dify" }] },
      };
    }
    if (endpointId === "chat" || endpointId === "tool-call") {
      return {
        ...common,
        durationMs: 36,
        body: {
          id: "chatcmpl-browser-preview",
          object: "chat.completion",
          model,
          request: request ?? null,
          choices: endpointId === "tool-call"
            ? [{
              index: 0,
              message: {
                role: "assistant",
                content: null,
                tool_calls: [{
                  id: "call-browser-preview",
                  type: "function",
                  function: { name: "get_time", arguments: "{\"city\":\"上海\"}" },
                }],
              },
              finish_reason: "tool_calls",
            }]
            : [{
              index: 0,
              message: { role: "assistant", content: "这是 Dify2API 插件调试器的浏览器预览响应。" },
              finish_reason: "stop",
            }],
        },
      };
    }
    throw new Error(`插件未声明调试入口：${endpointId}`);
  },
  async listPluginTools() { return []; },
  async invokePluginTool(_pluginId, toolName, input) {
    return {
      ok: true,
      output: { tool: toolName, input: input as unknown as PluginJsonValue },
      durationMs: 3,
    };
  },
  async listPluginProjects() { return mockPluginProjects; },
  async createPluginProject(pluginId, name, projectType) {
    const types = projectType === "tool"
      ? ["tool-provider"]
      : projectType === "service"
        ? ["provider-adapter", "service"]
        : ["provider-adapter", "service", "tool-provider", "debugger"];
    const project = { id: pluginId, name, version: "0.1.0", description: "DRPA 插件项目", types, directory: `浏览器预览数据/plugin-projects/${pluginId}`, valid: true, validationMessage: "插件清单与入口文件有效" };
    mockPluginProjects = [...mockPluginProjects, project];
    return project;
  },
  async validatePluginProject(pluginId) { return mockPluginProjects.find((item) => item.id === pluginId) ?? Promise.reject(new Error("插件项目不存在")); },
  async buildPluginProject(pluginId) { return `浏览器预览数据/build/plugins/${pluginId}-0.1.0.drpa-plugin`; },
};

const tauriGateway: DesktopGateway = {
  listWorkspaces: () => invoke<WorkspaceInfo[]>("list_workspaces"),
  createWorkspace: (name) => invoke<WorkspaceInfo>("create_workspace", { name }),
  switchWorkspace: (workspaceId) => invoke<void>("switch_workspace", { workspaceId }),
  getWorkspaceSnapshot: () => invoke<WorkspaceSnapshot>("get_workspace_snapshot"),
  reportUiReady: () => invoke<void>("report_ui_ready"),
  completeStartup: () => invoke<void>("complete_startup"),
  reportUiInputReady: () => invoke<void>("report_ui_input_ready"),
  installPackage: (archivePath) => invoke<PackageSummary>("install_package", { archivePath }),
  uninstallPackage: (packageId) => invoke<void>("uninstall_package", { packageId }),
  startRun: (packageId, profileId, parameters) => invoke<string>("start_run", { packageId, profileId, parameters }),
  cancelRun: (runId) => invoke<void>("cancel_run", { runId }),
  saveOpticalReceivedFile: (targetPath, payloadBase64) => invoke<string>("save_optical_received_file", { targetPath, payloadBase64 }),
  getRunDetail: (runId) => invoke<RunDetail>("get_run_detail", { runId }),
  openRunOutputDirectory: (runId) => invoke<void>("open_run_output_directory", { runId }),
  listAutomationPlans: () => invoke<AutomationPlan[]>("list_automation_plans"),
  createAutomationPlan: (input) => invoke<AutomationPlan>("create_automation_plan", { input }),
  updateAutomationPlan: (input) => invoke<AutomationPlan>("update_automation_plan", { input }),
  deleteAutomationPlan: (planId) => invoke<void>("delete_automation_plan", { planId }),
  setAutomationPlanEnabled: (planId, enabled) => invoke<AutomationPlan>("set_automation_plan_enabled", { planId, enabled }),
  runAutomationPlanNow: (planId) => invoke<AutomationRun>("run_automation_plan_now", { planId }),
  listAutomationRuns: (planId, limit) => invoke<AutomationRun[]>("list_automation_runs", { planId, limit }),
  listStudioProjects: () => invoke<StudioProject[]>("list_studio_projects"),
  createStudioProject: (name) => invoke<StudioProject>("create_studio_project", { name }),
  renameStudioProject: (projectId, name) => invoke<StudioProject>("rename_studio_project", { projectId, name }),
  openInstalledPackage: (packageId) => invoke<StudioProject>("open_installed_package", { packageId }),
  readProjectFile: (projectId, relativePath) => invoke<string>("read_project_file", { projectId, relativePath }),
  writeProjectFile: (projectId, relativePath, content) => invoke<void>("write_project_file", { projectId, relativePath, content }),
  createProjectDirectory: (projectId, relativePath) => invoke<void>("create_project_directory", { projectId, relativePath }),
  renameProjectEntry: (projectId, relativePath, targetPath) => invoke<void>("rename_project_entry", { projectId, relativePath, targetPath }),
  deleteProjectEntry: (projectId, relativePath) => invoke<void>("delete_project_entry", { projectId, relativePath }),
  deleteStudioProject: (projectId) => invoke<void>("delete_studio_project", { projectId }),
  importProjectFile: (projectId, sourcePath, targetDirectory) => invoke<string>("import_project_file", { projectId, sourcePath, targetDirectory }),
  buildStudioProject: (projectId) => invoke<string>("build_studio_project", { projectId }),
  installStudioProject: (projectId) => invoke<PackageSummary>("install_studio_project", { projectId }),
  openBuildOutputDirectory: () => invoke<void>("open_build_output_directory"),
  runStudioProject: (projectId, parameters) => invoke<string>("run_studio_project", { projectId, parameters }),
  executeStudioCell: (projectId, code) => invoke<StudioCellResult>("execute_studio_cell", { projectId, code }),
  completeStudioPython: (projectId, code, cursorPos) => invoke<StudioCompletionResult>("complete_studio_python", { projectId, code, cursorPos }),
  inspectStudioPython: (projectId, code, cursorPos, detailLevel) => invoke<StudioInspectResult>("inspect_studio_python", { projectId, code, cursorPos, detailLevel }),
  parsePythonFlow: (source, sourceName) => invoke<PythonFlowGraph>("parse_python_flow", { source, sourceName }),
  renderPythonFlow: (flow) => invoke<string>("render_python_flow", { flow }),
  validatePythonFlow: (flow) => invoke<PythonFlowValidationResult>("validate_python_flow", { flow }),
  prepareStudioKernel: (projectId) => invoke<void>("prepare_studio_kernel", { projectId }),
  restartStudioKernel: (projectId) => invoke<void>("restart_studio_kernel", { projectId }),
  getWorkspaceDatabaseInfo: () => invoke<DatabaseInfo>("get_workspace_database_info"),
  listDatabaseTables: () => invoke<DatabaseTable[]>("list_database_tables"),
  describeDatabaseTable: (tableName) => invoke<DatabaseColumn[]>("describe_database_table", { tableName }),
  executeDatabaseSql: (sql, offset, limit) => invoke<DatabaseQueryResult>("execute_database_sql", { sql, offset, limit }),
  selectDatabaseExportPath: async (format, suggestedName) => {
    const labels: Record<DatabaseExportFormat, string> = {
      xlsx: "Excel 工作簿",
      xls: "Excel 97-2003 XML",
      csv: "CSV 数据",
      json: "JSON 数据",
      sql: "SQL INSERT 脚本",
    };
    const selected = await import("@tauri-apps/plugin-dialog").then(({ save }) => save({
      defaultPath: `${suggestedName}.${format}`,
      filters: [{ name: labels[format], extensions: [format] }],
    }));
    return selected ?? null;
  },
  exportDatabaseQueryResult: (result, format, targetPath, tableName) => invoke<DatabaseExportResult>("export_database_query_result", { result, format, targetPath, tableName }),
  getDatabaseSchemaContext: () => invoke<string>("get_database_schema_context"),
  openWorkspaceDatabaseDirectory: () => invoke<void>("open_workspace_database_directory"),
  selectDatabaseSourceFile: async (engine) => {
    const excel = engine === "excel";
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{
        name: excel ? "Excel 工作簿" : "SQLite 数据库",
        extensions: excel ? ["xls", "xlsx", "xlsb", "ods"] : ["db", "sqlite", "sqlite3"],
      }],
    });
    return typeof selected === "string" ? selected : null;
  },
  listRemoteDatabaseProfiles: () => invoke<RemoteDatabaseProfile[]>("list_remote_database_profiles"),
  saveRemoteDatabaseProfile: (profile) => invoke<RemoteDatabaseProfile>("save_remote_database_profile", { profile }),
  deleteRemoteDatabaseProfile: (profileId) => invoke<void>("delete_remote_database_profile", { profileId }),
  testRemoteDatabaseConnection: (profile, password) => invoke<RemoteConnectionTest>("test_remote_database_connection", { profile, password }),
  listRemoteDatabaseTables: (profileId, password) => invoke<DatabaseTable[]>("list_remote_database_tables", { profileId, password }),
  describeRemoteDatabaseTable: (profileId, password, tableName) => invoke<DatabaseColumn[]>("describe_remote_database_table", { profileId, password, tableName }),
  executeRemoteDatabaseSql: (profileId, password, sql, offset, limit) => invoke<DatabaseQueryResult>("execute_remote_database_sql", { profileId, password, sql, offset, limit }),
  getRemoteDatabaseSchemaContext: (profileId, password) => invoke<string>("get_remote_database_schema_context", { profileId, password }),
  getBiDashboard: () => invoke<DashboardDocument>("get_bi_dashboard"),
  saveBiDashboard: (document) => invoke<DashboardDocument>("save_bi_dashboard", { document }),
  resetBiDashboard: () => invoke<DashboardDocument>("reset_bi_dashboard"),
  executeDashboardDatabaseQuery: (profileId, password, sql) => invoke<DatabaseQueryResult>("execute_dashboard_database_query", { profileId, password, sql }),
  getVaultStatus: () => invoke<VaultStatus>("get_vault_status"),
  beginVaultSetup: () => invoke<VaultSetup>("begin_vault_setup"),
  completeVaultSetup: (setupId, code) => invoke<VaultUnlockResult>("complete_vault_setup", { setupId, code }),
  unlockVault: (code) => invoke<VaultUnlockResult>("unlock_vault", { code }),
  unlockVaultWithRecovery: (recoveryCode) => invoke<VaultUnlockResult>("unlock_vault_with_recovery", { recoveryCode }),
  lockVault: () => invoke<void>("lock_vault"),
  listVaultCredentials: () => invoke<VaultCredentialSummary[]>("list_vault_credentials"),
  getVaultCredential: (id) => invoke<VaultCredential>("get_vault_credential", { id }),
  saveVaultCredential: (input) => invoke<VaultCredential>("save_vault_credential", { input }),
  deleteVaultCredential: (id) => invoke<void>("delete_vault_credential", { id }),
  startVaultService: (port) => invoke<VaultServiceStatus>("start_vault_service", { port }),
  stopVaultService: () => invoke<VaultServiceStatus>("stop_vault_service"),
  exportVaultRecoveryCode: (recoveryCode, targetPath) => invoke<string>("export_vault_recovery_code", { recoveryCode, targetPath }),
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
  cancelAgentRun: (requestId) => invoke<AgentRunSnapshot>("cancel_agent_run", { requestId }),
  getAgentRun: (requestId) => invoke<AgentRunSnapshot>("get_agent_run", { requestId }),
  selectAgentDocumentFiles: async () => {
    const selected = await open({
      multiple: true,
      directory: false,
      filters: [{ name: "对话文档", extensions: ["pdf", "docx", "xlsx", "pptx"] }],
    });
    if (!selected) return [];
    return Array.isArray(selected) ? selected : [selected];
  },
  selectAgentArtifactExportPath: async (suggestedName) => {
    const extension = suggestedName.split(".").at(-1) ?? "";
    const selected = await import("@tauri-apps/plugin-dialog").then(({ save }) => save({
      defaultPath: suggestedName,
      filters: extension ? [{ name: "文档产物", extensions: [extension] }] : undefined,
    }));
    return selected ?? null;
  },
  importAgentDocument: (sourcePath, sessionId) => invoke<AgentDocumentAttachment>("import_agent_document", { sourcePath, sessionId }),
  listAgentAttachments: (sessionId) => invoke<AgentDocumentAttachment[]>("list_agent_attachments", { sessionId }),
  deleteAgentAttachment: (sessionId, attachmentId) => invoke<void>("delete_agent_attachment", { sessionId, attachmentId }),
  listAgentArtifacts: (sessionId) => invoke<AgentDocumentArtifact[]>("list_agent_artifacts", { sessionId }),
  exportAgentArtifact: (sessionId, artifactId, destinationPath) => invoke<AgentDocumentExport>("export_agent_artifact", { sessionId, artifactId, destinationPath }),
  listAgentProjects: () => invoke<AgentConversationProject[]>("list_agent_projects"),
  createAgentProject: (name) => invoke<AgentConversationProject>("create_agent_project", { name }),
  openAgentFileSession: async (selectedSkillIds = []) => {
    const selected = await open({ multiple: false, directory: false });
    if (typeof selected !== "string") return null;
    const session = await invoke<AgentConversationSession>("open_agent_file_session", {
      sourcePath: selected,
      selectedSkillIds,
    });
    return { ...session, revision: session.revision ?? 1, bodyState: "ready", projectId: session.projectId ?? "", selectedSkillIds: session.selectedSkillIds ?? [], contextFiles: session.contextFiles ?? [] };
  },
  renameAgentProject: (projectId, name) => invoke<AgentConversationProject>("rename_agent_project", { projectId, name }),
  listAgentSessions: (projectId) => invoke<AgentConversationSessionSummary[]>("list_agent_sessions", { projectId }),
  createAgentSession: async (projectId, title, selectedSkillIds) => {
    const session = await invoke<AgentConversationSession>("create_agent_session", {
      projectId: projectId || null,
      title,
      selectedSkillIds,
    });
    return { ...session, revision: session.revision ?? 1, bodyState: "ready", projectId: session.projectId ?? "", selectedSkillIds: session.selectedSkillIds ?? [], contextFiles: session.contextFiles ?? [] };
  },
  getAgentSession: async (sessionId) => {
    const session = await invoke<AgentConversationSession>("get_agent_session", { sessionId });
    return { ...session, revision: session.revision ?? 1, bodyState: "ready", projectId: session.projectId ?? "", selectedSkillIds: session.selectedSkillIds ?? [], contextFiles: session.contextFiles ?? [] };
  },
  saveAgentSession: async (session) => {
    const saved = await invoke<AgentConversationSession>("save_agent_session", {
      session: { ...session, projectId: session.projectId || null },
    });
    return { ...saved, revision: saved.revision ?? ((session.revision ?? 0) + 1), bodyState: "ready", projectId: saved.projectId ?? "", selectedSkillIds: saved.selectedSkillIds ?? [], contextFiles: saved.contextFiles ?? [] };
  },
  renameAgentSession: async (sessionId, title) => {
    const session = await invoke<AgentConversationSession>("rename_agent_session", { sessionId, title });
    return { ...session, revision: session.revision ?? 1, bodyState: "ready", projectId: session.projectId ?? "", selectedSkillIds: session.selectedSkillIds ?? [], contextFiles: session.contextFiles ?? [] };
  },
  moveAgentSession: async (sessionId, projectId) => {
    const session = await invoke<AgentConversationSession>("move_agent_session", {
      sessionId,
      projectId: projectId || null,
    });
    return { ...session, revision: session.revision ?? 1, bodyState: "ready", projectId: session.projectId ?? "", selectedSkillIds: session.selectedSkillIds ?? [], contextFiles: session.contextFiles ?? [] };
  },
  deleteAgentSession: (sessionId) => invoke<void>("delete_agent_session", { sessionId }),
  listAgentExtensions: () => invoke<AgentExtensionSummary[]>("list_agent_extensions"),
  selectAgentExtensionPackage: async () => {
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "离线 Agent 扩展", extensions: ["js", "mjs", "tgz"] }],
    });
    return typeof selected === "string" ? selected : null;
  },
  installAgentExtension: (packagePath) => invoke<AgentExtensionSummary>("install_agent_extension", { packagePath }),
  setAgentExtensionEnabled: (extensionId, enabled) => invoke<AgentExtensionSummary>("set_agent_extension_enabled", { extensionId, enabled }),
  removeAgentExtension: (extensionId) => invoke<void>("remove_agent_extension", { extensionId }),
  getRuntimeStatus: () => invoke<RuntimeStatus>("get_runtime_status"),
  selectRuntimeProfile: (profileId) => invoke<RuntimeStatus>("select_runtime_profile", { profileId }),
  listRuntimePythonPackages: () => invoke<RuntimePythonPackageCatalog>("list_runtime_python_packages"),
  installRuntimePythonPackage: (requirement) => invoke<RuntimePythonPackageCatalog>("install_runtime_python_package", { requirement }),
  uninstallRuntimePythonPackage: (packageName) => invoke<RuntimePythonPackageCatalog>("uninstall_runtime_python_package", { packageName }),
  getSystemMetrics: () => invoke<SystemMetricsSnapshot>("get_system_metrics"),
  getPlatformCapabilities: () => invoke<PlatformCapabilities>("get_platform_capabilities"),
  initializeRuntime: () => invoke<RuntimeStatus>("initialize_runtime"),
  repairRuntime: () => invoke<RuntimeStatus>("repair_runtime"),
  applyWindowsUpdate: (packagePath) => invoke<WindowsUpdateSession>("apply_windows_update", { packagePath }),
  getWindowsUpdateStatus: (sessionId) => invoke<WindowsUpdateStatus>("get_windows_update_status", { sessionId }),
  getLatestWindowsUpdateStatus: () => invoke<WindowsUpdateStatus | null>("get_latest_windows_update_status"),
  restartForWindowsUpdate: (sessionId) => invoke<void>("restart_for_windows_update", { sessionId }),
  getDataDirectory: () => invoke<string>("get_data_directory"),
  openWorkspaceDataDirectory: () => invoke<void>("open_workspace_data_directory"),
  exportUserData: (targetPath) => invoke<UserDataTransferResult>("export_user_data", { targetPath }),
  importUserData: (sourcePath) => invoke<UserDataTransferResult>("import_user_data", { sourcePath }),
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
  runPluginDebugger: (pluginId, endpointId, request) => invoke<PluginDebuggerResponse>("run_plugin_debugger", { pluginId, endpointId, request }),
  listPluginTools: (pluginId) => invoke<PluginToolDescriptor[]>("list_plugin_tools", { pluginId }),
  invokePluginTool: (pluginId, toolName, input) => invoke<PluginToolWorkbenchResult>("invoke_plugin_tool", { pluginId, toolName, input }),
  listPluginProjects: () => invoke<PluginProjectSummary[]>("list_plugin_projects"),
  createPluginProject: (pluginId, name, projectType) => invoke<PluginProjectSummary>("create_plugin_project", { pluginId, name, projectType }),
  validatePluginProject: (pluginId) => invoke<PluginProjectSummary>("validate_plugin_project", { pluginId }),
  buildPluginProject: (pluginId) => invoke<string>("build_plugin_project", { pluginId }),
  listKnowledgeEntries: () => invoke<KnowledgeEntry[]>("list_knowledge_entries"),
  listKnowledgeBases: () => invoke<KnowledgeBaseSummary[]>("list_knowledge_bases"),
  createKnowledgeBase: (name, description) => invoke<KnowledgeBaseSummary>("create_knowledge_base", { name, description }),
  deleteKnowledgeBase: (knowledgeBaseId) => invoke<void>("delete_knowledge_base", { knowledgeBaseId }),
  listKnowledgeBaseSources: (knowledgeBaseId) => invoke<KnowledgeBaseSource[]>("list_knowledge_base_sources", { knowledgeBaseId }),
  importKnowledgeBaseFiles: (knowledgeBaseId, paths) => invoke<KnowledgeBaseSource[]>("import_knowledge_base_files", { knowledgeBaseId, sourcePaths: paths }),
  importKnowledgeBaseDirectory: (knowledgeBaseId, directoryPath) => invoke<KnowledgeBaseSource[]>("import_knowledge_base_directory", { knowledgeBaseId, directoryPath }),
  addKnowledgeBaseText: (knowledgeBaseId, title, content) => invoke<KnowledgeBaseSource>("add_knowledge_base_text", { knowledgeBaseId, title, content }),
  addKnowledgeBaseUrl: (knowledgeBaseId, url) => invoke<KnowledgeBaseSource>("add_knowledge_base_url", { knowledgeBaseId, url }),
  deleteKnowledgeBaseSource: (knowledgeBaseId, sourceId) => invoke<void>("delete_knowledge_base_source", { knowledgeBaseId, sourceId }),
  searchKnowledgeBase: (knowledgeBaseIds, query, limit) => invoke<KnowledgeBaseSearchResult[]>("search_knowledge_base", { knowledgeBaseIds, query, limit }),
  readKnowledgeFile: (relativePath) => invoke<string>("read_knowledge_file", { relativePath }),
  writeKnowledgeFile: (relativePath, content) => invoke<void>("write_knowledge_file", { relativePath, content }),
  createKnowledgeEntry: (relativePath, kind) => invoke<void>("create_knowledge_entry", { relativePath, kind }),
  renameKnowledgeEntry: (relativePath, targetPath) => invoke<void>("rename_knowledge_entry", { relativePath, targetPath }),
  deleteKnowledgeEntry: (relativePath) => invoke<void>("delete_knowledge_entry", { relativePath }),
  importKnowledgeFiles: (sourcePaths, targetDirectory) => invoke<string[]>("import_knowledge_files", { sourcePaths, targetDirectory }),
  exportKnowledgeFile: (relativePath, targetPath) => invoke<string>("export_knowledge_file", { relativePath, targetPath }),
};

export const desktopGateway: DesktopGateway = isTauriHost() ? tauriGateway : mockGateway;
