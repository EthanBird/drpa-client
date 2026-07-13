import { invoke } from "@tauri-apps/api/core";

import { mockSnapshot } from "../data/mockSnapshot";
import type { PackageSummary, StudioCellResult, StudioProject, WorkspaceSnapshot } from "../domain/models";

export interface DesktopGateway {
  getWorkspaceSnapshot(): Promise<WorkspaceSnapshot>;
  installPackage(archivePath: string): Promise<PackageSummary>;
  startRun(packageId: string, profileId: string, parameters: Record<string, unknown>): Promise<string>;
  cancelRun(runId: string): Promise<void>;
  listStudioProjects(): Promise<StudioProject[]>;
  createStudioProject(name: string): Promise<StudioProject>;
  openInstalledPackage(packageId: string): Promise<StudioProject>;
  readProjectFile(projectId: string, relativePath: string): Promise<string>;
  writeProjectFile(projectId: string, relativePath: string, content: string): Promise<void>;
  buildStudioProject(projectId: string): Promise<string>;
  runStudioProject(projectId: string, parameters: Record<string, unknown>): Promise<string>;
  executeStudioCell(projectId: string, code: string): Promise<StudioCellResult>;
  restartStudioKernel(projectId: string): Promise<void>;
  getDataDirectory(): Promise<string>;
}

function isTauriHost(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

const mockGateway: DesktopGateway = {
  async getWorkspaceSnapshot() {
    return structuredClone(mockSnapshot);
  },
  async installPackage() {
    throw new Error("浏览器预览模式不能读取本地 rpaz，请在桌面应用中测试安装。 ");
  },
  async startRun() {
    await new Promise((resolve) => window.setTimeout(resolve, 260));
    return `run-${Date.now()}`;
  },
  async cancelRun() {
    await new Promise((resolve) => window.setTimeout(resolve, 180));
  },
  async listStudioProjects() {
    return [];
  },
  async createStudioProject(name) {
    return { id: `project-${Date.now().toString(16).padStart(24, "0").slice(-24)}`, name, files: ["main.py", "manifest.yaml", "notebook.ipynb"] };
  },
  async openInstalledPackage(packageId) {
    const item = mockSnapshot.packages.find((candidate) => candidate.id === packageId);
    return { id: "project-000000000000000000000001", name: item?.name ?? packageId, files: ["main.py", "manifest.yaml", "notebook.ipynb"] };
  },
  async readProjectFile(_projectId, relativePath) {
    if (relativePath === "manifest.yaml") return "schema: 2\n";
    if (relativePath.endsWith(".ipynb")) return '{"cells":[],"metadata":{},"nbformat":4,"nbformat_minor":5}\n';
    return "def main(ctx):\n    ctx.log.info('你好，DRPA')\n";
  },
  async writeProjectFile() {},
  async buildStudioProject(projectId) {
    return `${projectId}.rpaz`;
  },
  async runStudioProject() {
    return `run-${Date.now()}`;
  },
  async executeStudioCell(_projectId, code) {
    return { executionCount: 1, stdout: "", stderr: "", result: `预览模式：${code.length} 个字符`, traceback: [], variables: [], durationMs: 1 };
  },
  async restartStudioKernel() {},
  async getDataDirectory() {
    return "浏览器预览数据（内存）";
  },
};

const tauriGateway: DesktopGateway = {
  getWorkspaceSnapshot: () => invoke<WorkspaceSnapshot>("get_workspace_snapshot"),
  installPackage: (archivePath) => invoke<PackageSummary>("install_package", { archivePath }),
  startRun: (packageId, profileId, parameters) => invoke<string>("start_run", { packageId, profileId, parameters }),
  cancelRun: (runId) => invoke<void>("cancel_run", { runId }),
  listStudioProjects: () => invoke<StudioProject[]>("list_studio_projects"),
  createStudioProject: (name) => invoke<StudioProject>("create_studio_project", { name }),
  openInstalledPackage: (packageId) => invoke<StudioProject>("open_installed_package", { packageId }),
  readProjectFile: (projectId, relativePath) => invoke<string>("read_project_file", { projectId, relativePath }),
  writeProjectFile: (projectId, relativePath, content) => invoke<void>("write_project_file", { projectId, relativePath, content }),
  buildStudioProject: (projectId) => invoke<string>("build_studio_project", { projectId }),
  runStudioProject: (projectId, parameters) => invoke<string>("run_studio_project", { projectId, parameters }),
  executeStudioCell: (projectId, code) => invoke<StudioCellResult>("execute_studio_cell", { projectId, code }),
  restartStudioKernel: (projectId) => invoke<void>("restart_studio_kernel", { projectId }),
  getDataDirectory: () => invoke<string>("get_data_directory"),
};

export const desktopGateway: DesktopGateway = isTauriHost() ? tauriGateway : mockGateway;
