import {
  Bot,
  CheckCircle2,
  ChevronDown,
  CircleAlert,
  Clock3,
  Code2,
  Cpu,
  Download,
  FileCode2,
  FileText,
  Folder,
  FolderCode,
  FolderPlus,
  KeyRound,
  Layers3,
  Link2,
  LoaderCircle,
  MessageSquarePlus,
  Paperclip,
  PanelRightClose,
  PanelRightOpen,
  Pencil,
  Plus,
  RefreshCcw,
  RotateCcw,
  Send,
  Sparkles,
  Trash2,
  Upload,
  Wrench,
  X,
  XCircle,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import type { DragEvent } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { useAppStore } from "../app/store";
import { SidebarToggle, useSidebarCollapsed } from "../components/SidebarToggle";
import { getAgentWorkspaceRuntime } from "../features/agent/AgentWorkspaceRuntime";
import type {
  AgentConversationMessage,
  AgentConversationProject,
  AgentConversationSession,
  AgentDocumentArtifact,
  AgentDocumentAttachment,
  AgentMessage,
  AgentSkillSummary,
  AgentToolEvent,
  StudioProject,
} from "../domain/models";
import { desktopGateway } from "../infra/gateway";
import "../styles/agent-documents.css";

const toolLabels: Record<string, string> = {
  agent_list_skills: "列出 Skills",
  agent_read_skill: "读取 Skill",
  agent_write_skill: "写入 Skill",
  agent_read_memory: "读取长期记忆",
  agent_write_memory: "更新长期记忆",
  agent_remember: "追加结构化记忆",
  knowledge_list_documents: "列出知识文档",
  knowledge_read_document: "读取知识文档",
  knowledge_write_document: "写入知识文档",
  knowledge_base_list: "列出向量知识库",
  knowledge_base_search: "检索向量知识库",
  document_read: "读取对话文档",
  document_create: "创建办公文档",
  document_convert: "转换办公文档",
  data_list_connections: "列出数据连接",
  data_get_schema: "读取数据库结构",
  data_query: "执行只读查询",
  data_create_connection: "创建连接配置",
  rpaz_list_files: "列出项目文件",
  read_file: "按行读取文件",
  find_files: "快速查找文件",
  search_text: "检索项目文本",
  edit_file: "精确编辑文件",
  rpaz_write_file: "写入项目文件",
  rpaz_validate: "校验 RPAZ",
  rpaz_build: "构建 RPAZ",
  rpaz_python: "运行内置 Python",
};

const suggestions = [
  "分析当前项目结构，给出可直接执行的改进计划",
  "读取项目数据并用 Python 完成一次可复现的分析",
  "如果这是 RPAZ 项目，请检查 manifest.yaml 与入口实现",
];

const documentGateway = desktopGateway;
const DOCUMENT_CONTEXT_MARKER = "\n\n[DRPA_LOCAL_DOCUMENT_CONTEXT_V1]\n";
const SUPPORTED_DOCUMENT_EXTENSION = /\.(pdf|docx|xlsx|pptx)$/i;

function visibleMessageContent(content: string): string {
  const marker = content.indexOf(DOCUMENT_CONTEXT_MARKER);
  return marker < 0 ? content : content.slice(0, marker);
}

export function visibleAssistantMessageContent(content: string): string {
  let visible = content.replace(
    /<(?:mm:)?think\b[^>]*>[\s\S]*?<\/(?:mm:)?think\s*>/gi,
    "",
  );
  const unclosed = /<(?:mm:)?think\b[^>]*>/i.exec(visible);
  if (unclosed) visible = visible.slice(0, unclosed.index);
  return visible.replace(/<\/?(?:mm:)?think\b[^>]*>/gi, "");
}

function replaceVisibleMessageContent(original: string, visible: string): string {
  const marker = original.indexOf(DOCUMENT_CONTEXT_MARKER);
  return marker < 0 ? visible : `${visible}${original.slice(marker)}`;
}

function modelMessageContent(message: AgentConversationMessage): string {
  if (!message.tools?.length) return message.content;
  const evidence = message.tools.slice(-24).map((tool) => {
    const output = tool.output.length > 2_000
      ? `${tool.output.slice(0, 2_000)}\n…工具输出已截断…`
      : tool.output;
    return `- ${tool.name} [${tool.status}]: ${tool.summary}\n${output}`;
  });
  return `${message.content}\n\n[DRPA_PREVIOUS_TOOL_EVIDENCE_V1]\n${evidence.join("\n")}`;
}

function withAttachmentContext(
  content: string,
  attachments: AgentDocumentAttachment[],
): string {
  if (attachments.length === 0) return content;
  const records = attachments.map((attachment) => (
    `- ${attachment.name} (${attachment.format.toUpperCase()}), documentId=${attachment.id}`
  ));
  return `${content}${DOCUMENT_CONTEXT_MARKER}`
    + "以下是客户端已复制到当前会话隔离区的附件。需要查看内容时，"
    + "请使用 document_read 并原样传入 documentId；不要猜测或请求文件路径。\n"
    + records.join("\n");
}

function formatDocumentSize(sizeBytes: number): string {
  if (sizeBytes < 1024) return `${sizeBytes} B`;
  if (sizeBytes < 1024 * 1024) return `${(sizeBytes / 1024).toFixed(1)} KB`;
  return `${(sizeBytes / 1024 / 1024).toFixed(1)} MB`;
}

function droppedDocumentPaths(event: DragEvent<HTMLElement>): string[] {
  const filePaths = Array.from(event.dataTransfer.files)
    .map((file) => (file as File & { path?: string }).path ?? "")
    .filter(Boolean);
  const uriPaths = event.dataTransfer.getData("text/uri-list")
    .split(/\r?\n/)
    .filter((value) => value && !value.startsWith("#"))
    .flatMap((value) => {
      try {
        const url = new URL(value);
        if (url.protocol !== "file:") return [];
        let path = decodeURIComponent(url.pathname);
        if (/^\/[A-Za-z]:\//.test(path)) path = path.slice(1);
        if (url.host) path = `//${url.host}${path}`;
        return [path];
      } catch {
        return [];
      }
    });
  return [...new Set([...filePaths, ...uriPaths])]
    .filter((path) => SUPPORTED_DOCUMENT_EXTENSION.test(path));
}

function messageId(role: AgentMessage["role"]): string {
  return `${role}-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function formatSessionTime(value: number): string {
  const date = new Date(value);
  const today = new Date();
  if (date.toDateString() === today.toDateString()) {
    return date.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" });
  }
  return date.toLocaleDateString("zh-CN", { month: "2-digit", day: "2-digit" });
}

function latestUserIndex(messages: AgentConversationMessage[]): number {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    if (messages[index].role === "user") return index;
  }
  return -1;
}

interface AgentPageProps {
  embedded?: boolean;
  embeddedProjectId?: string;
  embeddedProjectName?: string;
}

export function AgentPage({
  embedded = false,
  embeddedProjectId = "",
  embeddedProjectName = "",
}: AgentPageProps = {}) {
  const sessionsCollapsed = useSidebarCollapsed("agent-sessions");
  const agentBaseUrl = useAppStore((state) => state.agentBaseUrl);
  const agentModel = useAppStore((state) => state.agentModel);
  const apiKey = useAppStore((state) => state.agentApiKey);
  const agentProviderRef = useAppStore((state) => state.agentProviderRef);
  const agentMode = useAppStore((state) => state.agentMode);
  const agentStreamEnabled = useAppStore((state) => state.agentStreamEnabled);
  const agentContextWindow = useAppStore((state) => state.agentContextWindow);
  const agentMaxOutputTokens = useAppStore((state) => state.agentMaxOutputTokens);
  const agentMaxRounds = useAppStore((state) => state.agentMaxRounds);
  const agentMaxToolCalls = useAppStore((state) => state.agentMaxToolCalls);
  const agentMaxWallTimeSeconds = useAppStore((state) => state.agentMaxWallTimeSeconds);
  const agentTemperature = useAppStore((state) => state.agentTemperature);
  const agentPythonTimeoutSeconds = useAppStore((state) => state.agentPythonTimeoutSeconds);
  const agentToolPolicy = useAppStore((state) => state.agentToolPolicy);
  const agentInspectorOpen = useAppStore((state) => state.agentInspectorOpen);
  const activeWorkspaceId = useAppStore((state) => state.activeWorkspaceId);
  const workspaceScopeLoaded = useAppStore((state) => state.workspaceScopeLoaded);
  const agentSessions = useAppStore((state) => state.agentSessions);
  const activeAgentSessionId = useAppStore((state) => state.activeAgentSessionId);
  const setAgentBaseUrl = useAppStore((state) => state.setAgentBaseUrl);
  const setAgentModel = useAppStore((state) => state.setAgentModel);
  const setApiKey = useAppStore((state) => state.setAgentApiKey);
  const setAgentMode = useAppStore((state) => state.setAgentMode);
  const setAgentStreamEnabled = useAppStore((state) => state.setAgentStreamEnabled);
  const setAgentContextWindow = useAppStore((state) => state.setAgentContextWindow);
  const setAgentMaxOutputTokens = useAppStore((state) => state.setAgentMaxOutputTokens);
  const setAgentMaxRounds = useAppStore((state) => state.setAgentMaxRounds);
  const setAgentMaxToolCalls = useAppStore((state) => state.setAgentMaxToolCalls);
  const setAgentMaxWallTimeSeconds = useAppStore((state) => state.setAgentMaxWallTimeSeconds);
  const setAgentTemperature = useAppStore((state) => state.setAgentTemperature);
  const setAgentPythonTimeoutSeconds = useAppStore((state) => state.setAgentPythonTimeoutSeconds);
  const toggleAgentInspector = useAppStore((state) => state.toggleAgentInspector);
  const selectAgentConversation = useAppStore((state) => state.selectAgentConversation);
  const renameAgentConversation = useAppStore((state) => state.renameAgentConversation);
  const setAgentConversationProject = useAppStore((state) => state.setAgentConversationProject);
  const setAgentConversationMessages = useAppStore((state) => state.setAgentConversationMessages);
  const setAgentConversationSkills = useAppStore((state) => state.setAgentConversationSkills);
  const clearAgentConversation = useAppStore((state) => state.clearAgentConversation);
  const [projects, setProjects] = useState<StudioProject[]>([]);
  const [agentProjects, setAgentProjects] = useState<AgentConversationProject[]>([]);
  const [availableSkills, setAvailableSkills] = useState<AgentSkillSummary[]>([]);
  const [sessionIndexReady, setSessionIndexReady] = useState(false);
  const [sessionLoadingId, setSessionLoadingId] = useState("");
  const [projectDialog, setProjectDialog] = useState<{ mode: "create" | "rename"; projectId?: string } | null>(null);
  const [projectDraft, setProjectDraft] = useState("");
  const [projectBusy, setProjectBusy] = useState(false);
  const [projectContextMenu, setProjectContextMenu] = useState<{ x: number; y: number; projectId?: string } | null>(null);
  const [expandedProjectIds, setExpandedProjectIds] = useState<Set<string>>(() => new Set());
  const [draft, setDraft] = useState("");
  const [error, setError] = useState("");
  const [renamingId, setRenamingId] = useState("");
  const [renameDraft, setRenameDraft] = useState("");
  const [confirmation, setConfirmation] = useState<{ kind: "delete" | "clear"; sessionId: string; title: string } | null>(null);
  const [editingMessageId, setEditingMessageId] = useState("");
  const [editingDraft, setEditingDraft] = useState("");
  const [attachmentsBySession, setAttachmentsBySession] = useState<Record<string, AgentDocumentAttachment[]>>({});
  const [artifactsBySession, setArtifactsBySession] = useState<Record<string, AgentDocumentArtifact[]>>({});
  const [documentBusy, setDocumentBusy] = useState(false);
  const [exportingArtifactId, setExportingArtifactId] = useState("");
  const [documentNotice, setDocumentNotice] = useState("");
  const [documentDragActive, setDocumentDragActive] = useState(false);
  const transcriptRef = useRef<HTMLDivElement>(null);
  const agentRuntime = useMemo(
    () => getAgentWorkspaceRuntime(activeWorkspaceId),
    [activeWorkspaceId],
  );
  const surfaceId = embedded ? `studio:${embeddedProjectId || "unbound"}` : "main";
  const [surfaceSessionId, setSurfaceSessionId] = useState(() => (
    agentRuntime.getSurfaceSelection(surfaceId)
    || (!embedded ? activeAgentSessionId : "")
  ));

  useEffect(() => {
    setDraft(agentRuntime.getSurfaceDraft(surfaceId));
  }, [agentRuntime, surfaceId]);

  const updateDraft = useCallback((value: string) => {
    setDraft(value);
    agentRuntime.setSurfaceDraft(surfaceId, value);
  }, [agentRuntime, surfaceId]);

  const activeSession = useMemo(() => {
    const selected = agentSessions.find((session) => session.id === surfaceSessionId);
    if (!embedded || !embeddedProjectId) return selected ?? agentSessions[0];
    return selected?.projectId === embeddedProjectId
      ? selected
      : agentSessions.find((session) => session.projectId === embeddedProjectId);
  }, [agentSessions, embedded, embeddedProjectId, surfaceSessionId]);
  const runSessionId = activeSession?.id ?? "";
  const subscribeRun = useCallback(
    (listener: () => void) => agentRuntime.subscribeRun(runSessionId, listener),
    [agentRuntime, runSessionId],
  );
  const getRunProjection = useCallback(
    () => agentRuntime.getRunProjection(runSessionId),
    [agentRuntime, runSessionId],
  );
  const runProjection = useSyncExternalStore(subscribeRun, getRunProjection, getRunProjection);
  const busy = runProjection.status === "running" || runProjection.status === "cancelling";
  const currentRequestId = runProjection.requestId;
  const streamingContent = runProjection.content;
  const streamingTools = runProjection.tools;
  const messages = activeSession?.messages ?? [];
  const agentProjectId = activeSession?.projectId ?? "";
  const attachments = activeSession ? attachmentsBySession[activeSession.id] ?? [] : [];
  const artifacts = activeSession ? artifactsBySession[activeSession.id] ?? [] : [];

  const persistSession = useCallback((sessionId: string): Promise<void> => {
    return agentRuntime.persistSession(sessionId).catch((reason: unknown) => {
        setError(`保存会话失败：${String(reason)}`);
        throw reason;
      });
  }, [agentRuntime]);

  const refreshProjectIndex = useCallback(async () => {
    const next = await desktopGateway.listAgentProjects();
    setAgentProjects(next);
    return next;
  }, []);

  const openSession = useCallback(async (sessionId: string) => {
    setSurfaceSessionId(sessionId);
    agentRuntime.setSurfaceSelection(surfaceId, sessionId);
    if (!embedded) selectAgentConversation(sessionId);
    if (agentRuntime.isLoaded(sessionId)) return;
    const summary = useAppStore.getState().agentSessions.find((session) => session.id === sessionId);
    if (summary?.messageCount === 0) {
      await agentRuntime.loadSession(sessionId);
      return;
    }
    setSessionLoadingId(sessionId);
    try {
      await agentRuntime.loadSession(sessionId);
    } catch (reason) {
      setError(`读取会话失败：${String(reason)}`);
    } finally {
      setSessionLoadingId((current) => current === sessionId ? "" : current);
    }
  }, [agentRuntime, embedded, selectAgentConversation, surfaceId]);

  useEffect(() => {
    if (!workspaceScopeLoaded && !embedded) return;
    let active = true;
    setSessionIndexReady(false);

    void (async () => {
      try {
        const [index, studioProjects] = await Promise.all([
          agentRuntime.initialize(),
          embedded ? Promise.resolve([]) : desktopGateway.listStudioProjects().catch(() => []),
        ]);
        if (!active) return;
        setAgentProjects(index.projects);
        setProjects(studioProjects);
        setAvailableSkills(index.config?.skills ?? []);
        setSessionIndexReady(true);
        const sessions = useAppStore.getState().agentSessions;
        const remembered = agentRuntime.getSurfaceSelection(surfaceId);
        const preferred = embedded && embeddedProjectId
          ? sessions.find((session) => session.id === remembered && session.projectId === embeddedProjectId)
            ?? sessions.find((session) => session.projectId === embeddedProjectId)
          : sessions.find((session) => session.id === remembered)
            ?? sessions.find((session) => session.id === useAppStore.getState().activeAgentSessionId)
            ?? sessions[0];
        if (preferred) {
          setSurfaceSessionId(preferred.id);
          agentRuntime.setSurfaceSelection(surfaceId, preferred.id);
          if (!embedded) selectAgentConversation(preferred.id);
          await agentRuntime.loadSession(preferred.id);
        }
      } catch (reason) {
        if (active) {
          setError(`初始化会话数据库失败：${String(reason)}`);
          setSessionIndexReady(true);
        }
      }
    })();

    return () => { active = false; };
  }, [
    activeWorkspaceId,
    agentRuntime,
    embedded,
    embeddedProjectId,
    workspaceScopeLoaded,
    selectAgentConversation,
    surfaceId,
  ]);

  const refreshArtifacts = useCallback(async (sessionId: string) => {
    if (!documentGateway.listAgentArtifacts) return;
    try {
      const next = await documentGateway.listAgentArtifacts(sessionId);
      setArtifactsBySession((current) => ({ ...current, [sessionId]: next }));
    } catch (reason) {
      setDocumentNotice(`读取文档产物失败：${String(reason)}`);
    }
  }, []);

  const refreshAttachments = useCallback(async (sessionId: string) => {
    try {
      const next = await documentGateway.listAgentAttachments(sessionId);
      setAttachmentsBySession((current) => ({ ...current, [sessionId]: next }));
    } catch (reason) {
      setDocumentNotice(`读取对话附件失败：${String(reason)}`);
    }
  }, []);

  useEffect(() => {
    if (!activeSession) return;
    void refreshArtifacts(activeSession.id);
    void refreshAttachments(activeSession.id);
  }, [activeSession?.id, refreshArtifacts, refreshAttachments]);

  useEffect(() => {
    const target = transcriptRef.current;
    if (target) target.scrollTop = target.scrollHeight;
  }, [messages, busy, streamingContent, streamingTools]);

  const availableProjectOptions = useMemo(() => {
    const merged = new Map<string, {
      id: string;
      name: string;
      path?: string;
      files?: StudioProject["files"];
      kind: "general" | "rpaz";
    }>();
    agentProjects.forEach((project) => merged.set(project.id, {
      id: project.id,
      name: project.name,
      path: project.path,
      kind: "general",
    }));
    projects.forEach((project) => {
      const existing = merged.get(project.id);
      const isRpaz = project.files.some((file) => (
        file.replaceAll("\\", "/").toLowerCase().endsWith("/manifest.yaml")
        || file.toLowerCase() === "manifest.yaml"
      ));
      merged.set(project.id, {
        ...existing,
        id: project.id,
        name: isRpaz ? project.name : existing?.name ?? project.name,
        files: project.files,
        kind: isRpaz ? "rpaz" : "general",
      });
    });
    if (embeddedProjectId && !merged.has(embeddedProjectId)) {
      merged.set(embeddedProjectId, {
        id: embeddedProjectId,
        name: embeddedProjectName || embeddedProjectId,
        kind: "general",
      });
    }
    return [...merged.values()];
  }, [agentProjects, embeddedProjectId, embeddedProjectName, projects]);

  const selectedProject = useMemo(
    () => availableProjectOptions.find((project) => project.id === (
      embedded ? embeddedProjectId : agentProjectId
    )),
    [agentProjectId, availableProjectOptions, embedded, embeddedProjectId],
  );

  useEffect(() => {
    setExpandedProjectIds((current) => {
      const missing = availableProjectOptions.filter((project) => !current.has(project.id));
      if (missing.length === 0) return current;
      const next = new Set(current);
      missing.forEach((project) => next.add(project.id));
      return next;
    });
  }, [availableProjectOptions]);

  const sessionsByProject = useMemo(() => {
    const grouped = new Map<string, AgentConversationSession[]>();
    agentSessions.forEach((session) => {
      if (!session.projectId) return;
      grouped.set(session.projectId, [...(grouped.get(session.projectId) ?? []), session]);
    });
    return grouped;
  }, [agentSessions]);

  const recentSessions = useMemo(
    () => [...agentSessions].sort((left, right) => right.updatedAt - left.updatedAt),
    [agentSessions],
  );

  const embeddedSessions = useMemo(
    () => embeddedProjectId ? sessionsByProject.get(embeddedProjectId) ?? [] : [],
    [embeddedProjectId, sessionsByProject],
  );

  const importDocumentPaths = useCallback(async (sourcePaths: string[]) => {
    if (!activeSession || documentBusy) return;
    if (!documentGateway.importAgentDocument) {
      setDocumentNotice("当前运行环境尚未接入文档导入能力");
      return;
    }
    const supported = [...new Set(sourcePaths)]
      .filter((path) => SUPPORTED_DOCUMENT_EXTENSION.test(path))
      .slice(0, 12);
    if (supported.length === 0) {
      setDocumentNotice("请选择 PDF、DOCX、XLSX 或 PPTX 文件");
      return;
    }
    setDocumentBusy(true);
    setDocumentNotice("");
    const imported: AgentDocumentAttachment[] = [];
    const failures: string[] = [];
    for (const sourcePath of supported) {
      try {
        imported.push(await documentGateway.importAgentDocument(sourcePath, activeSession.id));
      } catch (reason) {
        failures.push(`${sourcePath.split(/[\\/]/).pop() ?? sourcePath}：${String(reason)}`);
      }
    }
    if (imported.length > 0) {
      setAttachmentsBySession((current) => {
        const previous = current[activeSession.id] ?? [];
        const known = new Set(previous.map((attachment) => attachment.id));
        return {
          ...current,
          [activeSession.id]: [...previous, ...imported.filter((attachment) => !known.has(attachment.id))],
        };
      });
    }
    setDocumentNotice(
      failures.length > 0
        ? `已导入 ${imported.length} 个，${failures.length} 个失败：${failures.join("；")}`
        : `已导入 ${imported.length} 个文档；发送消息后 Agent 可按需读取`,
    );
    setDocumentBusy(false);
  }, [activeSession, documentBusy]);

  useEffect(() => {
    const handleNativeDocumentDrop = (event: Event) => {
      const paths = (event as CustomEvent<{ paths?: string[] }>).detail?.paths ?? [];
      void importDocumentPaths(paths);
    };
    window.addEventListener("drpa-agent-document-drop", handleNativeDocumentDrop);
    return () => window.removeEventListener("drpa-agent-document-drop", handleNativeDocumentDrop);
  }, [importDocumentPaths]);

  const selectDocuments = async () => {
    if (!documentGateway.selectAgentDocumentFiles) {
      setDocumentNotice("当前运行环境尚未接入文档选择器");
      return;
    }
    try {
      const paths = await documentGateway.selectAgentDocumentFiles();
      if (paths.length > 0) await importDocumentPaths(paths);
    } catch (reason) {
      setDocumentNotice(`选择文档失败：${String(reason)}`);
    }
  };

  const removeAttachment = async (attachmentId: string) => {
    if (!activeSession || busy || documentBusy) return;
    setDocumentBusy(true);
    try {
      await documentGateway.deleteAgentAttachment(activeSession.id, attachmentId);
      setAttachmentsBySession((current) => ({
        ...current,
        [activeSession.id]: (current[activeSession.id] ?? [])
          .filter((attachment) => attachment.id !== attachmentId),
      }));
    } catch (reason) {
      setDocumentNotice(`删除对话附件失败：${String(reason)}`);
    } finally {
      setDocumentBusy(false);
    }
  };

  const exportArtifact = async (artifact: AgentDocumentArtifact) => {
    if (
      !activeSession
      || !documentGateway.selectAgentArtifactExportPath
      || !documentGateway.exportAgentArtifact
    ) {
      setDocumentNotice("当前运行环境尚未接入文档导出能力");
      return;
    }
    setExportingArtifactId(artifact.id);
    setDocumentNotice("");
    try {
      const destination = await documentGateway.selectAgentArtifactExportPath(artifact.name);
      if (!destination) return;
      const exported = await documentGateway.exportAgentArtifact(
        activeSession.id,
        artifact.id,
        destination,
      );
      setDocumentNotice(`已导出到 ${exported.path}`);
    } catch (reason) {
      setDocumentNotice(`导出文档失败：${String(reason)}`);
    } finally {
      setExportingArtifactId("");
    }
  };

  const handleDocumentDrop = (event: DragEvent<HTMLElement>) => {
    event.preventDefault();
    setDocumentDragActive(false);
    const paths = droppedDocumentPaths(event);
    if (paths.length === 0) {
      setDocumentNotice("未取得本地文件路径；请点击回形针并通过系统文件选择器添加文档");
      return;
    }
    void importDocumentPaths(paths);
  };

  const createBoundConversation = useCallback(async (projectId?: string) => {
    if (busy || sessionLoadingId === "new") return;
    const targetProjectId = projectId ?? (embedded ? embeddedProjectId : "");
    setSessionLoadingId("new");
    try {
      const session = await agentRuntime.createSession(targetProjectId);
      setSurfaceSessionId(session.id);
      agentRuntime.setSurfaceSelection(surfaceId, session.id);
      if (!embedded) selectAgentConversation(session.id);
      if (targetProjectId) void refreshProjectIndex();
    } catch (reason) {
      setError(`新建会话失败：${String(reason)}`);
    } finally {
      setSessionLoadingId("");
    }
  }, [
    agentRuntime,
    busy,
    embedded,
    embeddedProjectId,
    refreshProjectIndex,
    selectAgentConversation,
    sessionLoadingId,
    surfaceId,
  ]);

  useEffect(() => {
    if (!embedded || !sessionIndexReady || !embeddedProjectId) return;
    const bound = agentSessions.find((session) => session.projectId === embeddedProjectId);
    if (bound) {
      if (activeSession?.id !== bound.id) void openSession(bound.id);
      return;
    }
    void createBoundConversation(embeddedProjectId);
  }, [
    activeSession?.id,
    agentSessions,
    createBoundConversation,
    embedded,
    embeddedProjectId,
    openSession,
    sessionIndexReady,
  ]);

  const finishRenameSession = useCallback(async (sessionId: string, title: string) => {
    setRenamingId("");
    try {
      await agentRuntime.renameSession(sessionId, title);
    } catch (reason) {
      setError(`重命名会话失败：${String(reason)}`);
    }
  }, [agentRuntime]);

  const moveSessionToProject = useCallback(async (sessionId: string, projectId: string) => {
    const previousProjectId = useAppStore.getState().agentSessions
      .find((session) => session.id === sessionId)?.projectId ?? "";
    setAgentConversationProject(sessionId, projectId);
    try {
      await agentRuntime.moveSession(sessionId, projectId);
      await refreshProjectIndex();
    } catch (reason) {
      setAgentConversationProject(sessionId, previousProjectId);
      setError(`移动会话失败：${String(reason)}`);
    }
  }, [agentRuntime, refreshProjectIndex, setAgentConversationProject]);

  const toggleSessionSkill = useCallback(async (skillId: string) => {
    if (!activeSession || !agentRuntime.isLoaded(activeSession.id)) return;
    const selected = activeSession.selectedSkillIds ?? [];
    const next = selected.includes(skillId)
      ? selected.filter((item) => item !== skillId)
      : [...selected, skillId];
    setAgentConversationSkills(activeSession.id, next);
    await persistSession(activeSession.id).catch(() => undefined);
  }, [activeSession, agentRuntime, persistSession, setAgentConversationSkills]);

  const openCreateProject = useCallback(() => {
    setProjectContextMenu(null);
    setProjectDraft("");
    setProjectDialog({ mode: "create" });
  }, []);

  const openRenameProject = useCallback((projectId: string) => {
    const projectOption = availableProjectOptions.find((item) => item.id === projectId);
    const project = agentProjects.find((item) => item.id === projectId);
    if (projectOption?.kind === "rpaz") {
      setProjectContextMenu(null);
      setError("RPAZ 项目的名称由开发工作室管理；这里可以新建独立的通用项目。");
      return;
    }
    if (!project) {
      setProjectContextMenu(null);
      setError("这个通用目录尚未登记为 Agent 项目，请先在项目栏中新建或关联会话。");
      return;
    }
    setProjectContextMenu(null);
    setProjectDraft(project.name);
    setProjectDialog({ mode: "rename", projectId });
  }, [agentProjects, availableProjectOptions]);

  const submitProjectDialog = useCallback(async () => {
    const name = projectDraft.trim();
    if (!projectDialog || !name || projectBusy) return;
    setProjectBusy(true);
    try {
      if (projectDialog.mode === "create") {
        const project = await desktopGateway.createAgentProject(name);
        setAgentProjects((current) => [...current, project]);
      } else if (projectDialog.projectId) {
        const project = await desktopGateway.renameAgentProject(projectDialog.projectId, name);
        setAgentProjects((current) => current.map((item) => (
          item.id === project.id ? project : item
        )));
      }
      setProjectDialog(null);
      setProjectDraft("");
    } catch (reason) {
      setError(`${projectDialog.mode === "create" ? "创建" : "重命名"}项目失败：${String(reason)}`);
    } finally {
      setProjectBusy(false);
    }
  }, [projectBusy, projectDialog, projectDraft]);

  useEffect(() => {
    if (!projectContextMenu) return;
    const close = () => setProjectContextMenu(null);
    window.addEventListener("click", close);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("blur", close);
    };
  }, [projectContextMenu]);

  const confirmConversationAction = async () => {
    if (!confirmation) return;
    if (confirmation.kind === "delete") {
      try {
        const beforeDelete = useAppStore.getState().agentSessions;
        const deletingSurfaceSelection = surfaceSessionId === confirmation.sessionId;
        if (beforeDelete.length === 1) {
          const replacement = await agentRuntime.createSession(embedded ? embeddedProjectId : "");
          setSurfaceSessionId(replacement.id);
          agentRuntime.setSurfaceSelection(surfaceId, replacement.id);
        }
        await agentRuntime.deleteSession(confirmation.sessionId);
        if (deletingSurfaceSelection) {
          const remaining = useAppStore.getState().agentSessions;
          const replacement = embedded && embeddedProjectId
            ? remaining.find((session) => session.projectId === embeddedProjectId)
            : remaining.find((session) => session.id === useAppStore.getState().activeAgentSessionId)
              ?? remaining[0];
          if (replacement) void openSession(replacement.id);
        }
        setAttachmentsBySession((current) => {
          const next = { ...current };
          delete next[confirmation.sessionId];
          return next;
        });
        setArtifactsBySession((current) => {
          const next = { ...current };
          delete next[confirmation.sessionId];
          return next;
        });
        void refreshProjectIndex();
      } catch (reason) {
        setError(`删除会话失败：${String(reason)}`);
      }
    } else {
      clearAgentConversation(confirmation.sessionId);
      await persistSession(confirmation.sessionId).catch(() => undefined);
    }
    setConfirmation(null);
  };

  const runTurn = async (sessionId: string, history: AgentConversationMessage[]) => {
    if (busy) return;
    if (!agentBaseUrl.trim() || !agentModel.trim()) {
      setError("请先填写 OpenAI 兼容 URL 和模型名称");
      return;
    }
    setAgentConversationMessages(sessionId, history);
    setError("");
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    await persistSession(sessionId).catch(() => undefined);
    const requestId = `req-${Date.now()}-${Math.random().toString(16).slice(2)}`;
    try {
      const result = await agentRuntime.runTurn({
        requestId,
        sessionId,
        baseUrl: agentBaseUrl.trim(),
        model: agentModel.trim(),
        apiKey,
        providerRef: agentProviderRef,
        mode: agentMode,
        projectId: embedded
          ? embeddedProjectId
          : useAppStore.getState().agentSessions.find((session) => session.id === sessionId)?.projectId ?? agentProjectId,
        stream: agentStreamEnabled,
        contextWindow: agentContextWindow,
        maxOutputTokens: agentMaxOutputTokens,
        maxRounds: agentMaxRounds,
        temperature: agentTemperature,
        pythonTimeoutSeconds: agentPythonTimeoutSeconds,
        maxToolCalls: agentMaxToolCalls,
        maxWallTimeSeconds: agentMaxWallTimeSeconds,
        selectedSkillIds: useAppStore.getState().agentSessions.find((session) => session.id === sessionId)?.selectedSkillIds ?? [],
        toolPolicy: agentToolPolicy,
        messages: history.map((message) => ({
          role: message.role,
          content: modelMessageContent(message),
        })),
      });
      setAgentConversationMessages(sessionId, [...history, {
        id: messageId("assistant"),
        role: "assistant",
        content: result.message,
        tools: result.tools,
        durationMs: result.durationMs,
        tokens: result.usage.promptTokens + result.usage.completionTokens,
        run: {
          requestId,
          stopReason: result.stopReason,
          rounds: result.rounds,
          toolCalls: result.toolCalls,
        },
      }]);
      await persistSession(sessionId).catch(() => undefined);
    } catch (reason) {
      setError(String(reason).includes("运行已取消")
        ? "Agent 运行已取消"
        : `Agent 请求失败：${String(reason)}`);
    } finally {
      void refreshArtifacts(sessionId);
    }
  };

  const cancelCurrentRun = async () => {
    if (!currentRequestId) return;
    try {
      await agentRuntime.cancelRun(runSessionId);
    } catch (reason) {
      setError(`取消 Agent 运行失败：${String(reason)}`);
    }
  };

  const send = async (preset?: string) => {
    const visibleContent = (preset ?? draft).trim()
      || (attachments.length > 0 ? "请读取并处理已附加的文档。" : "");
    if (!visibleContent || busy || documentBusy || !activeSession) return;
    const content = withAttachmentContext(visibleContent, attachments);
    const userMessage: AgentConversationMessage = { id: messageId("user"), role: "user", content };
    const history = [...messages, userMessage];
    if (activeSession.title === "新对话") {
      const generatedTitle = visibleContent.replace(/\s+/g, " ").slice(0, 30);
      renameAgentConversation(activeSession.id, generatedTitle);
      await finishRenameSession(activeSession.id, generatedTitle);
    }
    updateDraft("");
    await runTurn(activeSession.id, history);
  };

  const regenerate = async () => {
    if (!activeSession || busy) return;
    const userIndex = latestUserIndex(messages);
    if (userIndex < 0) return;
    await runTurn(activeSession.id, messages.slice(0, userIndex + 1));
  };

  const submitEditedMessage = async () => {
    if (!activeSession || busy) return;
    const content = editingDraft.trim();
    const userIndex = messages.findIndex((message) => message.id === editingMessageId && message.role === "user");
    if (!content || userIndex < 0) return;
    const history = messages.slice(0, userIndex + 1).map((message, index) => (
      index === userIndex
        ? { ...message, content: replaceVisibleMessageContent(message.content, content) }
        : message
    ));
    setEditingMessageId("");
    setEditingDraft("");
    await runTurn(activeSession.id, history);
  };

  const lastUserIndex = latestUserIndex(messages);
  const latestUserMessageId = lastUserIndex >= 0 ? messages[lastUserIndex].id : undefined;
  const toolIsActive = (name: string) => {
    if (!agentToolPolicy.enabled) return false;
    if (name === "rpaz_validate" || name === "rpaz_build") {
      return agentToolPolicy.projectWrite && selectedProject?.kind === "rpaz";
    }
    if (name === "rpaz_python") return agentToolPolicy.python && Boolean(selectedProject);
    if (name === "rpaz_write_file" || name === "edit_file") return agentToolPolicy.projectWrite && Boolean(selectedProject);
    if (name === "read_file" || name === "find_files" || name === "search_text") return agentToolPolicy.arbitraryFileRead && Boolean(selectedProject);
    if (name === "rpaz_list_files") return Boolean(selectedProject);
    if (name === "data_create_connection") return agentToolPolicy.databaseConnections;
    if (name.startsWith("data_")) return agentToolPolicy.databaseRead;
    if (name.startsWith("knowledge_base_")) return agentToolPolicy.knowledgeBaseRead;
    if (name === "knowledge_write_document" || name === "agent_write_skill" || name === "agent_write_memory" || name === "agent_remember") return agentToolPolicy.workspaceWrite;
    if (name === "document_read") return agentToolPolicy.documentRead;
    if (name === "document_create") return agentToolPolicy.documentWrite;
    if (name === "document_convert") return agentToolPolicy.documentConvert;
    return name.startsWith("agent_") || name.startsWith("knowledge_");
  };
  const activeToolCount = Object.keys(toolLabels).filter(toolIsActive).length;
  const selectedSkillIds = activeSession?.selectedSkillIds ?? [];

  const renderSessionRow = (
    session: AgentConversationSession,
    nested = false,
    quickLink = false,
  ) => (
    <div
      className={`agent-session-row ${nested ? "nested" : ""} ${session.id === activeSession?.id ? "active" : ""}`}
      key={`${nested ? "nested" : "recent"}-${session.id}`}
    >
      {!quickLink && renamingId === session.id ? (
        <input
          autoFocus
          aria-label="对话名称"
          value={renameDraft}
          onChange={(event) => setRenameDraft(event.target.value)}
          onBlur={() => void finishRenameSession(session.id, renameDraft)}
          onKeyDown={(event) => {
            if (event.key === "Enter") void finishRenameSession(session.id, renameDraft);
            if (event.key === "Escape") setRenamingId("");
          }}
        />
      ) : (
        <button
          className="agent-session-select"
          type="button"
          aria-label={quickLink ? `最近会话 ${session.title}` : `打开对话 ${session.title}`}
          onClick={() => void openSession(session.id)}
          disabled={busy || !sessionIndexReady}
        >
          <strong>{session.title}</strong>
          <span>
            {sessionLoadingId === session.id
              ? <><LoaderCircle className="spin" size={10} /> 正在载入</>
              : <><Clock3 size={10} /> {formatSessionTime(session.updatedAt)} · {session.messageCount ?? session.messages.length} 条</>}
          </span>
        </button>
      )}
      {!quickLink && (
        <div className="agent-session-actions">
          <button type="button" aria-label={`重命名对话 ${session.title}`} onClick={() => { setRenamingId(session.id); setRenameDraft(session.title); }} disabled={busy}><Pencil size={11} /></button>
          <button type="button" aria-label={`删除对话 ${session.title}`} onClick={() => setConfirmation({ kind: "delete", sessionId: session.id, title: session.title })} disabled={busy}><Trash2 size={11} /></button>
        </div>
      )}
    </div>
  );

  if (embedded) {
    return (
      <section className="studio-agent agent-conversation" aria-label="开发工作室 AI Agent">
        <header className="studio-agent-header">
          <div><Bot size={15} /><span><strong>{agentMode === "developer" ? "JCode Agent" : "RPAZ Agent"}</strong><small>{agentMode === "developer" ? "完整开发工具" : selectedProject ? "当前项目已绑定" : "未选择项目"}</small></span></div>
          <div className="studio-agent-mode" role="group" aria-label="工作室 Agent 模式"><button type="button" className={agentMode === "rpaz" ? "active" : ""} onClick={() => setAgentMode("rpaz")} disabled={busy}>RPAZ</button><button type="button" className={agentMode === "developer" ? "active" : ""} onClick={() => setAgentMode("developer")} disabled={busy}>JCode</button></div>
          <select aria-label="工作室 Agent 对话" value={embeddedSessions.some((session) => session.id === activeSession?.id) ? activeSession?.id : ""} onChange={(event) => { if (event.target.value) void openSession(event.target.value); }} disabled={busy || embeddedSessions.length === 0}>
            {embeddedSessions.length === 0 && <option value="">正在准备项目会话…</option>}
            {embeddedSessions.map((session) => <option value={session.id} key={session.id}>{session.title}</option>)}
          </select>
          <button type="button" title="新建对话" aria-label="新建工作室 Agent 对话" onClick={() => void createBoundConversation()} disabled={busy}><Plus size={13} /></button>
          <button type="button" title="清空当前对话" aria-label="清空工作室 Agent 对话" onClick={() => activeSession && setConfirmation({ kind: "clear", sessionId: activeSession.id, title: activeSession.title })} disabled={busy || messages.length === 0}><Trash2 size={13} /></button>
        </header>
        <div className="agent-transcript studio-agent-transcript" ref={transcriptRef} aria-live="polite">
          {messages.length === 0 && !busy && (
            <div className="agent-welcome studio-agent-welcome">
              <div className="agent-orbit"><Sparkles size={20} /></div>
              <h2>{agentMode === "developer" ? "用 JCode 完成开发任务" : "和 Agent 一起开发"}</h2>
              <p>{agentMode === "developer" ? `JCode 将在“${selectedProject?.name ?? embeddedProjectName}”目录运行完整开发工具。` : selectedProject ? `已绑定“${selectedProject.name}”，可直接读取、修改和运行 Python；RPAZ 项目还可校验与构建。` : "选择一个开发项目后，Agent 会自动绑定当前工作区。"}</p>
              <div className="agent-suggestions">{suggestions.map((suggestion) => <button type="button" key={suggestion} onClick={() => void send(suggestion)}><MessageSquarePlus size={13} /><span>{suggestion}</span></button>)}</div>
            </div>
          )}
          {messages.map((message, index) => (
            <article className={`agent-message ${message.role}`} key={message.id}>
              <div className="agent-message-avatar">{message.role === "assistant" ? <Bot size={14} /> : "你"}</div>
              <div className="agent-message-body">
                <header>
                  <strong>{message.role === "assistant" ? agentMode === "developer" ? "JCode Agent" : "DRPA Agent" : "你"}</strong>
                  {message.role === "assistant" && <span>{message.durationMs} ms · {message.tokens ?? 0} tokens</span>}
                  <span className="agent-message-actions">
                    {message.role === "user" && message.id === latestUserMessageId && <button type="button" aria-label="编辑最新消息" onClick={() => { setEditingMessageId(message.id); setEditingDraft(visibleMessageContent(message.content)); }} disabled={busy}><Pencil size={12} /></button>}
                    {((message.role === "assistant" && index === messages.length - 1) || (message.role === "user" && message.id === latestUserMessageId && index === messages.length - 1)) && <button type="button" aria-label="重新生成回复" onClick={() => void regenerate()} disabled={busy}><RotateCcw size={12} /></button>}
                  </span>
                </header>
                {message.tools && message.tools.length > 0 && <div className="agent-tool-events">{message.tools.map((tool) => <ToolEvent event={tool} key={tool.callId} />)}</div>}
                {editingMessageId === message.id ? (
                  <div className="agent-message-editor">
                    <textarea aria-label="编辑最新用户消息" autoFocus value={editingDraft} onChange={(event) => setEditingDraft(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) { event.preventDefault(); void submitEditedMessage(); } }} />
                    <footer><button className="button ghost small" type="button" onClick={() => setEditingMessageId("")}>取消</button><button className="button primary small" type="button" onClick={() => void submitEditedMessage()} disabled={!editingDraft.trim()}>重新生成</button></footer>
                  </div>
                ) : <AgentMarkdown content={visibleMessageContent(message.content)} hideReasoning={message.role === "assistant"} />}
              </div>
            </article>
          ))}
          {busy && (
            <article className="agent-message assistant pending">
              <div className="agent-message-avatar"><Bot size={14} /></div>
              <div className="agent-message-body">
                <header><strong>{agentMode === "developer" ? "JCode Agent" : "DRPA Agent"}</strong><span>{agentStreamEnabled ? "流式生成中" : "模型与本地工具协同中"}</span></header>
                {streamingTools.length > 0 && <div className="agent-tool-events">{streamingTools.map((tool) => <ToolEvent event={tool} key={tool.callId} />)}</div>}
                {streamingContent ? <AgentMarkdown content={streamingContent} streaming hideReasoning /> : <div className="agent-thinking"><LoaderCircle className="spin" size={14} /> 正在分析任务…</div>}
              </div>
            </article>
          )}
        </div>
        <div className="agent-composer-wrap studio-agent-composer-wrap">
          {error && <div className="agent-error"><CircleAlert size={13} />{error}<button type="button" onClick={() => setError("")}>×</button></div>}
          {documentNotice && <div className="agent-document-notice"><FileText size={13} /><span>{documentNotice}</span><button type="button" aria-label="关闭文档提示" onClick={() => setDocumentNotice("")}><X size={12} /></button></div>}
          <div
            className={`agent-composer ${documentDragActive ? "document-drag-active" : ""}`}
            onDragEnter={(event) => { event.preventDefault(); setDocumentDragActive(true); }}
            onDragOver={(event) => { event.preventDefault(); event.dataTransfer.dropEffect = "copy"; }}
            onDragLeave={(event) => {
              if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
                setDocumentDragActive(false);
              }
            }}
            onDrop={handleDocumentDrop}
          >
            <DocumentComposerContext
              attachments={attachments}
              artifacts={artifacts}
              documentBusy={documentBusy || busy}
              exportingArtifactId={exportingArtifactId}
              onRemoveAttachment={removeAttachment}
              onExportArtifact={(artifact) => void exportArtifact(artifact)}
              onRefreshArtifacts={() => activeSession && void refreshArtifacts(activeSession.id)}
            />
            <textarea value={draft} onChange={(event) => updateDraft(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) { event.preventDefault(); void send(); } }} placeholder={!activeSession ? "正在准备项目会话…" : selectedProject ? `向 Agent 描述“${selectedProject.name}”的开发任务…` : "先选择左侧开发项目…"} disabled={busy || !selectedProject || !activeSession} />
            <SkillPicker
              skills={availableSkills}
              selectedSkillIds={selectedSkillIds}
              disabled={busy || !activeSession || sessionLoadingId === activeSession?.id}
              onToggle={(skillId) => void toggleSessionSkill(skillId)}
            />
            <footer><button className="agent-attach-trigger" type="button" title="添加 PDF、Word、Excel 或 PowerPoint" aria-label="添加对话文档" onClick={() => void selectDocuments()} disabled={busy || documentBusy}><Paperclip size={14} /></button><span><Code2 size={12} /> {selectedProject ? "当前项目" : "未绑定项目"}</span><small>Ctrl + Enter</small>{busy ? <button type="button" aria-label="取消 Agent 运行" onClick={() => void cancelCurrentRun()} disabled={!currentRequestId}><XCircle size={14} /></button> : <button type="button" aria-label="发送工作室 Agent 消息" onClick={() => void send()} disabled={documentBusy || (!draft.trim() && attachments.length === 0) || !selectedProject}><Send size={14} /></button>}</footer>
          </div>
        </div>
        {confirmation && (
          <div className="knowledge-confirm-overlay studio-agent-confirm" role="dialog" aria-modal="true" aria-label="确认清空对话">
            <section className="knowledge-confirm">
              <div className="knowledge-confirm-icon"><CircleAlert size={18} /></div>
              <div><h2>清空当前对话？</h2><p>“{confirmation.title}”的全部消息将被清空，会话本身会保留。</p></div>
              <footer><button className="button secondary" type="button" onClick={() => setConfirmation(null)}>取消</button><button className="button danger" type="button" onClick={confirmConversationAction}>确认清空</button></footer>
            </section>
          </div>
        )}
      </section>
    );
  }

  return (
    <div className="page agent-page">
      <header className="agent-header">
        <div className="agent-title-mark"><Bot size={19} /></div>
        <div>
          <span className="eyebrow">Infrastructure / Local Tooling</span>
          <h1>AI Agent</h1>
          <p>面向通用项目、数据分析与 RPAZ 自动化的本地持久化智能工作台。</p>
        </div>
        <span className="agent-beta-badge">BETA</span>
        <div className="agent-mode-switch" role="group" aria-label="Agent 模式"><button type="button" className={agentMode === "rpaz" ? "active" : ""} onClick={() => setAgentMode("rpaz")} disabled={busy}>RPAZ Agent</button><button type="button" className={agentMode === "developer" ? "active" : ""} onClick={() => setAgentMode("developer")} disabled={busy}>JCode 开发者</button></div>
        <div className="agent-connection-state"><span /> OpenAI Compatible</div>
        <button className="button ghost small" type="button" aria-label={agentInspectorOpen ? "隐藏 Agent 配置" : "显示 Agent 配置"} onClick={toggleAgentInspector}>{agentInspectorOpen ? <PanelRightClose size={13} /> : <PanelRightOpen size={13} />} {agentInspectorOpen ? "隐藏配置" : "显示配置"}</button>
        <button className="button ghost small" type="button" onClick={() => activeSession && setConfirmation({ kind: "clear", sessionId: activeSession.id, title: activeSession.title })} disabled={messages.length === 0 || busy}><Trash2 size={13} /> 清空对话</button>
      </header>

      <div className={`agent-layout ${agentInspectorOpen ? "" : "config-hidden"}${sessionsCollapsed ? " sessions-collapsed" : ""}`}>
        {sessionsCollapsed ? <SidebarToggle id="agent-sessions" side="left" label="Agent 对话侧边栏" restore /> : <aside className="agent-sessions collapsible-sidebar" aria-label="Agent 对话列表">
          <SidebarToggle id="agent-sessions" side="left" label="Agent 对话侧边栏" />
          <header><div><MessageSquarePlus size={14} /><strong>对话</strong></div><span>{sessionIndexReady ? "SESSION.DB" : "LOADING"}</span></header>
          <div
            className="agent-session-list agent-session-browser"
            onContextMenu={(event) => {
              event.preventDefault();
              setProjectContextMenu({ x: event.clientX, y: event.clientY });
            }}
          >
            <button className="agent-new-chat" type="button" aria-label="新建 Agent 对话" onClick={() => void createBoundConversation("")} disabled={busy || !sessionIndexReady || sessionLoadingId === "new"}>
              {sessionLoadingId === "new" ? <LoaderCircle className="spin" size={13} /> : <MessageSquarePlus size={13} />}
              <span><strong>新建普通会话</strong><small>不绑定项目，随时可以移动</small></span>
            </button>

            <section className="agent-project-browser" aria-label="Agent 项目">
              <header>
                <span><FolderCode size={12} /> 项目</span>
                <button type="button" aria-label="新建 Agent 项目" title="新建项目" onClick={(event) => { event.stopPropagation(); openCreateProject(); }}><FolderPlus size={12} /></button>
              </header>
              {availableProjectOptions.length === 0 && (
                <button className="agent-project-empty" type="button" onClick={openCreateProject}>
                  <FolderPlus size={14} /> 创建第一个项目
                </button>
              )}
              {availableProjectOptions.map((project) => {
                const projectSessions = sessionsByProject.get(project.id) ?? [];
                return (
                  <details
                    className="agent-project-group"
                    key={project.id}
                    open={expandedProjectIds.has(project.id)}
                    onToggle={(event) => {
                      const isOpen = event.currentTarget.open;
                      setExpandedProjectIds((current) => {
                        const next = new Set(current);
                        if (isOpen) next.add(project.id);
                        else next.delete(project.id);
                        return next;
                      });
                    }}
                    onContextMenu={(event) => {
                      event.preventDefault();
                      event.stopPropagation();
                      setProjectContextMenu({ x: event.clientX, y: event.clientY, projectId: project.id });
                    }}
                  >
                    <summary>
                      <ChevronDown size={11} />
                      <Folder size={13} />
                      <span><strong>{project.name}</strong><small>{project.kind === "rpaz" ? "RPAZ 子集" : "通用项目"} · {projectSessions.length} 个会话</small></span>
                      <button type="button" aria-label={`在 ${project.name} 新建会话`} onClick={(event) => { event.preventDefault(); event.stopPropagation(); void createBoundConversation(project.id); }}><Plus size={11} /></button>
                    </summary>
                    <div>
                      {projectSessions.length > 0
                        ? projectSessions.map((session) => renderSessionRow(session, true))
                        : <p>右侧 + 可新建项目会话</p>}
                    </div>
                  </details>
                );
              })}
            </section>

            <section className="agent-recent-browser" aria-label="最近会话">
              <header><span><Clock3 size={12} /> 最近会话</span><em>{agentSessions.length}</em></header>
              {recentSessions.map((session) => renderSessionRow(session, false, Boolean(session.projectId)))}
            </section>
          </div>
          <footer>{agentSessions.length} 个持久化会话 · 无数量上限 · 按需载入正文</footer>
        </aside>}

        <section className="agent-conversation" aria-label="AI Agent 对话">
          <div className="agent-transcript" ref={transcriptRef} aria-live="polite">
            {messages.length === 0 && !busy && (
              <div className="agent-welcome">
                <div className="agent-orbit"><Sparkles size={24} /></div>
                <h2>{agentMode === "developer" ? "JCode 开发者 Agent" : "从一次对话或一个项目开始"}</h2>
                <p>{agentMode === "developer" ? selectedProject ? `JCode 已绑定“${selectedProject.name}”，将在项目目录使用完整文件、命令和开发工具。` : "JCode 将以当前 DRPA 工作区为工作目录，使用完整开发工具处理任务。" : selectedProject ? `Agent 已绑定“${selectedProject.name}”，可以使用 Skills、知识库、项目文件与 Python；RPAZ 是其中的规范化项目子集。` : "普通会话可直接问答和处理文档；绑定任意项目后即可使用项目文件与 Python 工具。"}</p>
                <div className="agent-suggestions">
                  {suggestions.map((suggestion) => <button type="button" key={suggestion} onClick={() => void send(suggestion)}><MessageSquarePlus size={14} /><span>{suggestion}</span></button>)}
                </div>
              </div>
            )}
            {messages.map((message, index) => (
              <article className={`agent-message ${message.role}`} key={message.id}>
                <div className="agent-message-avatar">{message.role === "assistant" ? <Bot size={15} /> : "你"}</div>
                <div className="agent-message-body">
                  <header>
                    <strong>{message.role === "assistant" ? agentMode === "developer" ? "JCode Agent" : "DRPA Agent" : "你"}</strong>
                    {message.role === "assistant" && <span>{message.durationMs} ms · {message.tokens ?? 0} tokens</span>}
                    <span className="agent-message-actions">
                      {message.role === "user" && message.id === latestUserMessageId && <button type="button" aria-label="编辑最新消息" title="编辑并重新生成" onClick={() => { setEditingMessageId(message.id); setEditingDraft(visibleMessageContent(message.content)); }} disabled={busy}><Pencil size={12} /></button>}
                      {message.role === "user" && message.id === latestUserMessageId && index === messages.length - 1 && <button type="button" aria-label="重新生成回复" title="重新生成" onClick={() => void regenerate()} disabled={busy}><RotateCcw size={12} /></button>}
                      {message.role === "assistant" && index === messages.length - 1 && <button type="button" aria-label="重新生成回复" title="重新生成" onClick={() => void regenerate()} disabled={busy}><RotateCcw size={12} /></button>}
                    </span>
                  </header>
                  {message.tools && message.tools.length > 0 && <div className="agent-tool-events">{message.tools.map((tool) => <ToolEvent event={tool} key={tool.callId} />)}</div>}
                  {editingMessageId === message.id ? (
                    <div className="agent-message-editor">
                      <textarea aria-label="编辑最新用户消息" autoFocus value={editingDraft} onChange={(event) => setEditingDraft(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) { event.preventDefault(); void submitEditedMessage(); } }} />
                      <footer><span>提交后会从这条消息重新生成</span><button className="button ghost small" type="button" onClick={() => setEditingMessageId("")}>取消</button><button className="button primary small" type="button" onClick={() => void submitEditedMessage()} disabled={!editingDraft.trim()}>保存并重新生成</button></footer>
                    </div>
                  ) : <AgentMarkdown content={visibleMessageContent(message.content)} hideReasoning={message.role === "assistant"} />}
                </div>
              </article>
            ))}
            {busy && (
              <article className="agent-message assistant pending">
                <div className="agent-message-avatar"><Bot size={15} /></div>
                <div className="agent-message-body">
                  <header><strong>{agentMode === "developer" ? "JCode Agent" : "DRPA Agent"}</strong><span>{agentStreamEnabled ? "流式生成中" : "模型与本地工具协同中"}</span></header>
                  {streamingTools.length > 0 && <div className="agent-tool-events">{streamingTools.map((tool) => <ToolEvent event={tool} key={tool.callId} />)}</div>}
                  {streamingContent ? <AgentMarkdown content={streamingContent} streaming hideReasoning /> : <div className="agent-thinking"><LoaderCircle className="spin" size={14} /> 正在分析任务…</div>}
                </div>
              </article>
            )}
          </div>

          <div className="agent-composer-wrap">
            {error && <div className="agent-error"><CircleAlert size={13} />{error}<button type="button" onClick={() => setError("")}>×</button></div>}
            {documentNotice && <div className="agent-document-notice"><FileText size={13} /><span>{documentNotice}</span><button type="button" aria-label="关闭文档提示" onClick={() => setDocumentNotice("")}><X size={12} /></button></div>}
            <div
              className={`agent-composer ${documentDragActive ? "document-drag-active" : ""}`}
              onDragEnter={(event) => { event.preventDefault(); setDocumentDragActive(true); }}
              onDragOver={(event) => { event.preventDefault(); event.dataTransfer.dropEffect = "copy"; }}
              onDragLeave={(event) => {
                if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
                  setDocumentDragActive(false);
                }
              }}
              onDrop={handleDocumentDrop}
            >
              <DocumentComposerContext
                attachments={attachments}
                artifacts={artifacts}
                documentBusy={documentBusy || busy}
                exportingArtifactId={exportingArtifactId}
                onRemoveAttachment={removeAttachment}
                onExportArtifact={(artifact) => void exportArtifact(artifact)}
                onRefreshArtifacts={() => activeSession && void refreshArtifacts(activeSession.id)}
              />
              <textarea
                value={draft}
                onChange={(event) => updateDraft(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
                    event.preventDefault();
                    void send();
                  }
                }}
                placeholder={!sessionIndexReady || !activeSession ? "正在载入会话…" : selectedProject ? `向 Agent 描述“${selectedProject.name}”的开发或分析任务…` : "开始普通对话，可询问 RPAZ、数据分析或文档处理，也可以在右侧绑定任意项目…"}
                disabled={busy || !sessionIndexReady}
              />
              <SkillPicker
                skills={availableSkills}
                selectedSkillIds={selectedSkillIds}
                disabled={busy || !activeSession || sessionLoadingId === activeSession?.id}
                onToggle={(skillId) => void toggleSessionSkill(skillId)}
              />
              <footer><button className="agent-attach-trigger" type="button" title="添加 PDF、Word、Excel 或 PowerPoint" aria-label="添加对话文档" onClick={() => void selectDocuments()} disabled={busy || documentBusy || !sessionIndexReady}><Paperclip size={14} /></button><span><Code2 size={12} /> {selectedProject?.name ?? "未绑定项目"}</span><small>Ctrl + Enter</small>{busy ? <button type="button" aria-label="取消 Agent 运行" onClick={() => void cancelCurrentRun()} disabled={!currentRequestId}><XCircle size={15} /></button> : <button type="button" aria-label="发送消息" onClick={() => void send()} disabled={!sessionIndexReady || documentBusy || (!draft.trim() && attachments.length === 0)}><Send size={15} /></button>}</footer>
            </div>
          </div>
        </section>

        {agentInspectorOpen && <aside className="agent-inspector">
          <header><div><Wrench size={15} /><strong>Agent 配置</strong></div><button type="button" aria-label="收起右侧 Agent 配置" onClick={toggleAgentInspector}><PanelRightClose size={13} /></button></header>
          <div className="agent-inspector-scroll">
            <section className="agent-config-section">
              <h2><Bot size={13} /> Agent 模式</h2>
              <div className="agent-mode-cards"><button type="button" className={agentMode === "rpaz" ? "active" : ""} onClick={() => setAgentMode("rpaz")} disabled={busy}><strong>RPAZ Agent</strong><small>DRPA 内置工具与可配置工具策略</small></button><button type="button" className={agentMode === "developer" ? "active developer" : "developer"} onClick={() => setAgentMode("developer")} disabled={busy}><strong>JCode 开发者 Agent</strong><small>完整文件、命令与开发工具访问</small></button></div>
            </section>

            <section className="agent-config-section">
              <h2><Link2 size={13} /> 模型连接</h2>
              <label><span>OpenAI 兼容 URL</span><input aria-label="OpenAI 兼容 URL" value={agentBaseUrl} onChange={(event) => setAgentBaseUrl(event.target.value)} placeholder="http://127.0.0.1/v1" /></label>
              <label><span>Model</span><div className="agent-input-icon"><Cpu size={13} /><input aria-label="模型名称" value={agentModel} onChange={(event) => setAgentModel(event.target.value)} placeholder="deepseek-v4-flash" /></div></label>
              <label><span>API Key <em>可选</em></span><div className="agent-input-icon"><KeyRound size={13} /><input aria-label="API Key" type="password" autoComplete="off" value={apiKey} onChange={(event) => setApiKey(event.target.value)} placeholder={agentProviderRef ? "由插件宿主安全注入" : "sk-… / 本地服务可留空"} /></div><small>{agentProviderRef ? `凭据引用：${agentProviderRef.pluginId} / ${agentProviderRef.providerId}，密钥不会进入页面。` : "只保留到当前应用会话，不写入磁盘。"}</small></label>
            </section>

            <section className="agent-config-section">
              <h2><Cpu size={13} /> 生成参数 <span>{agentStreamEnabled ? "STREAM" : "BUFFERED"}</span></h2>
              <label className="agent-switch-label"><span>流式输出</span><button className={`switch ${agentStreamEnabled ? "on" : ""}`} type="button" role="switch" aria-label="Agent 流式输出" aria-checked={agentStreamEnabled} onClick={() => setAgentStreamEnabled(!agentStreamEnabled)}><span /></button><small>开启后按增量实时渲染 Markdown。</small></label>
              <label><span>上下文窗口</span><input aria-label="Agent 上下文窗口" type="number" min={1024} max={2000000} step={1024} value={agentContextWindow} onChange={(event) => { if (Number.isFinite(event.currentTarget.valueAsNumber)) setAgentContextWindow(event.currentTarget.valueAsNumber); }} /><small>按模型 token 上限裁剪较早对话，默认 384K（393216）。</small></label>
              <label><span>最大输出 tokens</span><input aria-label="Agent 最大输出 tokens" type="number" min={64} max={131072} step={64} value={agentMaxOutputTokens} onChange={(event) => { if (Number.isFinite(event.currentTarget.valueAsNumber)) setAgentMaxOutputTokens(event.currentTarget.valueAsNumber); }} /><small>默认 98304，可按服务端能力调整。</small></label>
              <label><span>最大模型/工具循环</span><input aria-label="Agent 最大模型工具循环" type="number" min={1} max={256} step={1} value={agentMaxRounds} onChange={(event) => { if (Number.isFinite(event.currentTarget.valueAsNumber)) setAgentMaxRounds(event.currentTarget.valueAsNumber); }} /><small>单次请求默认 64 轮，可配置 1–256。</small></label>
              <label><span>最大工具调用数</span><input aria-label="Agent 最大工具调用数" type="number" min={1} max={4096} step={1} value={agentMaxToolCalls} onChange={(event) => { if (Number.isFinite(event.currentTarget.valueAsNumber)) setAgentMaxToolCalls(event.currentTarget.valueAsNumber); }} /><small>跨全部轮次累计，默认 128；达到上限后进入最终总结。</small></label>
              <label><span>单次运行时限（秒）</span><input aria-label="Agent 单次运行时限秒数" type="number" min={10} max={86400} step={30} value={agentMaxWallTimeSeconds} onChange={(event) => { if (Number.isFinite(event.currentTarget.valueAsNumber)) setAgentMaxWallTimeSeconds(event.currentTarget.valueAsNumber); }} /><small>默认 900 秒；达到时限后结束工具循环并保留事件日志。</small></label>
              <label><span>Python 超时（秒）</span><input aria-label="Agent Python 超时秒数" type="number" min={1} max={86400} step={30} value={agentPythonTimeoutSeconds} onChange={(event) => { if (Number.isFinite(event.currentTarget.valueAsNumber)) setAgentPythonTimeoutSeconds(event.currentTarget.valueAsNumber); }} /><small>默认 300 秒；长时间分析可继续调大。</small></label>
              <label><span>Temperature</span><input aria-label="Agent Temperature" type="number" min={0} max={2} step={0.1} value={agentTemperature} onChange={(event) => { if (Number.isFinite(event.currentTarget.valueAsNumber)) setAgentTemperature(event.currentTarget.valueAsNumber); }} /></label>
            </section>

            <section className="agent-config-section">
              <h2><FolderCode size={13} /> 工作上下文</h2>
              <label><span>项目（通用 / RPAZ）</span><div className="agent-select"><FileCode2 size={13} /><select aria-label="Agent 开发项目" value={agentProjectId} onChange={(event) => activeSession && void moveSessionToProject(activeSession.id, event.target.value)} disabled={!sessionIndexReady || !activeSession || busy || Boolean(sessionLoadingId)}><option value="">不绑定项目</option>{availableProjectOptions.map((project) => <option value={project.id} key={project.id}>{project.name} · {project.kind === "rpaz" ? "RPAZ" : "通用"}</option>)}</select><ChevronDown size={13} /></div><small>任意项目都可使用文件与 Python；RPAZ 项目额外支持规范校验和构建。</small></label>
              {selectedProject && <div className="agent-project-summary"><strong>{selectedProject.name}</strong><span>{selectedProject.kind === "rpaz" ? `RPAZ 项目 · ${selectedProject.files?.length ?? 0} 个索引文件` : "通用 Agent 项目"}</span><code>{selectedProject.path ?? selectedProject.id}</code></div>}
            </section>

            <section className="agent-config-section agent-tools-section">
              <h2><Wrench size={13} /> {agentMode === "developer" ? "JCode 工具" : "内置工具"} <span>{agentMode === "developer" ? "FULL" : `${activeToolCount} ACTIVE`}</span></h2>
              {agentMode === "developer" ? <div className="agent-developer-tool-note"><Code2 size={16} /><span><strong>完整开发工具已启用</strong><small>JCode 可访问当前工作目录中的文件、命令、构建与测试工具；DRPA 内置工具分类开关仅作用于 RPAZ Agent。</small></span></div> : <div className="agent-tool-list">{Object.entries(toolLabels).map(([name, label]) => { const active = toolIsActive(name); return <div className={active ? "active" : ""} key={name}><CheckCircle2 size={12} /><span><strong>{label}</strong><code>{name}</code></span></div>; })}</div>}
            </section>
          </div>
          <footer><span className="agent-limit-dot" /> {agentMode === "developer" ? "JCode 完整开发工具 · 每会话单任务运行" : `单 Agent · ${agentMaxRounds} 轮 / ${agentMaxToolCalls} 次工具 / ${agentMaxWallTimeSeconds} 秒 · Python ${agentPythonTimeoutSeconds} 秒`}</footer>
        </aside>}
      </div>
      {projectContextMenu && (
        <div
          className="agent-project-context-menu"
          role="menu"
          style={{ left: projectContextMenu.x, top: projectContextMenu.y }}
          onClick={(event) => event.stopPropagation()}
        >
          <button type="button" role="menuitem" onClick={openCreateProject}><FolderPlus size={12} /> 新建项目</button>
          {projectContextMenu.projectId && (
            <>
              <button type="button" role="menuitem" onClick={() => { const projectId = projectContextMenu.projectId; setProjectContextMenu(null); if (projectId) void createBoundConversation(projectId); }}><MessageSquarePlus size={12} /> 在项目中新建会话</button>
              <button type="button" role="menuitem" onClick={() => projectContextMenu.projectId && openRenameProject(projectContextMenu.projectId)}><Pencil size={12} /> 重命名项目</button>
            </>
          )}
        </div>
      )}
      {projectDialog && (
        <div className="knowledge-confirm-overlay" role="dialog" aria-modal="true" aria-label={projectDialog.mode === "create" ? "新建 Agent 项目" : "重命名 Agent 项目"}>
          <section className="knowledge-confirm agent-project-dialog">
            <div className="knowledge-confirm-icon"><Layers3 size={18} /></div>
            <div>
              <h2>{projectDialog.mode === "create" ? "新建项目" : "重命名项目"}</h2>
              <p>项目是通用的工作目录；RPAZ 包是带有规范和构建能力的项目子集。</p>
              <input
                autoFocus
                aria-label="Agent 项目名称"
                value={projectDraft}
                onChange={(event) => setProjectDraft(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") void submitProjectDialog();
                  if (event.key === "Escape") setProjectDialog(null);
                }}
                placeholder="例如：月度数据分析"
              />
            </div>
            <footer><button className="button secondary" type="button" onClick={() => setProjectDialog(null)} disabled={projectBusy}>取消</button><button className="button primary" type="button" onClick={() => void submitProjectDialog()} disabled={!projectDraft.trim() || projectBusy}>{projectBusy ? "处理中…" : projectDialog.mode === "create" ? "创建项目" : "保存名称"}</button></footer>
          </section>
        </div>
      )}
      {confirmation && (
        <div className="knowledge-confirm-overlay" role="dialog" aria-modal="true" aria-label={confirmation.kind === "delete" ? "确认删除对话" : "确认清空对话"}>
          <section className="knowledge-confirm">
            <div className="knowledge-confirm-icon"><CircleAlert size={18} /></div>
            <div><h2>{confirmation.kind === "delete" ? "删除这条对话？" : "清空当前对话？"}</h2><p>{confirmation.kind === "delete" ? `“${confirmation.title}”及其全部消息将从本机会话列表删除。` : `“${confirmation.title}”的全部消息将被清空，会话本身会保留。`}</p></div>
            <footer><button className="button secondary" type="button" onClick={() => setConfirmation(null)}>取消</button><button className="button danger" type="button" onClick={confirmConversationAction}>{confirmation.kind === "delete" ? "确认删除" : "确认清空"}</button></footer>
          </section>
        </div>
      )}
    </div>
  );
}

function SkillPicker({
  skills,
  selectedSkillIds,
  disabled,
  onToggle,
}: {
  skills: AgentSkillSummary[];
  selectedSkillIds: string[];
  disabled: boolean;
  onToggle: (skillId: string) => void;
}) {
  const selectedSkills = skills.filter((skill) => selectedSkillIds.includes(skill.name));
  return (
    <div className="agent-skill-picker">
      <details>
        <summary aria-label="选择 Agent Skills">
          <Sparkles size={12} />
          <span>Skills</span>
          <em>{selectedSkillIds.length > 0 ? `已选 ${selectedSkillIds.length}` : "自动选择"}</em>
          <ChevronDown size={11} />
        </summary>
        <div className="agent-skill-options">
          <header><strong>本次会话使用的 Skills</strong><small>不选择时由 Agent 自动匹配</small></header>
          {skills.length === 0 && <p>当前工作区还没有可用 Skill</p>}
          {skills.map((skill) => {
            const selected = selectedSkillIds.includes(skill.name);
            return (
              <button
                className={selected ? "selected" : ""}
                type="button"
                key={skill.name}
                aria-pressed={selected}
                disabled={disabled}
                onClick={() => onToggle(skill.name)}
              >
                {selected ? <CheckCircle2 size={13} /> : <span className="agent-skill-checkbox" />}
                <span>
                  <strong>{skill.displayName || (skill.name === "data-analysis" ? "数据分析" : skill.name)}</strong>
                  <small>{skill.description}</small>
                </span>
                {skill.name === "data-analysis" && <em>DATA</em>}
              </button>
            );
          })}
        </div>
      </details>
      {selectedSkills.length > 0 && (
        <div className="agent-selected-skill-chips" aria-label="已选 Skills">
          {selectedSkills.map((skill) => (
            <button type="button" key={skill.name} onClick={() => onToggle(skill.name)} disabled={disabled} title={`移除 ${skill.displayName || skill.name}`}>
              {skill.displayName || skill.name}<X size={9} />
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function DocumentComposerContext({
  attachments,
  artifacts,
  documentBusy,
  exportingArtifactId,
  onRemoveAttachment,
  onExportArtifact,
  onRefreshArtifacts,
}: {
  attachments: AgentDocumentAttachment[];
  artifacts: AgentDocumentArtifact[];
  documentBusy: boolean;
  exportingArtifactId: string;
  onRemoveAttachment: (attachmentId: string) => void;
  onExportArtifact: (artifact: AgentDocumentArtifact) => void;
  onRefreshArtifacts: () => void;
}) {
  return (
    <div className="agent-document-context">
      <div className="agent-document-context-header">
        <span>
          {documentBusy
            ? <><LoaderCircle className="spin" size={12} /> 正在处理文档…</>
            : <><Upload size={12} /> 可选择或拖入 PDF、DOCX、XLSX、PPTX</>}
        </span>
        <button type="button" aria-label="刷新文档产物" title="刷新文档产物" onClick={onRefreshArtifacts} disabled={documentBusy}><RefreshCcw size={11} /></button>
      </div>
      {attachments.length > 0 && (
        <div className="agent-document-chips" role="list" aria-label="当前对话附件">
          {attachments.map((attachment) => (
            <div className="agent-document-chip" role="listitem" key={attachment.id} title={attachment.id}>
              <FileText size={13} />
              <span><strong>{attachment.name}</strong><small>{attachment.format.toUpperCase()} · {formatDocumentSize(attachment.sizeBytes)}</small></span>
              <button type="button" aria-label={`移除附件 ${attachment.name}`} onClick={() => onRemoveAttachment(attachment.id)} disabled={documentBusy}><X size={11} /></button>
            </div>
          ))}
        </div>
      )}
      {artifacts.length > 0 && (
        <details className="agent-document-artifacts">
          <summary><FileText size={12} /><span>Agent 产物</span><em>{artifacts.length}</em><ChevronDown size={11} /></summary>
          <div>
            {artifacts.map((artifact) => (
              <article key={artifact.id}>
                <FileText size={13} />
                <span><strong>{artifact.name}</strong><small>{artifact.format.toUpperCase()} · {formatDocumentSize(artifact.sizeBytes)}</small></span>
                <button type="button" aria-label={`导出产物 ${artifact.name}`} title="导出到本地" onClick={() => onExportArtifact(artifact)} disabled={Boolean(exportingArtifactId)}>
                  {exportingArtifactId === artifact.id ? <LoaderCircle className="spin" size={12} /> : <Download size={12} />}
                </button>
              </article>
            ))}
          </div>
        </details>
      )}
    </div>
  );
}

function ToolEvent({ event }: { event: AgentToolEvent }) {
  return (
    <details className={`agent-tool-event ${event.status}`}>
      <summary>{event.status === "running" ? <LoaderCircle className="spin" size={13} /> : event.status === "completed" ? <CheckCircle2 size={13} /> : <XCircle size={13} />}<span><strong>{toolLabels[event.name] ?? event.name}</strong><small>{event.summary}</small></span><ChevronDown size={13} /></summary>
      <pre>{event.output}</pre>
    </details>
  );
}

function AgentMarkdown({ content, streaming = false, hideReasoning = false }: { content: string; streaming?: boolean; hideReasoning?: boolean }) {
  const visible = hideReasoning ? visibleAssistantMessageContent(content) : content;
  return <div className={`agent-message-content agent-markdown ${streaming ? "streaming" : ""}`}><Markdown remarkPlugins={[remarkGfm]}>{visible}</Markdown>{streaming && <span className="agent-stream-caret" aria-hidden="true" />}</div>;
}
