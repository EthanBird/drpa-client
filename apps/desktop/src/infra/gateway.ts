import { invoke } from "@tauri-apps/api/core";

import { mockSnapshot } from "../data/mockSnapshot";
import type {
  AgentTurnRequest,
  AgentTurnResult,
  CurrentUser,
  PackageSummary,
  RuntimeStatus,
  StudioCellResult,
  StudioProject,
  WindowsUpdateSession,
  WindowsUpdateStatus,
  WorkspaceSnapshot,
} from "../domain/models";

export interface DesktopGateway {
  getWorkspaceSnapshot(): Promise<WorkspaceSnapshot>;
  installPackage(archivePath: string): Promise<PackageSummary>;
  uninstallPackage(packageId: string): Promise<void>;
  startRun(packageId: string, profileId: string, parameters: Record<string, unknown>): Promise<string>;
  cancelRun(runId: string): Promise<void>;
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
  runStudioProject(projectId: string, parameters: Record<string, unknown>): Promise<string>;
  executeStudioCell(projectId: string, code: string): Promise<StudioCellResult>;
  prepareStudioKernel(projectId: string): Promise<void>;
  restartStudioKernel(projectId: string): Promise<void>;
  runAgentTurn(request: AgentTurnRequest): Promise<AgentTurnResult>;
  getRuntimeStatus(): Promise<RuntimeStatus>;
  initializeRuntime(): Promise<RuntimeStatus>;
  repairRuntime(): Promise<RuntimeStatus>;
  applyWindowsUpdate(packagePath: string): Promise<WindowsUpdateSession>;
  getWindowsUpdateStatus(sessionId: string): Promise<WindowsUpdateStatus>;
  restartForWindowsUpdate(sessionId: string): Promise<void>;
  getDataDirectory(): Promise<string>;
  getCurrentUser(): Promise<CurrentUser>;
}

function isTauriHost(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

const mockStudioProjects: StudioProject[] = [];
const mockStudioContents = new Map<string, Map<string, string>>();

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
  async runStudioProject() {
    return `run-${Date.now()}`;
  },
  async executeStudioCell(_projectId, code) {
    return { executionCount: 1, stdout: "", stderr: "", result: `预览模式：${code.length} 个字符`, traceback: [], outputs: [], variables: [], durationMs: 1 };
  },
  async prepareStudioKernel() {},
  async restartStudioKernel() {},
  async runAgentTurn(request) {
    await new Promise((resolve) => window.setTimeout(resolve, 220));
    const prompt = request.messages.at(-1)?.content ?? "";
    return {
      message: request.projectId
        ? `已连接浏览器预览 Agent。当前问题：${prompt}`
        : `已收到问题：${prompt}\n选择一个开发项目后可启用 RPAZ 工具。`,
      tools: [],
      usage: { promptTokens: 18, completionTokens: 24 },
      durationMs: 220,
    };
  },
  async getRuntimeStatus() {
    return { state: "ready", bundleVersion: "浏览器预览", pythonVersion: "3.11.9", runtimeRoot: "内存预览", environmentRoot: "内存预览", browserExecutable: "内存预览", message: "浏览器预览使用模拟运行环境" };
  },
  async initializeRuntime() { return this.getRuntimeStatus(); },
  async repairRuntime() { return this.getRuntimeStatus(); },
  async applyWindowsUpdate() { throw new Error("浏览器预览不能应用 Windows 更新包"); },
  async getWindowsUpdateStatus() { throw new Error("浏览器预览没有更新会话"); },
  async restartForWindowsUpdate() {},
  async getDataDirectory() {
    return "浏览器预览数据（内存）";
  },
  async getCurrentUser() {
    return { displayName: "本地用户", accountName: "browser-preview", initials: "本地" };
  },
};

const tauriGateway: DesktopGateway = {
  getWorkspaceSnapshot: () => invoke<WorkspaceSnapshot>("get_workspace_snapshot"),
  installPackage: (archivePath) => invoke<PackageSummary>("install_package", { archivePath }),
  uninstallPackage: (packageId) => invoke<void>("uninstall_package", { packageId }),
  startRun: (packageId, profileId, parameters) => invoke<string>("start_run", { packageId, profileId, parameters }),
  cancelRun: (runId) => invoke<void>("cancel_run", { runId }),
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
  runStudioProject: (projectId, parameters) => invoke<string>("run_studio_project", { projectId, parameters }),
  executeStudioCell: (projectId, code) => invoke<StudioCellResult>("execute_studio_cell", { projectId, code }),
  prepareStudioKernel: (projectId) => invoke<void>("prepare_studio_kernel", { projectId }),
  restartStudioKernel: (projectId) => invoke<void>("restart_studio_kernel", { projectId }),
  runAgentTurn: (request) => invoke<AgentTurnResult>("run_agent_turn", { request }),
  getRuntimeStatus: () => invoke<RuntimeStatus>("get_runtime_status"),
  initializeRuntime: () => invoke<RuntimeStatus>("initialize_runtime"),
  repairRuntime: () => invoke<RuntimeStatus>("repair_runtime"),
  applyWindowsUpdate: (packagePath) => invoke<WindowsUpdateSession>("apply_windows_update", { packagePath }),
  getWindowsUpdateStatus: (sessionId) => invoke<WindowsUpdateStatus>("get_windows_update_status", { sessionId }),
  restartForWindowsUpdate: (sessionId) => invoke<void>("restart_for_windows_update", { sessionId }),
  getDataDirectory: () => invoke<string>("get_data_directory"),
  getCurrentUser: () => invoke<CurrentUser>("get_current_user"),
};

export const desktopGateway: DesktopGateway = isTauriHost() ? tauriGateway : mockGateway;
