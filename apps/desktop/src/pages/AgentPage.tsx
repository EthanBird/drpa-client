import {
  Bot,
  CheckCircle2,
  ChevronDown,
  CircleAlert,
  Clock3,
  Code2,
  Cpu,
  FileCode2,
  FolderCode,
  KeyRound,
  Link2,
  LoaderCircle,
  MessageSquarePlus,
  PanelRightClose,
  PanelRightOpen,
  Pencil,
  Plus,
  RotateCcw,
  Send,
  Sparkles,
  Trash2,
  Wrench,
  XCircle,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { useAppStore } from "../app/store";
import type { AgentConversationMessage, AgentMessage, AgentToolEvent, StudioProject } from "../domain/models";
import { desktopGateway } from "../infra/gateway";

const toolLabels: Record<string, string> = {
  agent_list_skills: "列出 Skills",
  agent_read_skill: "读取 Skill",
  agent_write_skill: "写入 Skill",
  agent_read_memory: "读取长期记忆",
  agent_write_memory: "更新长期记忆",
  knowledge_list_documents: "列出知识文档",
  knowledge_read_document: "读取知识文档",
  knowledge_write_document: "写入知识文档",
  rpaz_list_files: "列出项目文件",
  rpaz_read_file: "读取项目文件",
  rpaz_write_file: "写入项目文件",
  rpaz_validate: "校验 RPAZ",
  rpaz_build: "构建 RPAZ",
  rpaz_python: "运行内置 Python",
};

const suggestions = [
  "检查当前项目的 manifest.yaml 与入口实现，并修复发现的问题",
  "梳理这个 RPAZ 项目的参数和输出，给出改进建议",
  "为当前项目补充一个最小可运行的示例并完成校验",
];

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

export function AgentPage() {
  const agentBaseUrl = useAppStore((state) => state.agentBaseUrl);
  const agentModel = useAppStore((state) => state.agentModel);
  const apiKey = useAppStore((state) => state.agentApiKey);
  const agentStreamEnabled = useAppStore((state) => state.agentStreamEnabled);
  const agentContextWindow = useAppStore((state) => state.agentContextWindow);
  const agentMaxOutputTokens = useAppStore((state) => state.agentMaxOutputTokens);
  const agentTemperature = useAppStore((state) => state.agentTemperature);
  const agentInspectorOpen = useAppStore((state) => state.agentInspectorOpen);
  const agentSessions = useAppStore((state) => state.agentSessions);
  const activeAgentSessionId = useAppStore((state) => state.activeAgentSessionId);
  const setAgentBaseUrl = useAppStore((state) => state.setAgentBaseUrl);
  const setAgentModel = useAppStore((state) => state.setAgentModel);
  const setApiKey = useAppStore((state) => state.setAgentApiKey);
  const setAgentStreamEnabled = useAppStore((state) => state.setAgentStreamEnabled);
  const setAgentContextWindow = useAppStore((state) => state.setAgentContextWindow);
  const setAgentMaxOutputTokens = useAppStore((state) => state.setAgentMaxOutputTokens);
  const setAgentTemperature = useAppStore((state) => state.setAgentTemperature);
  const toggleAgentInspector = useAppStore((state) => state.toggleAgentInspector);
  const createAgentConversation = useAppStore((state) => state.createAgentConversation);
  const selectAgentConversation = useAppStore((state) => state.selectAgentConversation);
  const deleteAgentConversation = useAppStore((state) => state.deleteAgentConversation);
  const renameAgentConversation = useAppStore((state) => state.renameAgentConversation);
  const setAgentConversationProject = useAppStore((state) => state.setAgentConversationProject);
  const setAgentConversationMessages = useAppStore((state) => state.setAgentConversationMessages);
  const clearAgentConversation = useAppStore((state) => state.clearAgentConversation);
  const [projects, setProjects] = useState<StudioProject[]>([]);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [renamingId, setRenamingId] = useState("");
  const [renameDraft, setRenameDraft] = useState("");
  const [confirmation, setConfirmation] = useState<{ kind: "delete" | "clear"; sessionId: string; title: string } | null>(null);
  const [streamingContent, setStreamingContent] = useState("");
  const [streamingTools, setStreamingTools] = useState<AgentToolEvent[]>([]);
  const [editingMessageId, setEditingMessageId] = useState("");
  const [editingDraft, setEditingDraft] = useState("");
  const transcriptRef = useRef<HTMLDivElement>(null);

  const activeSession = useMemo(
    () => agentSessions.find((session) => session.id === activeAgentSessionId) ?? agentSessions[0],
    [activeAgentSessionId, agentSessions],
  );
  const messages = activeSession?.messages ?? [];
  const agentProjectId = activeSession?.projectId ?? "";

  useEffect(() => {
    let active = true;
    void desktopGateway.listStudioProjects().then((items) => {
      if (!active) return;
      setProjects(items);
      if (activeSession && agentProjectId && !items.some((item) => item.id === agentProjectId)) {
        setAgentConversationProject(activeSession.id, "");
      }
    }).catch((reason: unknown) => setError(`读取开发项目失败：${String(reason)}`));
    return () => { active = false; };
  }, [activeSession?.id, agentProjectId, setAgentConversationProject]);

  useEffect(() => {
    const target = transcriptRef.current;
    if (target) target.scrollTop = target.scrollHeight;
  }, [messages, busy, streamingContent, streamingTools]);

  const selectedProject = useMemo(
    () => projects.find((project) => project.id === agentProjectId),
    [agentProjectId, projects],
  );

  const confirmConversationAction = () => {
    if (!confirmation) return;
    if (confirmation.kind === "delete") deleteAgentConversation(confirmation.sessionId);
    else clearAgentConversation(confirmation.sessionId);
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
    setStreamingContent("");
    setStreamingTools([]);
    setBusy(true);
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    const requestId = `req-${Date.now()}-${Math.random().toString(16).slice(2)}`;
    let unlisten: (() => void) | undefined;
    try {
      if (agentStreamEnabled) {
        unlisten = await desktopGateway.listenAgentStream(requestId, (event) => {
          if (event.type === "roundStarted") {
            setStreamingContent("");
          } else if (event.type === "delta") {
            setStreamingContent((current) => current + event.content);
          } else {
            setStreamingTools((current) => [...current.filter((tool) => tool.callId !== event.tool.callId), event.tool]);
          }
        });
      }
      const result = await desktopGateway.runAgentTurn({
        requestId,
        sessionId,
        baseUrl: agentBaseUrl.trim(),
        model: agentModel.trim(),
        apiKey,
        projectId: agentSessions.find((session) => session.id === sessionId)?.projectId ?? agentProjectId,
        stream: agentStreamEnabled,
        contextWindow: agentContextWindow,
        maxOutputTokens: agentMaxOutputTokens,
        temperature: agentTemperature,
        messages: history.map(({ role, content: messageContent }) => ({ role, content: messageContent })),
      });
      setAgentConversationMessages(sessionId, [...history, {
        id: messageId("assistant"),
        role: "assistant",
        content: result.message,
        tools: result.tools,
        durationMs: result.durationMs,
        tokens: result.usage.promptTokens + result.usage.completionTokens,
      }]);
    } catch (reason) {
      setError(`Agent 请求失败：${String(reason)}`);
    } finally {
      unlisten?.();
      setStreamingContent("");
      setStreamingTools([]);
      setBusy(false);
    }
  };

  const send = async (preset?: string) => {
    const content = (preset ?? draft).trim();
    if (!content || busy || !activeSession) return;
    const userMessage: AgentConversationMessage = { id: messageId("user"), role: "user", content };
    const history = [...messages, userMessage];
    if (activeSession.title === "新对话") renameAgentConversation(activeSession.id, content.replace(/\s+/g, " ").slice(0, 30));
    setDraft("");
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
    const history = messages.slice(0, userIndex + 1).map((message, index) => index === userIndex ? { ...message, content } : message);
    setEditingMessageId("");
    setEditingDraft("");
    await runTurn(activeSession.id, history);
  };

  const lastUserIndex = latestUserIndex(messages);
  const latestUserMessageId = lastUserIndex >= 0 ? messages[lastUserIndex].id : undefined;

  return (
    <div className="page agent-page">
      <header className="agent-header">
        <div className="agent-title-mark"><Bot size={19} /></div>
        <div>
          <span className="eyebrow">Infrastructure / Local Tooling</span>
          <h1>AI Agent</h1>
          <p>面向 RPAZ 开发的轻量对话、文件操作、校验、构建与 Python 辅助。</p>
        </div>
        <div className="agent-connection-state"><span /> OpenAI Compatible</div>
        <button className="button ghost small" type="button" aria-label={agentInspectorOpen ? "隐藏 Agent 配置" : "显示 Agent 配置"} onClick={toggleAgentInspector}>{agentInspectorOpen ? <PanelRightClose size={13} /> : <PanelRightOpen size={13} />} {agentInspectorOpen ? "隐藏配置" : "显示配置"}</button>
        <button className="button ghost small" type="button" onClick={() => activeSession && setConfirmation({ kind: "clear", sessionId: activeSession.id, title: activeSession.title })} disabled={messages.length === 0 || busy}><Trash2 size={13} /> 清空对话</button>
      </header>

      <div className={`agent-layout ${agentInspectorOpen ? "" : "config-hidden"}`}>
        <aside className="agent-sessions" aria-label="Agent 对话列表">
          <header><div><MessageSquarePlus size={14} /><strong>对话</strong></div><button type="button" aria-label="新建 Agent 对话" onClick={() => createAgentConversation()} disabled={busy}><Plus size={14} /></button></header>
          <div className="agent-session-list">
            {agentSessions.map((session) => (
              <div className={`agent-session-row ${session.id === activeSession?.id ? "active" : ""}`} key={session.id}>
                {renamingId === session.id ? (
                  <input
                    autoFocus
                    aria-label="对话名称"
                    value={renameDraft}
                    onChange={(event) => setRenameDraft(event.target.value)}
                    onBlur={() => { renameAgentConversation(session.id, renameDraft); setRenamingId(""); }}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") { renameAgentConversation(session.id, renameDraft); setRenamingId(""); }
                      if (event.key === "Escape") setRenamingId("");
                    }}
                  />
                ) : (
                  <button className="agent-session-select" type="button" aria-label={`打开对话 ${session.title}`} onClick={() => selectAgentConversation(session.id)} disabled={busy}>
                    <strong>{session.title}</strong><span><Clock3 size={10} /> {formatSessionTime(session.updatedAt)} · {session.messages.length} 条</span>
                  </button>
                )}
                <div className="agent-session-actions">
                  <button type="button" aria-label={`重命名对话 ${session.title}`} onClick={() => { setRenamingId(session.id); setRenameDraft(session.title); }} disabled={busy}><Pencil size={11} /></button>
                  <button type="button" aria-label={`删除对话 ${session.title}`} onClick={() => setConfirmation({ kind: "delete", sessionId: session.id, title: session.title })} disabled={busy}><Trash2 size={11} /></button>
                </div>
              </div>
            ))}
          </div>
          <footer>{agentSessions.length} 个本地会话 · 最多保留 50 个</footer>
        </aside>

        <section className="agent-conversation" aria-label="AI Agent 对话">
          <div className="agent-transcript" ref={transcriptRef} aria-live="polite">
            {messages.length === 0 && !busy && (
              <div className="agent-welcome">
                <div className="agent-orbit"><Sparkles size={24} /></div>
                <h2>从一个 RPAZ 开发任务开始</h2>
                <p>{selectedProject ? `Agent 已绑定“${selectedProject.name}”，可以按需使用 Skills、记忆、知识库和项目工具。` : "八个 Agent、Skills、记忆与知识库工具已启用；选择右侧开发项目后再启用六个 RPAZ 工具。"}</p>
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
                    <strong>{message.role === "assistant" ? "DRPA Agent" : "你"}</strong>
                    {message.role === "assistant" && <span>{message.durationMs} ms · {message.tokens ?? 0} tokens</span>}
                    <span className="agent-message-actions">
                      {message.role === "user" && message.id === latestUserMessageId && <button type="button" aria-label="编辑最新消息" title="编辑并重新生成" onClick={() => { setEditingMessageId(message.id); setEditingDraft(message.content); }} disabled={busy}><Pencil size={12} /></button>}
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
                  ) : <AgentMarkdown content={message.content} />}
                </div>
              </article>
            ))}
            {busy && (
              <article className="agent-message assistant pending">
                <div className="agent-message-avatar"><Bot size={15} /></div>
                <div className="agent-message-body">
                  <header><strong>DRPA Agent</strong><span>{agentStreamEnabled ? "流式生成中" : "模型与本地工具协同中"}</span></header>
                  {streamingTools.length > 0 && <div className="agent-tool-events">{streamingTools.map((tool) => <ToolEvent event={tool} key={tool.callId} />)}</div>}
                  {streamingContent ? <AgentMarkdown content={streamingContent} streaming /> : <div className="agent-thinking"><LoaderCircle className="spin" size={14} /> 正在分析任务…</div>}
                </div>
              </article>
            )}
          </div>

          <div className="agent-composer-wrap">
            {error && <div className="agent-error"><CircleAlert size={13} />{error}<button type="button" onClick={() => setError("")}>×</button></div>}
            <div className="agent-composer">
              <textarea
                value={draft}
                onChange={(event) => setDraft(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
                    event.preventDefault();
                    void send();
                  }
                }}
                placeholder={selectedProject ? `向 Agent 描述“${selectedProject.name}”的开发任务…` : "询问 RPAZ 开发问题，或先在右侧绑定项目…"}
                disabled={busy}
              />
              <footer><span><Code2 size={12} /> {selectedProject?.name ?? "未绑定项目"}</span><small>Ctrl + Enter</small><button type="button" aria-label="发送消息" onClick={() => void send()} disabled={busy || !draft.trim()}><Send size={15} /></button></footer>
            </div>
          </div>
        </section>

        {agentInspectorOpen && <aside className="agent-inspector">
          <header><div><Wrench size={15} /><strong>Agent 配置</strong></div><button type="button" aria-label="收起右侧 Agent 配置" onClick={toggleAgentInspector}><PanelRightClose size={13} /></button></header>
          <div className="agent-inspector-scroll">
            <section className="agent-config-section">
              <h2><Link2 size={13} /> 模型连接</h2>
              <label><span>OpenAI 兼容 URL</span><input aria-label="OpenAI 兼容 URL" value={agentBaseUrl} onChange={(event) => setAgentBaseUrl(event.target.value)} placeholder="https://api.openai.com/v1" /></label>
              <label><span>Model</span><div className="agent-input-icon"><Cpu size={13} /><input aria-label="模型名称" value={agentModel} onChange={(event) => setAgentModel(event.target.value)} placeholder="gpt-5.4-mini" /></div></label>
              <label><span>API Key <em>可选</em></span><div className="agent-input-icon"><KeyRound size={13} /><input aria-label="API Key" type="password" autoComplete="off" value={apiKey} onChange={(event) => setApiKey(event.target.value)} placeholder="sk-… / 本地服务可留空" /></div><small>只保留到当前应用会话，不写入磁盘。</small></label>
            </section>

            <section className="agent-config-section">
              <h2><Cpu size={13} /> 生成参数 <span>{agentStreamEnabled ? "STREAM" : "BUFFERED"}</span></h2>
              <label className="agent-switch-label"><span>流式输出</span><button className={`switch ${agentStreamEnabled ? "on" : ""}`} type="button" role="switch" aria-label="Agent 流式输出" aria-checked={agentStreamEnabled} onClick={() => setAgentStreamEnabled(!agentStreamEnabled)}><span /></button><small>开启后按增量实时渲染 Markdown。</small></label>
              <label><span>上下文窗口</span><input aria-label="Agent 上下文窗口" type="number" min={1024} max={2000000} step={1024} value={agentContextWindow} onChange={(event) => { if (Number.isFinite(event.currentTarget.valueAsNumber)) setAgentContextWindow(event.currentTarget.valueAsNumber); }} /><small>按模型 token 上限裁剪较早对话，默认 128K。</small></label>
              <label><span>最大输出 tokens</span><input aria-label="Agent 最大输出 tokens" type="number" min={64} max={131072} step={64} value={agentMaxOutputTokens} onChange={(event) => { if (Number.isFinite(event.currentTarget.valueAsNumber)) setAgentMaxOutputTokens(event.currentTarget.valueAsNumber); }} /></label>
              <label><span>Temperature</span><input aria-label="Agent Temperature" type="number" min={0} max={2} step={0.1} value={agentTemperature} onChange={(event) => { if (Number.isFinite(event.currentTarget.valueAsNumber)) setAgentTemperature(event.currentTarget.valueAsNumber); }} /></label>
            </section>

            <section className="agent-config-section">
              <h2><FolderCode size={13} /> 工作上下文</h2>
              <label><span>开发项目</span><div className="agent-select"><FileCode2 size={13} /><select aria-label="Agent 开发项目" value={agentProjectId} onChange={(event) => activeSession && setAgentConversationProject(activeSession.id, event.target.value)}><option value="">不绑定项目</option>{projects.map((project) => <option value={project.id} key={project.id}>{project.name}</option>)}</select><ChevronDown size={13} /></div></label>
              {selectedProject && <div className="agent-project-summary"><strong>{selectedProject.name}</strong><span>{selectedProject.files.length} 个文件</span><code>{selectedProject.id}</code></div>}
            </section>

            <section className="agent-config-section agent-tools-section">
              <h2><Wrench size={13} /> 内置工具 <span>{agentProjectId ? "14 ACTIVE" : "8 ACTIVE"}</span></h2>
              <div className="agent-tool-list">{Object.entries(toolLabels).map(([name, label]) => { const active = name.startsWith("agent_") || name.startsWith("knowledge_") || Boolean(agentProjectId); return <div className={active ? "active" : ""} key={name}><CheckCircle2 size={12} /><span><strong>{label}</strong><code>{name}</code></span></div>; })}</div>
            </section>
          </div>
          <footer><span className="agent-limit-dot" /> 单 Agent · 最多 8 轮工具调用 · Python 30 秒</footer>
        </aside>}
      </div>
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

function ToolEvent({ event }: { event: AgentToolEvent }) {
  return (
    <details className={`agent-tool-event ${event.status}`}>
      <summary>{event.status === "completed" ? <CheckCircle2 size={13} /> : <XCircle size={13} />}<span><strong>{toolLabels[event.name] ?? event.name}</strong><small>{event.summary}</small></span><ChevronDown size={13} /></summary>
      <pre>{event.output}</pre>
    </details>
  );
}

function AgentMarkdown({ content, streaming = false }: { content: string; streaming?: boolean }) {
  return <div className={`agent-message-content agent-markdown ${streaming ? "streaming" : ""}`}><Markdown remarkPlugins={[remarkGfm]}>{content}</Markdown>{streaming && <span className="agent-stream-caret" aria-hidden="true" />}</div>;
}
