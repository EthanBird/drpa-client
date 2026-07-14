import { create } from "zustand";
import { persist } from "zustand/middleware";

import type { NavigationId, WorkspaceSnapshot } from "../domain/models";

interface AppStore {
  activeNavigation: NavigationId;
  commandOpen: boolean;
  theme: "light" | "dark";
  inspectorOpen: boolean;
  dragActive: boolean;
  operationNotice: string;
  agentBaseUrl: string;
  agentModel: string;
  agentProjectId: string;
  selectedPackageId: string;
  selectedProfileId: string;
  snapshot: WorkspaceSnapshot | null;
  setActiveNavigation: (id: NavigationId) => void;
  setCommandOpen: (open: boolean) => void;
  setTheme: (theme: "light" | "dark") => void;
  toggleInspector: () => void;
  selectPackage: (packageId: string, profileId?: string) => void;
  selectProfile: (profileId: string) => void;
  setSnapshot: (snapshot: WorkspaceSnapshot) => void;
  setDragActive: (active: boolean) => void;
  setOperationNotice: (notice: string) => void;
  setAgentBaseUrl: (url: string) => void;
  setAgentModel: (model: string) => void;
  setAgentProjectId: (projectId: string) => void;
}

export const useAppStore = create<AppStore>()(persist((set) => ({
  activeNavigation: "overview",
  commandOpen: false,
  theme: "light",
  inspectorOpen: true,
  dragActive: false,
  operationNotice: "",
  agentBaseUrl: "https://api.openai.com/v1",
  agentModel: "gpt-5.4-mini",
  agentProjectId: "",
  selectedPackageId: "",
  selectedProfileId: "",
  snapshot: null,
  setActiveNavigation: (activeNavigation) => set({ activeNavigation }),
  setCommandOpen: (commandOpen) => set({ commandOpen }),
  setTheme: (theme) => set({ theme }),
  toggleInspector: () => set((state) => ({ inspectorOpen: !state.inspectorOpen })),
  selectPackage: (selectedPackageId, selectedProfileId) =>
    set((state) => ({
      selectedPackageId,
      selectedProfileId: selectedProfileId ?? state.selectedProfileId,
    })),
  selectProfile: (selectedProfileId) => set({ selectedProfileId }),
  setSnapshot: (snapshot) => set({ snapshot }),
  setDragActive: (dragActive) => set({ dragActive }),
  setOperationNotice: (operationNotice) => set({ operationNotice }),
  setAgentBaseUrl: (agentBaseUrl) => set({ agentBaseUrl }),
  setAgentModel: (agentModel) => set({ agentModel }),
  setAgentProjectId: (agentProjectId) => set({ agentProjectId }),
}), {
  name: "drpa-ui-preferences",
  partialize: (state) => ({
    theme: state.theme,
    agentBaseUrl: state.agentBaseUrl,
    agentModel: state.agentModel,
    agentProjectId: state.agentProjectId,
  }),
}));
