import { create } from "zustand";
import { persist } from "zustand/middleware";

import type { NavigationId, WorkspaceSnapshot } from "../domain/models";

interface AppStore {
  activeNavigation: NavigationId;
  commandOpen: boolean;
  compactMode: boolean;
  inspectorOpen: boolean;
  selectedPackageId: string;
  selectedProfileId: string;
  snapshot: WorkspaceSnapshot | null;
  setActiveNavigation: (id: NavigationId) => void;
  setCommandOpen: (open: boolean) => void;
  toggleCompactMode: () => void;
  toggleInspector: () => void;
  selectPackage: (packageId: string, profileId?: string) => void;
  selectProfile: (profileId: string) => void;
  setSnapshot: (snapshot: WorkspaceSnapshot) => void;
}

export const useAppStore = create<AppStore>()(persist((set) => ({
  activeNavigation: "overview",
  commandOpen: false,
  compactMode: false,
  inspectorOpen: true,
  selectedPackageId: "",
  selectedProfileId: "",
  snapshot: null,
  setActiveNavigation: (activeNavigation) => set({ activeNavigation }),
  setCommandOpen: (commandOpen) => set({ commandOpen }),
  toggleCompactMode: () => set((state) => ({ compactMode: !state.compactMode })),
  toggleInspector: () => set((state) => ({ inspectorOpen: !state.inspectorOpen })),
  selectPackage: (selectedPackageId, selectedProfileId) =>
    set((state) => ({
      selectedPackageId,
      selectedProfileId: selectedProfileId ?? state.selectedProfileId,
    })),
  selectProfile: (selectedProfileId) => set({ selectedProfileId }),
  setSnapshot: (snapshot) => set({ snapshot }),
}), {
  name: "drpa-ui-preferences",
  partialize: (state) => ({ compactMode: state.compactMode }),
}));
