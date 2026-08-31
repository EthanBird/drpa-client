import { create } from "zustand";
import { persist } from "zustand/middleware";

import type { AgentConversationMessage, AgentConversationSession, AgentMode, AgentProviderRef, AgentToolPolicy, NavigationId, WorkspaceSnapshot } from "../domain/models";
import {
  CONFIGURABLE_NAVIGATION_IDS,
  getVisibleNavigationIds,
  normalizeCustomNavigationIds,
  type NavigationMode,
} from "./navigation";

export type FontScale = number;
export type UiLanguage = "zh-CN" | "en-US";
export type UiDensity = "comfortable" | "compact";

function createAgentSession(projectId = ""): AgentConversationSession {
  const now = Date.now();
  return {
    id: `agent-${now}-${Math.random().toString(16).slice(2)}`,
    title: "新对话",
    projectId,
    createdAt: now,
    updatedAt: now,
    revision: 0,
    messages: [],
    selectedSkillIds: [],
    contextFiles: [],
    messageCount: 0,
    bodyState: "ready",
  };
}

const initialAgentSession = createAgentSession();
const initialWorkspaceId = typeof window === "undefined"
  ? "personal"
  : localStorage.getItem("drpa-active-workspace-id") ?? "personal";

interface AgentWorkspaceState {
  sessions: AgentConversationSession[];
  activeSessionId: string;
  projectId: string;
}

interface AppStore {
  activeNavigation: NavigationId;
  commandOpen: boolean;
  theme: "light" | "dark";
  fontScale: FontScale;
  language: UiLanguage;
  uiDensity: UiDensity;
  hidePageHeaders: boolean;
  navigationMode: NavigationMode;
  customVisibleNavigationIds: NavigationId[];
  collapsedSidebars: Record<string, boolean>;
  inspectorOpen: boolean;
  dragActive: boolean;
  operationNotice: string;
  agentBaseUrl: string;
  agentModel: string;
  agentApiKey: string;
  agentProviderRef: AgentProviderRef | null;
  agentMode: AgentMode;
  agentStreamEnabled: boolean;
  agentContextWindow: number;
  agentMaxOutputTokens: number;
  agentMaxRounds: number;
  agentMaxToolCalls: number;
  agentMaxWallTimeSeconds: number;
  agentTemperature: number;
  agentPythonTimeoutSeconds: number;
  agentToolPolicy: AgentToolPolicy;
  agentProjectId: string;
  agentInspectorOpen: boolean;
  activeWorkspaceId: string;
  workspaceScopeLoaded: boolean;
  agentWorkspaceStates: Record<string, AgentWorkspaceState>;
  agentSessions: AgentConversationSession[];
  activeAgentSessionId: string;
  selectedPackageId: string;
  selectedProfileId: string;
  snapshot: WorkspaceSnapshot | null;
  setActiveNavigation: (id: NavigationId) => void;
  setCommandOpen: (open: boolean) => void;
  setTheme: (theme: "light" | "dark") => void;
  setFontScale: (fontScale: FontScale) => void;
  setLanguage: (language: UiLanguage) => void;
  setUiDensity: (density: UiDensity) => void;
  setHidePageHeaders: (hidden: boolean) => void;
  setNavigationMode: (mode: NavigationMode) => void;
  setNavigationVisible: (id: NavigationId, visible: boolean) => void;
  setSidebarCollapsed: (id: string, collapsed: boolean) => void;
  toggleInspector: () => void;
  selectPackage: (packageId: string, profileId?: string) => void;
  selectProfile: (profileId: string) => void;
  setSnapshot: (snapshot: WorkspaceSnapshot) => void;
  setDragActive: (active: boolean) => void;
  setOperationNotice: (notice: string) => void;
  setAgentBaseUrl: (url: string) => void;
  setAgentModel: (model: string) => void;
  setAgentApiKey: (apiKey: string) => void;
  setAgentProviderRef: (providerRef: AgentProviderRef | null) => void;
  setAgentMode: (mode: AgentMode) => void;
  setAgentStreamEnabled: (enabled: boolean) => void;
  setAgentContextWindow: (tokens: number) => void;
  setAgentMaxOutputTokens: (tokens: number) => void;
  setAgentMaxRounds: (rounds: number) => void;
  setAgentMaxToolCalls: (calls: number) => void;
  setAgentMaxWallTimeSeconds: (seconds: number) => void;
  setAgentTemperature: (temperature: number) => void;
  setAgentPythonTimeoutSeconds: (seconds: number) => void;
  setAgentToolPolicy: (policy: Partial<AgentToolPolicy>) => void;
  setAgentProjectId: (projectId: string) => void;
  toggleAgentInspector: () => void;
  setWorkspaceScope: (workspaceId: string) => void;
  clearAgentWorkspaceSessionCache: (workspaceId: string) => void;
  createAgentConversation: (projectId?: string) => string;
  replaceAgentConversations: (sessions: AgentConversationSession[], activeSessionId?: string) => void;
  upsertAgentConversation: (session: AgentConversationSession) => void;
  selectAgentConversation: (sessionId: string) => void;
  deleteAgentConversation: (sessionId: string) => void;
  renameAgentConversation: (sessionId: string, title: string) => void;
  setAgentConversationProject: (sessionId: string, projectId: string) => void;
  setAgentConversationMessages: (sessionId: string, messages: AgentConversationMessage[]) => void;
  setAgentConversationSkills: (sessionId: string, selectedSkillIds: string[]) => void;
  clearAgentConversation: (sessionId: string) => void;
}

export const useAppStore = create<AppStore>()(persist((set) => ({
  activeNavigation: "overview",
  commandOpen: false,
  theme: "light",
  fontScale: 100,
  language: "zh-CN",
  uiDensity: "compact",
  hidePageHeaders: false,
  navigationMode: "normal",
  customVisibleNavigationIds: [...CONFIGURABLE_NAVIGATION_IDS],
  collapsedSidebars: {},
  inspectorOpen: true,
  dragActive: false,
  operationNotice: "",
  agentBaseUrl: "http://127.0.0.1/v1",
  agentModel: "deepseek-v4-flash",
  agentApiKey: "",
  agentProviderRef: null,
  agentMode: "rpaz",
  agentStreamEnabled: true,
  agentContextWindow: 393216,
  agentMaxOutputTokens: 98304,
  agentMaxRounds: 64,
  agentMaxToolCalls: 128,
  agentMaxWallTimeSeconds: 900,
  agentTemperature: 0.2,
  agentPythonTimeoutSeconds: 300,
  agentToolPolicy: {
    enabled: true,
    databaseRead: true,
    databaseConnections: true,
    arbitraryFileRead: true,
    fileReadScope: "system",
    knowledgeBaseRead: true,
    documentRead: true,
    documentWrite: true,
    documentConvert: true,
    projectWrite: true,
    python: true,
    workspaceWrite: true,
    extensions: true,
    browser: true,
    rpazRuns: true,
    runRecords: true,
    vaultRead: true,
    vaultWrite: true,
  },
  agentProjectId: "",
  agentInspectorOpen: true,
  activeWorkspaceId: initialWorkspaceId,
  workspaceScopeLoaded: false,
  agentWorkspaceStates: {},
  agentSessions: [initialAgentSession],
  activeAgentSessionId: initialAgentSession.id,
  selectedPackageId: "",
  selectedProfileId: "",
  snapshot: null,
  setActiveNavigation: (activeNavigation) => set({ activeNavigation }),
  setCommandOpen: (commandOpen) => set({ commandOpen }),
  setTheme: (theme) => set({ theme }),
  setFontScale: (fontScale) => set({ fontScale: Math.min(200, Math.max(75, Math.round(fontScale / 5) * 5)) }),
  setLanguage: (language) => set({ language }),
  setUiDensity: (uiDensity) => set({ uiDensity }),
  setHidePageHeaders: (hidePageHeaders) => set({ hidePageHeaders }),
  setNavigationMode: (navigationMode) => set((state) => ({
    navigationMode,
    customVisibleNavigationIds: navigationMode === "custom" && state.navigationMode !== "custom"
      ? [...getVisibleNavigationIds(state.navigationMode, state.customVisibleNavigationIds)]
      : state.customVisibleNavigationIds,
  })),
  setNavigationVisible: (id, visible) => set((state) => {
    const selected = new Set(state.customVisibleNavigationIds);
    if (visible) selected.add(id);
    else selected.delete(id);
    return {
      navigationMode: "custom",
      customVisibleNavigationIds: normalizeCustomNavigationIds([...selected]),
    };
  }),
  setSidebarCollapsed: (id, collapsed) => set((state) => ({
    collapsedSidebars: { ...state.collapsedSidebars, [id]: collapsed },
  })),
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
  setAgentBaseUrl: (agentBaseUrl) => set({ agentBaseUrl, agentProviderRef: null }),
  setAgentModel: (agentModel) => set({ agentModel, agentProviderRef: null }),
  setAgentApiKey: (agentApiKey) => set({ agentApiKey, agentProviderRef: null }),
  setAgentProviderRef: (agentProviderRef) => set({ agentProviderRef }),
  setAgentMode: (agentMode) => set({ agentMode }),
  setAgentStreamEnabled: (agentStreamEnabled) => set({ agentStreamEnabled }),
  setAgentContextWindow: (agentContextWindow) => set({ agentContextWindow }),
  setAgentMaxOutputTokens: (agentMaxOutputTokens) => set({ agentMaxOutputTokens }),
  setAgentMaxRounds: (agentMaxRounds) => set({ agentMaxRounds: Math.min(256, Math.max(1, Math.round(agentMaxRounds))) }),
  setAgentMaxToolCalls: (agentMaxToolCalls) => set({ agentMaxToolCalls: Math.min(4096, Math.max(1, Math.round(agentMaxToolCalls))) }),
  setAgentMaxWallTimeSeconds: (agentMaxWallTimeSeconds) => set({ agentMaxWallTimeSeconds: Math.min(86400, Math.max(10, Math.round(agentMaxWallTimeSeconds))) }),
  setAgentTemperature: (agentTemperature) => set({ agentTemperature }),
  setAgentPythonTimeoutSeconds: (agentPythonTimeoutSeconds) => set({ agentPythonTimeoutSeconds }),
  setAgentToolPolicy: (policy) => set((state) => ({
    agentToolPolicy: { ...state.agentToolPolicy, ...policy },
  })),
  setAgentProjectId: (agentProjectId) => set({ agentProjectId }),
  toggleAgentInspector: () => set((state) => ({ agentInspectorOpen: !state.agentInspectorOpen })),
  setWorkspaceScope: (workspaceId) => {
    localStorage.setItem("drpa-active-workspace-id", workspaceId);
    set((state) => {
      if (state.workspaceScopeLoaded && state.activeWorkspaceId === workspaceId) {
        return {};
      }
      const agentWorkspaceStates = state.agentWorkspaceStates;
      const target = agentWorkspaceStates[workspaceId];
      if (target?.sessions.length) {
        const active = target.sessions.find((session) => session.id === target.activeSessionId)
          ?? target.sessions[0];
        return {
          activeWorkspaceId: workspaceId,
          workspaceScopeLoaded: true,
          agentWorkspaceStates,
          agentSessions: target.sessions,
          activeAgentSessionId: active.id,
          agentProjectId: active.projectId,
          agentProviderRef: null,
        };
      }
      if (state.activeWorkspaceId === workspaceId) {
        return { workspaceScopeLoaded: true };
      }
      const session = createAgentSession();
      return {
        activeWorkspaceId: workspaceId,
        workspaceScopeLoaded: true,
        agentWorkspaceStates,
        agentSessions: [session],
        activeAgentSessionId: session.id,
        agentProjectId: "",
        agentProviderRef: null,
      };
    });
  },
  clearAgentWorkspaceSessionCache: (workspaceId) => set((state) => {
    if (!state.agentWorkspaceStates[workspaceId]) return {};
    const agentWorkspaceStates = { ...state.agentWorkspaceStates };
    delete agentWorkspaceStates[workspaceId];
    return { agentWorkspaceStates };
  }),
  createAgentConversation: (projectId = "") => {
    const session = createAgentSession(projectId);
    set((state) => ({
      agentSessions: [session, ...state.agentSessions],
      activeAgentSessionId: session.id,
      agentProjectId: projectId,
    }));
    return session.id;
  },
  replaceAgentConversations: (agentSessions, preferredActiveSessionId) => set((state) => {
    const fallback = agentSessions[0];
    const activeAgentSessionId = agentSessions.some((session) => (
      session.id === (preferredActiveSessionId ?? state.activeAgentSessionId)
    ))
      ? (preferredActiveSessionId ?? state.activeAgentSessionId)
      : fallback?.id ?? "";
    const active = agentSessions.find((session) => session.id === activeAgentSessionId);
    return {
      agentSessions,
      activeAgentSessionId,
      agentProjectId: active?.projectId ?? "",
    };
  }),
  upsertAgentConversation: (session) => set((state) => ({
    agentSessions: state.agentSessions.some((item) => item.id === session.id)
      ? state.agentSessions.map((item) => item.id === session.id ? session : item)
      : [session, ...state.agentSessions],
  })),
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
      ? { ...session, messages, messageCount: messages.length, updatedAt: Date.now() }
      : session),
  })),
  setAgentConversationSkills: (sessionId, selectedSkillIds) => set((state) => ({
    agentSessions: state.agentSessions.map((session) => session.id === sessionId
      ? { ...session, selectedSkillIds: [...new Set(selectedSkillIds)], updatedAt: Date.now() }
      : session),
  })),
  clearAgentConversation: (sessionId) => set((state) => ({
    agentSessions: state.agentSessions.map((session) => session.id === sessionId
      ? { ...session, messages: [], messageCount: 0, updatedAt: Date.now() }
      : session),
  })),
}), {
  name: "drpa-ui-preferences",
  version: 12,
  migrate: (persistedState, version) => {
    const state = (persistedState ?? {}) as Partial<AppStore>;
    const migrated = { ...state };
    if (version < 3 && state.agentSessions?.length) {
      migrated.agentWorkspaceStates = {
        personal: {
          sessions: state.agentSessions,
          activeSessionId: state.activeAgentSessionId ?? state.agentSessions[0].id,
          projectId: state.agentProjectId ?? "",
        },
      };
    }
    if (version < 4) {
      migrated.agentToolPolicy = {
        enabled: true,
        databaseRead: true,
        databaseConnections: true,
        arbitraryFileRead: true,
        knowledgeBaseRead: true,
        documentRead: true,
        documentWrite: true,
        documentConvert: true,
        projectWrite: true,
        python: true,
        workspaceWrite: true,
        extensions: true,
        ...state.agentToolPolicy,
        browser: state.agentToolPolicy?.browser ?? true,
        rpazRuns: state.agentToolPolicy?.rpazRuns ?? true,
        runRecords: state.agentToolPolicy?.runRecords ?? true,
        vaultRead: state.agentToolPolicy?.vaultRead ?? true,
        vaultWrite: state.agentToolPolicy?.vaultWrite ?? true,
      };
    }
    if (version < 5) {
      if (state.agentBaseUrl === "https://api.openai.com/v1" || !state.agentBaseUrl) {
        migrated.agentBaseUrl = "http://127.0.0.1/v1";
      }
      if (state.agentModel === "gpt-5.4-mini" || !state.agentModel) {
        migrated.agentModel = "deepseek-v4-flash";
      }
      if (state.agentContextWindow === 128000 || !state.agentContextWindow) {
        migrated.agentContextWindow = 393216;
      }
      if (state.agentMaxOutputTokens === 4096 || !state.agentMaxOutputTokens) {
        migrated.agentMaxOutputTokens = 98304;
      }
      migrated.agentPythonTimeoutSeconds = state.agentPythonTimeoutSeconds ?? 300;
      const normalizeSessions = (sessions: AgentConversationSession[] = []) => sessions.map((session) => ({
        ...session,
        projectId: session.projectId ?? "",
        selectedSkillIds: session.selectedSkillIds ?? [],
        contextFiles: session.contextFiles ?? [],
        messageCount: session.messageCount ?? session.messages.length,
      }));
      if (migrated.agentWorkspaceStates) {
        migrated.agentWorkspaceStates = Object.fromEntries(
          Object.entries(migrated.agentWorkspaceStates).map(([workspaceId, workspaceState]) => [
            workspaceId,
            { ...workspaceState, sessions: normalizeSessions(workspaceState.sessions) },
          ]),
        );
      }
      migrated.agentSessions = normalizeSessions(state.agentSessions);
    }
    if (version < 9) {
      migrated.agentToolPolicy = {
        enabled: true,
        databaseRead: true,
        databaseConnections: true,
        arbitraryFileRead: true,
        knowledgeBaseRead: true,
        documentRead: true,
        documentWrite: true,
        documentConvert: true,
        projectWrite: true,
        python: true,
        workspaceWrite: true,
        extensions: true,
        ...state.agentToolPolicy,
        browser: state.agentToolPolicy?.browser ?? true,
        rpazRuns: state.agentToolPolicy?.rpazRuns ?? true,
        runRecords: state.agentToolPolicy?.runRecords ?? true,
        vaultRead: state.agentToolPolicy?.vaultRead ?? true,
        vaultWrite: state.agentToolPolicy?.vaultWrite ?? true,
      };
    }
    if (version < 10) {
      migrated.agentMaxToolCalls = state.agentMaxToolCalls ?? 128;
      migrated.agentMaxWallTimeSeconds = state.agentMaxWallTimeSeconds ?? 900;
    }
    if (version < 11) {
      migrated.agentToolPolicy = {
        ...migrated.agentToolPolicy,
        fileReadScope: state.agentToolPolicy?.fileReadScope ?? "system",
      } as AgentToolPolicy;
    }
    if (version < 12) {
      // Preserve the full navigation existing users had before interface modes
      // were introduced. They can opt into Normal or Custom mode in Settings.
      migrated.navigationMode = "developer";
      migrated.customVisibleNavigationIds = [...CONFIGURABLE_NAVIGATION_IDS];
    }
    if (version < 6) {
      migrated.agentMode = state.agentMode === "developer" ? "developer" : "rpaz";
    }
    if (version < 7) {
      const legacyScale = (state as { fontScale?: unknown }).fontScale;
      const legacyScales: Record<string, number> = {
        small: 90,
        standard: 100,
        large: 110,
        extraLarge: 120,
      };
      migrated.fontScale = typeof legacyScale === "number"
        ? Math.min(200, Math.max(75, legacyScale))
        : legacyScales[String(legacyScale ?? "standard")] ?? 100;
      migrated.language = (state as { language?: unknown }).language === "en-US" ? "en-US" : "zh-CN";
    }
    if (version < 8) {
      // session.db is the only durable conversation source from v8 onward.
      // Keep the old cache in-memory for the one-time AgentPage migration, but
      // never persist it again after this hydration.
      migrated.agentWorkspaceStates = state.agentWorkspaceStates ?? {};
      migrated.agentSessions = (state.agentSessions ?? []).map((session) => ({
        ...session,
        revision: session.revision ?? 0,
        bodyState: session.messages?.length ? "ready" : "summary",
      }));
    }
    return migrated as never;
  },
  partialize: (state) => ({
    theme: state.theme,
    fontScale: state.fontScale,
    language: state.language,
    uiDensity: state.uiDensity,
    hidePageHeaders: state.hidePageHeaders,
    navigationMode: state.navigationMode,
    customVisibleNavigationIds: state.customVisibleNavigationIds,
    collapsedSidebars: state.collapsedSidebars,
    agentBaseUrl: state.agentBaseUrl,
    agentModel: state.agentModel,
    agentProviderRef: state.agentProviderRef,
    agentMode: state.agentMode,
    agentStreamEnabled: state.agentStreamEnabled,
    agentContextWindow: state.agentContextWindow,
    agentMaxOutputTokens: state.agentMaxOutputTokens,
    agentMaxRounds: state.agentMaxRounds,
    agentMaxToolCalls: state.agentMaxToolCalls,
    agentMaxWallTimeSeconds: state.agentMaxWallTimeSeconds,
    agentTemperature: state.agentTemperature,
    agentPythonTimeoutSeconds: state.agentPythonTimeoutSeconds,
    agentToolPolicy: state.agentToolPolicy,
    agentProjectId: state.agentProjectId,
    agentInspectorOpen: state.agentInspectorOpen,
  }),
}));
