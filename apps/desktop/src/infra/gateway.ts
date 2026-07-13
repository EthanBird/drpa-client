import { invoke } from "@tauri-apps/api/core";

import { mockSnapshot } from "../data/mockSnapshot";
import type { WorkspaceSnapshot } from "../domain/models";

export interface DesktopGateway {
  getWorkspaceSnapshot(): Promise<WorkspaceSnapshot>;
  startRun(packageId: string, profileId: string): Promise<string>;
  cancelRun(runId: string): Promise<void>;
}

function isTauriHost(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

const mockGateway: DesktopGateway = {
  async getWorkspaceSnapshot() {
    return structuredClone(mockSnapshot);
  },
  async startRun() {
    await new Promise((resolve) => window.setTimeout(resolve, 260));
    return `run-${Date.now()}`;
  },
  async cancelRun() {
    await new Promise((resolve) => window.setTimeout(resolve, 180));
  },
};

const tauriGateway: DesktopGateway = {
  getWorkspaceSnapshot: () => invoke<WorkspaceSnapshot>("get_workspace_snapshot"),
  startRun: (packageId, profileId) => invoke<string>("start_run", { packageId, profileId }),
  cancelRun: (runId) => invoke<void>("cancel_run", { runId }),
};

export const desktopGateway: DesktopGateway = isTauriHost() ? tauriGateway : mockGateway;
