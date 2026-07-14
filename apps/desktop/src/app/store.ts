import { create } from "zustand";
import { persist } from "zustand/middleware";

import type { AgentConversationMessage, AgentConversationSession, NavigationId, WorkspaceSnapshot } from "../domain/models";

export type FontScale = "small" | "standard" | "large" | "extraLarge";

function createAgentSession(projectId = ""): AgentConversationSession {
  const now = Date.now();
  return {
    id: `agent-${now}-${Math.random().toString(16).slice(2)}`,
    title: "新对话",
    projectId,
    createdAt: now,
    updatedAt: now,
    messages: [],
  };
}

const initialAgentSession = createAgentSession();

interface AppStore {
  activeNavigation: NavigationId;
  commandOpen: boolean;
  theme: "light" | "dark";
  fontScale: FontScale;
  inspectorOpen: boolean;
  dragActive: boolean;
  operationNotice: string;
  agentBaseUrl: string;
  agentModel: string;
  agentApiKey: string;
  agentStreamEnabled: boolean;
  agentContextWindow: number;
  agentMaxOutputTokens: number;
  agentTemperature: number;
  agentProjectId: string;
  agentInspectorOpen: boolean;
  agentSessions: AgentConversationSession[];
  activeAgentSessionId: string;
  selectedPackageId: string;
  selectedProfileId: string;
  snapshot: WorkspaceSnapshot | null;
  setActiveNavigation: (id: NavigationId) => void;
  setCommandOpen: (open: boolean) => void;
  setTheme: (theme: "light" | "dark") => void;
  setFontScale: (fontScale: FontScale) => void;
  toggleInspector: () => void;
  selectPackage: (packageId: string, profileId?: string) => void;
  selectProfile: (profileId: string) => void;
  setSnapshot: (snapshot: WorkspaceSnapshot) => void;
  setDragActive: (active: boolean) => void;
  setOperationNotice: (notice: string) => void;
  setAgentBaseUrl: (url: string) => void;
  setAgentModel: (model: string) => void;
  setAgentApiKey: (apiKey: string) => void;
  setAgentStreamEnabled: (enabled: boolean) => void;
  setAgentContextWindow: (tokens: number) => void;
  setAgentMaxOutputTokens: (tokens: number) => void;
  setAgentTemperature: (temperature: number) => void;
  setAgentProjectId: (projectId: string) => void;
  toggleAgentInspector: () => void;
  createAgentConversation: () => string;
  selectAgentConversation: (sessionId: string) => void;
  deleteAgentConversation: (sessionId: string) => void;
  renameAgentConversation: (sessionId: string, title: string) => void;
  setAgentConversationProject: (sessionId: string, projectId: string) => void;
  setAgentConversationMessages: (sessionId: string, messages: AgentConversationMessage[]) => void;
  clearAgentConversation: (sessionId: string) => void;
}

export const useAppStore = create<AppStore>()(persist((set) => ({
  activeNavigation: "overview",
  commandOpen: false,
  theme: "light",
  fontScale: "standard",
  inspectorOpen: true,
  dragActive: false,
  operationNotice: "",
  agentBaseUrl: "https://api.openai.com/v1",
  agentModel: "gpt-5.4-mini",
  agentApiKey: "",
  agentStreamEnabled: true,
  agentContextWindow: 128000,
  agentMaxOutputTokens: 4096,
  agentTemperature: 0.2,
  agentProjectId: "",
  agentInspectorOpen: true,
  agentSessions: [initialAgentSession],
  activeAgentSessionId: initialAgentSession.id,
  selectedPackageId: "",
  selectedProfileId: "",
  snapshot: null,
  setActiveNavigation: (activeNavigation) => set({ activeNavigation }),
  setCommandOpen: (commandOpen) => set({ commandOpen }),
  setTheme: (theme) => set({ theme }),
  setFontScale: (fontScale) => set({ fontScale }),
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
  setAgentApiKey: (agentApiKey) => set({ agentApiKey }),
  setAgentStreamEnabled: (agentStreamEnabled) => set({ agentStreamEnabled }),
  setAgentContextWindow: (agentContextWindow) => set({ agentContextWindow }),
  setAgentMaxOutputTokens: (agentMaxOutputTokens) => set({ agentMaxOutputTokens }),
  setAgentTemperature: (agentTemperature) => set({ agentTemperature }),
  setAgentProjectId: (agentProjectId) => set({ agentProjectId }),
  toggleAgentInspector: () => set((state) => ({ agentInspectorOpen: !state.agentInspectorOpen })),
  createAgentConversation: () => {
    const session = createAgentSession();
    set((state) => ({
      agentSessions: [session, ...state.agentSessions].slice(0, 50),
      activeAgentSessionId: session.id,
      agentProjectId: "",
    }));
    return session.id;
  },
  selectAgentConversation: (activeAgentSessionId) => set((state) => {
    const session = state.agentSessions.find((item) => item.id === activeAgentSessionId);
    return session ? { activeAgentSessionId, agentProjectId: session.projectId } : {};
  }),
  deleteAgentConversation: (sessionId) => set((state) => {
    let sessions = state.agentSessions.filter((item) => item.id !== sessionId);
    if (sessions.length === 0) sessions = [createAgentSession()];
    const activeAgentSessionId = state.activeAgentSessionId === sessionId
      ? sessions[0].id
      : state.activeAgentSessionId;
    const active = sessions.find((item) => item.id === activeAgentSessionId) ?? sessions[0];
    return { agentSessions: sessions, activeAgentSessionId: active.id, agentProjectId: active.projectId };
  }),
  renameAgentConversation: (sessionId, title) => set((state) => ({
    agentSessions: state.agentSessions.map((session) => session.id === sessionId
      ? { ...session, title: title.trim().slice(0, 60) || "新对话", updatedAt: Date.now() }
      : session),
  })),
  setAgentConversationProject: (sessionId, projectId) => set((state) => ({
    agentProjectId: state.activeAgentSessionId === sessionId ? projectId : state.agentProjectId,
    agentSessions: state.agentSessions.map((session) => session.id === sessionId
      ? { ...session, projectId, updatedAt: Date.now() }
      : session),
  })),
  setAgentConversationMessages: (sessionId, messages) => set((state) => ({
    agentSessions: state.agentSessions.map((session) => session.id === sessionId
      ? { ...session, messages: messages.slice(-120), updatedAt: Date.now() }
      : session),
  })),
  clearAgentConversation: (sessionId) => set((state) => ({
    agentSessions: state.agentSessions.map((session) => session.id === sessionId
      ? { ...session, messages: [], updatedAt: Date.now() }
      : session),
  })),
}), {
  name: "drpa-ui-preferences",
  version: 2,
  partialize: (state) => ({
    theme: state.theme,
    fontScale: state.fontScale,
    agentBaseUrl: state.agentBaseUrl,
    agentModel: state.agentModel,
    agentStreamEnabled: state.agentStreamEnabled,
    agentContextWindow: state.agentContextWindow,
    agentMaxOutputTokens: state.agentMaxOutputTokens,
    agentTemperature: state.agentTemperature,
    agentProjectId: state.agentProjectId,
    agentInspectorOpen: state.agentInspectorOpen,
    agentSessions: state.agentSessions,
    activeAgentSessionId: state.activeAgentSessionId,
  }),
}));
