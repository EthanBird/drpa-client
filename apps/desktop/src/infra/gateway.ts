import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { mockSnapshot } from "../data/mockSnapshot";
import type {
  AgentTurnRequest,
  AgentTurnResult,
  AgentStreamEvent,
  AgentWorkspaceConfig,
  CurrentUser,
  DatabaseColumn,
  DatabaseInfo,
  DatabaseQueryResult,
  DatabaseTable,
  KnowledgeEntry,
  PackageSummary,
  PlatformCapabilities,
  RuntimeStatus,
  RunDetail,
  StudioCellResult,
  StudioCompletionResult,
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
  prepareStudioKernel(projectId: string): Promise<void>;
  restartStudioKernel(projectId: string): Promise<void>;
  getWorkspaceDatabaseInfo(): Promise<DatabaseInfo>;
  listDatabaseTables(): Promise<DatabaseTable[]>;
  describeDatabaseTable(tableName: string): Promise<DatabaseColumn[]>;
  executeDatabaseSql(sql: string): Promise<DatabaseQueryResult>;
  openWorkspaceDatabaseDirectory(): Promise<void>;
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
  writeAgentSkill(name: string, content: string): Promise<void>;
  deleteAgentSkill(name: string): Promise<void>;
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
const mockAgentSkills = new Map<string, string>([[
  "rpaz-development",
  "---\nname: rpaz-development\ndescription: 创建、修改、校验或构建 RPAZ 脚本包时使用。\n---\n\n# RPAZ Development\n",
]]);
const mockAgentStreamListeners = new Map<string, (event: AgentStreamEvent) => void>();

function mockAgentWorkspaceConfig(): AgentWorkspaceConfig {
  return {
    agentsMarkdown: mockAgentAgentsMarkdown,
    memoryMarkdown: mockAgentMemoryMarkdown,
    skills: Array.from(mockAgentSkills, ([name, content]) => ({
      name,
      description: content.match(/^description:\s*(.+)$/m)?.[1] ?? "未填写描述",
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
  async openWorkspaceDatabaseDirectory() {},
  async runAgentTurn(request) {
    const prompt = request.messages.at(-1)?.content ?? "";
    const message = request.projectId
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
  async writeAgentSkill(name, content) {
    mockAgentSkills.set(name, content);
  },
  async deleteAgentSkill(name) {
    mockAgentSkills.delete(name);
  },
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
  prepareStudioKernel: (projectId) => invoke<void>("prepare_studio_kernel", { projectId }),
  restartStudioKernel: (projectId) => invoke<void>("restart_studio_kernel", { projectId }),
  getWorkspaceDatabaseInfo: () => invoke<DatabaseInfo>("get_workspace_database_info"),
  listDatabaseTables: () => invoke<DatabaseTable[]>("list_database_tables"),
  describeDatabaseTable: (tableName) => invoke<DatabaseColumn[]>("describe_database_table", { tableName }),
  executeDatabaseSql: (sql) => invoke<DatabaseQueryResult>("execute_database_sql", { sql }),
  openWorkspaceDatabaseDirectory: () => invoke<void>("open_workspace_database_directory"),
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
  writeAgentSkill: (name, content) => invoke<void>("write_agent_skill", { name, content }),
  deleteAgentSkill: (name) => invoke<void>("delete_agent_skill", { name }),
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
