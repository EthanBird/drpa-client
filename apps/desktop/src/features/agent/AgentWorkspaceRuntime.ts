import type {
  AgentConversationProject,
  AgentConversationSession,
  AgentConversationSessionSummary,
  AgentContextCheckpoint,
  AgentWorkspaceConfig,
  AgentStreamEvent,
  AgentToolEvent,
  AgentRunRoundProjection,
  AgentTurnRequest,
  AgentTurnResult,
} from "../../domain/models";
import { desktopGateway } from "../../infra/gateway";
import { useAppStore } from "../../app/store";

interface AgentWorkspaceIndex {
  projects: AgentConversationProject[];
  sessions: AgentConversationSessionSummary[];
  config: AgentWorkspaceConfig | null;
}

export interface AgentRunProjection {
  requestId: string;
  status: "idle" | "running" | "cancelling" | "completed" | "failed" | "cancelled";
  content: string;
  tools: AgentToolEvent[];
  rounds: AgentRunRoundProjection[];
  usage: AgentTurnResult["usage"];
  durationMs: number;
  stopReason: string;
  error: string;
  retries: Array<{ round: number; attempt: number; maxAttempts: number; delayMs: number; error: string }>;
  contextCheckpoint?: AgentContextCheckpoint;
}

const IDLE_RUN: AgentRunProjection = Object.freeze({
  requestId: "",
  status: "idle",
  content: "",
  tools: [],
  rounds: [],
  usage: { promptTokens: 0, completionTokens: 0 },
  durationMs: 0,
  stopReason: "",
  error: "",
  retries: [],
});

function updateToolInPlace(tools: AgentToolEvent[], next: AgentToolEvent): AgentToolEvent[] {
  const index = tools.findIndex((tool) => tool.callId === next.callId);
  if (index < 0) return [...tools, next];
  return tools.map((tool, toolIndex) => toolIndex === index ? { ...tool, ...next } : tool);
}

function startRound(rounds: AgentRunRoundProjection[], round: number): AgentRunRoundProjection[] {
  const existing = rounds.findIndex((item) => item.round === round);
  const completed = rounds.map((item) => ({
    ...item,
    status: item.round < round ? "completed" as const : item.status,
  }));
  if (existing >= 0) {
    return completed.map((item) => item.round === round ? { ...item, status: "running" } : item);
  }
  return [...completed, { round, status: "running" }];
}

function assembleRound(
  rounds: AgentRunRoundProjection[],
  event: Extract<AgentStreamEvent, { type: "contextAssembled" }>,
): AgentRunRoundProjection[] {
  const prepared = rounds.some((item) => item.round === event.round)
    ? rounds
    : startRound(rounds, event.round);
  return prepared.map((item) => item.round === event.round ? {
    ...item,
    estimatedTokens: event.estimatedTokens,
    omittedMessages: event.omittedMessages,
    omittedTools: event.omittedTools,
  } : item);
}

function summarySession(
  summary: AgentConversationSessionSummary,
  body?: AgentConversationSession,
): AgentConversationSession {
  return {
    id: summary.id,
    title: summary.title,
    projectId: summary.projectId ?? "",
    createdAt: summary.createdAt,
    updatedAt: summary.updatedAt,
    revision: summary.revision ?? body?.revision ?? 0,
    messages: body?.messages ?? [],
    selectedSkillIds: summary.selectedSkillIds ?? body?.selectedSkillIds ?? [],
    contextFiles: body?.contextFiles ?? [],
    messageCount: summary.messageCount,
    bodyState: body ? "ready" : "summary",
  };
}

function sessionFingerprint(session: AgentConversationSession): string {
  return JSON.stringify({
    title: session.title,
    projectId: session.projectId,
    messages: session.messages,
    selectedSkillIds: session.selectedSkillIds,
    contextFiles: session.contextFiles ?? [],
  });
}

/**
 * One runtime exists per workspace regardless of how many Agent surfaces are
 * mounted. It owns loading, write serialization, tombstones and surface
 * selections; React surfaces only observe the shared store.
 */
export class AgentWorkspaceRuntime {
  private initialization?: Promise<AgentWorkspaceIndex>;
  private readonly bodies = new Map<string, AgentConversationSession>();
  private readonly loads = new Map<string, Promise<AgentConversationSession>>();
  private readonly writes = new Map<string, Promise<void>>();
  private readonly deleted = new Set<string>();
  private readonly surfaceSelections = new Map<string, string>();
  private readonly surfaceDrafts = new Map<string, string>();
  private readonly runs = new Map<string, AgentRunProjection>();
  private readonly runListeners = new Map<string, Set<() => void>>();
  private readonly pendingRunContent = new Map<string, string>();
  private readonly runFrames = new Map<string, number>();

  constructor(readonly workspaceId: string) {}

  initialize(): Promise<AgentWorkspaceIndex> {
    if (this.initialization) return this.initialization;
    this.initialization = this.initializeOnce().catch((error) => {
      this.initialization = undefined;
      throw error;
    });
    return this.initialization;
  }

  private async initializeOnce(): Promise<AgentWorkspaceIndex> {
    const [storedProjects, initialSessions, config] = await Promise.all([
      desktopGateway.listAgentProjects(),
      desktopGateway.listAgentSessions(),
      desktopGateway.getAgentWorkspaceConfig().catch(() => null),
    ]);
    let sessions = initialSessions;
    let projects = storedProjects;
    const store = useAppStore.getState();
    const legacyState = store.agentWorkspaceStates[this.workspaceId];
    const legacySessions = legacyState?.sessions ?? [];
    const storedIds = new Set(sessions.map((session) => session.id));
    const missingLegacy = legacySessions.filter((session) => !storedIds.has(session.id));

    for (const legacy of missingLegacy) {
      const saved = await desktopGateway.saveAgentSession({
        ...legacy,
        revision: legacy.revision ?? 0,
        projectId: legacy.projectId ?? "",
        selectedSkillIds: legacy.selectedSkillIds ?? [],
        messages: legacy.messages ?? [],
        bodyState: "ready",
      });
      this.bodies.set(saved.id, { ...saved, bodyState: "ready" });
    }
    if (missingLegacy.length > 0) {
      [sessions, projects] = await Promise.all([
        desktopGateway.listAgentSessions(),
        desktopGateway.listAgentProjects(),
      ]);
    }
    if (sessions.length === 0) {
      const created = await desktopGateway.createAgentSession();
      this.bodies.set(created.id, { ...created, bodyState: "ready" });
      sessions = [{
        id: created.id,
        title: created.title,
        projectId: created.projectId || null,
        createdAt: created.createdAt,
        updatedAt: created.updatedAt,
        revision: created.revision,
        messageCount: 0,
        selectedSkillIds: created.selectedSkillIds,
      }];
    }

    const current = new Map(useAppStore.getState().agentSessions.map((session) => [session.id, session]));
    const legacy = new Map(legacySessions.map((session) => [session.id, session]));
    const indexed = sessions.map((summary) => {
      const body = this.bodies.get(summary.id)
        ?? (current.get(summary.id)?.bodyState === "ready" ? current.get(summary.id) : undefined)
        ?? (legacy.get(summary.id)?.messages?.length ? legacy.get(summary.id) : undefined);
      if (body) this.bodies.set(summary.id, { ...body, revision: summary.revision, bodyState: "ready" });
      return summarySession(summary, body);
    });
    const preferred = sessions.some((session) => session.id === useAppStore.getState().activeAgentSessionId)
      ? useAppStore.getState().activeAgentSessionId
      : sessions[0]?.id;
    useAppStore.getState().replaceAgentConversations(indexed, preferred);
    if (legacyState) useAppStore.getState().clearAgentWorkspaceSessionCache(this.workspaceId);
    return { projects, sessions, config };
  }

  isLoaded(sessionId: string): boolean {
    return this.bodies.has(sessionId);
  }

  loadedBody(sessionId: string): AgentConversationSession | undefined {
    return this.bodies.get(sessionId);
  }

  async loadSession(sessionId: string): Promise<AgentConversationSession> {
    const loaded = this.bodies.get(sessionId);
    if (loaded) return loaded;
    const pending = this.loads.get(sessionId);
    if (pending) return pending;
    const load = desktopGateway.getAgentSession(sessionId)
      .then((session) => {
        const ready = { ...session, bodyState: "ready" as const };
        this.bodies.set(sessionId, ready);
        useAppStore.getState().upsertAgentConversation(ready);
        return ready;
      })
      .finally(() => this.loads.delete(sessionId));
    this.loads.set(sessionId, load);
    return load;
  }

  async createSession(projectId = ""): Promise<AgentConversationSession> {
    const session = { ...await desktopGateway.createAgentSession(projectId), bodyState: "ready" as const };
    this.deleted.delete(session.id);
    this.bodies.set(session.id, session);
    useAppStore.getState().upsertAgentConversation(session);
    return session;
  }

  async openFileSession(selectedSkillIds: string[] = []): Promise<AgentConversationSession | null> {
    const opened = await desktopGateway.openAgentFileSession(selectedSkillIds);
    if (!opened) return null;
    const session = { ...opened, bodyState: "ready" as const };
    this.deleted.delete(session.id);
    this.bodies.set(session.id, session);
    useAppStore.getState().upsertAgentConversation(session);
    return session;
  }

  persistSession(sessionId: string): Promise<void> {
    return this.serialize(sessionId, async () => {
      if (this.deleted.has(sessionId)) throw new Error("会话已删除，已阻止延迟保存重新创建会话");
      const current = useAppStore.getState().agentSessions.find((session) => session.id === sessionId);
      if (!current || !this.bodies.has(sessionId)) throw new Error("会话正文尚未加载，已阻止覆盖数据库");
      const input = {
        ...current,
        revision: this.bodies.get(sessionId)?.revision ?? current.revision ?? 0,
        bodyState: "ready" as const,
      };
      const fingerprint = sessionFingerprint(input);
      const saved = { ...await desktopGateway.saveAgentSession(input), bodyState: "ready" as const };
      this.bodies.set(sessionId, saved);
      const latest = useAppStore.getState().agentSessions.find((session) => session.id === sessionId);
      if (!latest) return;
      useAppStore.getState().upsertAgentConversation(
        sessionFingerprint(latest) === fingerprint
          ? saved
          : { ...latest, revision: saved.revision, bodyState: "ready" },
      );
    });
  }

  renameSession(sessionId: string, title: string): Promise<AgentConversationSession> {
    return this.serializeResult(sessionId, async () => {
      const saved = { ...await desktopGateway.renameAgentSession(sessionId, title), bodyState: "ready" as const };
      this.bodies.set(sessionId, saved);
      useAppStore.getState().upsertAgentConversation(saved);
      return saved;
    });
  }

  moveSession(sessionId: string, projectId: string): Promise<AgentConversationSession> {
    return this.serializeResult(sessionId, async () => {
      const saved = { ...await desktopGateway.moveAgentSession(sessionId, projectId), bodyState: "ready" as const };
      this.bodies.set(sessionId, saved);
      useAppStore.getState().upsertAgentConversation(saved);
      return saved;
    });
  }

  async deleteSession(sessionId: string): Promise<void> {
    this.deleted.add(sessionId);
    await (this.writes.get(sessionId) ?? Promise.resolve()).catch(() => undefined);
    await desktopGateway.deleteAgentSession(sessionId);
    this.bodies.delete(sessionId);
    this.loads.delete(sessionId);
    this.writes.delete(sessionId);
    for (const [surfaceId, selectedId] of this.surfaceSelections) {
      if (selectedId === sessionId) this.surfaceSelections.delete(surfaceId);
    }
    useAppStore.getState().deleteAgentConversation(sessionId);
  }

  private serialize(sessionId: string, operation: () => Promise<void>): Promise<void> {
    const previous = this.writes.get(sessionId) ?? Promise.resolve();
    const next = previous.catch(() => undefined).then(operation);
    this.writes.set(sessionId, next);
    void next.finally(() => {
      if (this.writes.get(sessionId) === next) this.writes.delete(sessionId);
    }).catch(() => undefined);
    return next;
  }

  private serializeResult<T>(sessionId: string, operation: () => Promise<T>): Promise<T> {
    let result!: T;
    return this.serialize(sessionId, async () => { result = await operation(); }).then(() => result);
  }

  getSurfaceSelection(surfaceId: string): string {
    return this.surfaceSelections.get(surfaceId) ?? "";
  }

  setSurfaceSelection(surfaceId: string, sessionId: string): void {
    if (sessionId) this.surfaceSelections.set(surfaceId, sessionId);
  }

  getSurfaceDraft(surfaceId: string): string {
    return this.surfaceDrafts.get(surfaceId) ?? "";
  }

  setSurfaceDraft(surfaceId: string, draft: string): void {
    if (draft) this.surfaceDrafts.set(surfaceId, draft);
    else this.surfaceDrafts.delete(surfaceId);
  }

  getRunProjection(sessionId: string): AgentRunProjection {
    return this.runs.get(sessionId) ?? IDLE_RUN;
  }

  subscribeRun(sessionId: string, listener: () => void): () => void {
    if (!sessionId) return () => undefined;
    const listeners = this.runListeners.get(sessionId) ?? new Set<() => void>();
    listeners.add(listener);
    this.runListeners.set(sessionId, listeners);
    return () => {
      listeners.delete(listener);
      if (listeners.size === 0) this.runListeners.delete(sessionId);
    };
  }

  async runTurn(request: AgentTurnRequest): Promise<AgentTurnResult> {
    const sessionId = request.sessionId ?? "";
    if (!sessionId) throw new Error("Agent Run 缺少 sessionId");
    const existing = this.getRunProjection(sessionId);
    if (existing.status === "running" || existing.status === "cancelling") {
      throw new Error(`当前会话已有 Agent Run 在执行：${existing.requestId}`);
    }
    this.pendingRunContent.set(sessionId, "");
    this.setRunProjection(sessionId, {
      requestId: request.requestId,
      status: "running",
      content: "",
      tools: [],
      rounds: [],
      usage: { promptTokens: 0, completionTokens: 0 },
      durationMs: 0,
      stopReason: "",
      error: "",
      retries: [],
    });
    let unlisten: () => void = () => undefined;
    const startedAt = Date.now();
    try {
      unlisten = await desktopGateway.listenAgentStream(request.requestId, (event) => {
        this.consumeRunEvent(sessionId, event);
      });
      const result = await desktopGateway.runAgentTurn(request);
      this.flushRunContent(sessionId);
      const latest = this.getRunProjection(sessionId);
      this.setRunProjection(sessionId, {
        ...latest,
        status: "completed",
        content: result.message,
        tools: result.tools,
        rounds: latest.rounds.map((round) => ({ ...round, status: "completed" })),
        usage: result.usage,
        durationMs: result.durationMs,
        stopReason: result.stopReason,
        retries: latest.retries,
        contextCheckpoint: result.contextCheckpoint ?? latest.contextCheckpoint,
      });
      return result;
    } catch (reason) {
      this.flushRunContent(sessionId);
      const message = String(reason);
      const cancelled = this.getRunProjection(sessionId).status === "cancelling"
        || message.includes("运行已取消");
      this.setRunProjection(sessionId, {
        ...this.getRunProjection(sessionId),
        status: cancelled ? "cancelled" : "failed",
        error: message,
        durationMs: Math.max(this.getRunProjection(sessionId).durationMs, Date.now() - startedAt),
      });
      throw reason;
    } finally {
      unlisten();
    }
  }

  async cancelRun(sessionId: string): Promise<void> {
    const current = this.getRunProjection(sessionId);
    if (!current.requestId || !["running", "cancelling"].includes(current.status)) return;
    this.setRunProjection(sessionId, { ...current, status: "cancelling" });
    await desktopGateway.cancelAgentRun(current.requestId);
  }

  private consumeRunEvent(sessionId: string, event: AgentStreamEvent): void {
    const current = this.getRunProjection(sessionId);
    if (event.type === "delta") {
      this.pendingRunContent.set(
        sessionId,
        (this.pendingRunContent.get(sessionId) ?? current.content) + event.content,
      );
      this.scheduleRunContent(sessionId);
    } else if (event.type === "contentReplace") {
      this.pendingRunContent.set(sessionId, event.content);
      this.scheduleRunContent(sessionId);
    } else if (event.type === "roundStarted") {
      this.setRunProjection(sessionId, { ...current, rounds: startRound(current.rounds, event.round) });
    } else if (event.type === "contextAssembled") {
      this.setRunProjection(sessionId, { ...current, rounds: assembleRound(current.rounds, event) });
    } else if (event.type === "retrying") {
      this.setRunProjection(sessionId, {
        ...current,
        retries: [...current.retries, {
          round: event.round,
          attempt: event.attempt,
          maxAttempts: event.maxAttempts,
          delayMs: event.delayMs,
          error: event.error,
        }],
      });
    } else if (event.type === "contextCompacted") {
      this.setRunProjection(sessionId, { ...current, contextCheckpoint: event.checkpoint });
    } else if (event.type === "tool") {
      this.setRunProjection(sessionId, {
        ...current,
        tools: updateToolInPlace(current.tools, event.tool),
      });
    } else if (event.type === "completed") {
      this.setRunProjection(sessionId, {
        ...current,
        rounds: current.rounds.map((round) => ({ ...round, status: "completed" })),
        usage: event.usage,
        durationMs: event.durationMs,
        stopReason: event.stopReason,
      });
    } else if (event.type === "failed") {
      this.setRunProjection(sessionId, { ...current, status: "failed", error: event.error });
    } else if (event.type === "cancelled") {
      this.setRunProjection(sessionId, { ...current, status: "cancelled" });
    }
  }

  private scheduleRunContent(sessionId: string): void {
    if (this.runFrames.has(sessionId)) return;
    const frame = requestAnimationFrame(() => {
      this.runFrames.delete(sessionId);
      this.flushRunContent(sessionId);
    });
    this.runFrames.set(sessionId, frame);
  }

  private flushRunContent(sessionId: string): void {
    const frame = this.runFrames.get(sessionId);
    if (frame !== undefined) {
      cancelAnimationFrame(frame);
      this.runFrames.delete(sessionId);
    }
    const content = this.pendingRunContent.get(sessionId);
    if (content === undefined) return;
    this.setRunProjection(sessionId, { ...this.getRunProjection(sessionId), content });
  }

  private setRunProjection(sessionId: string, projection: AgentRunProjection): void {
    this.runs.set(sessionId, projection);
    this.runListeners.get(sessionId)?.forEach((listener) => listener());
  }

  dispose(): void {
    this.runFrames.forEach((frame) => cancelAnimationFrame(frame));
    this.runFrames.clear();
    this.runListeners.clear();
  }
}

const runtimes = new Map<string, AgentWorkspaceRuntime>();

export function getAgentWorkspaceRuntime(workspaceId: string): AgentWorkspaceRuntime {
  let runtime = runtimes.get(workspaceId);
  if (!runtime) {
    runtime = new AgentWorkspaceRuntime(workspaceId);
    runtimes.set(workspaceId, runtime);
  }
  return runtime;
}

export function resetAgentWorkspaceRuntimesForTests(): void {
  runtimes.forEach((runtime) => runtime.dispose());
  runtimes.clear();
}
