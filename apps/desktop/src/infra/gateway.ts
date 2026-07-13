import { invoke } from "@tauri-apps/api/core";

import { mockSnapshot } from "../data/mockSnapshot";
import type { PackageSummary, StudioProject, WorkspaceSnapshot } from "../domain/models";

export interface DesktopGateway {
  getWorkspaceSnapshot(): Promise<WorkspaceSnapshot>;
  installPackage(archivePath: string): Promise<PackageSummary>;
  startRun(packageId: string, profileId: string, parameters: Record<string, unknown>): Promise<string>;
  cancelRun(runId: string): Promise<void>;
  listStudioProjects(): Promise<StudioProject[]>;
  createStudioProject(projectId: string, name: string): Promise<StudioProject>;
  readProjectFile(projectId: string, relativePath: string): Promise<string>;
  writeProjectFile(projectId: string, relativePath: string, content: string): Promise<void>;
  buildStudioProject(projectId: string): Promise<string>;
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
  async createStudioProject(projectId, name) {
    return { id: projectId, name, files: ["main.py", "manifest.yaml"] };
  },
  async readProjectFile(_projectId, relativePath) {
    return relativePath === "manifest.yaml" ? "schema: 2\n" : "def main(ctx):\n    ctx.log.info('你好，DRPA')\n";
  },
  async writeProjectFile() {},
  async buildStudioProject(projectId) {
    return `${projectId}.rpaz`;
  },
};

const tauriGateway: DesktopGateway = {
  getWorkspaceSnapshot: () => invoke<WorkspaceSnapshot>("get_workspace_snapshot"),
  installPackage: (archivePath) => invoke<PackageSummary>("install_package", { archivePath }),
  startRun: (packageId, profileId, parameters) => invoke<string>("start_run", { packageId, profileId, parameters }),
  cancelRun: (runId) => invoke<void>("cancel_run", { runId }),
  listStudioProjects: () => invoke<StudioProject[]>("list_studio_projects"),
  createStudioProject: (projectId, name) => invoke<StudioProject>("create_studio_project", { projectId, name }),
  readProjectFile: (projectId, relativePath) => invoke<string>("read_project_file", { projectId, relativePath }),
  writeProjectFile: (projectId, relativePath, content) => invoke<void>("write_project_file", { projectId, relativePath, content }),
  buildStudioProject: (projectId) => invoke<string>("build_studio_project", { projectId }),
};

export const desktopGateway: DesktopGateway = isTauriHost() ? tauriGateway : mockGateway;
