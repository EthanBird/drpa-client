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
  Send,
  Sparkles,
  Trash2,
  Wrench,
  XCircle,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { useAppStore } from "../app/store";
import type { AgentConversationMessage, AgentMessage, AgentToolEvent, StudioProject } from "../domain/models";
import { desktopGateway } from "../infra/gateway";

const toolLabels: Record<string, string> = {
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

export function AgentPage() {
  const agentBaseUrl = useAppStore((state) => state.agentBaseUrl);
  const agentModel = useAppStore((state) => state.agentModel);
  const agentInspectorOpen = useAppStore((state) => state.agentInspectorOpen);
  const agentSessions = useAppStore((state) => state.agentSessions);
  const activeAgentSessionId = useAppStore((state) => state.activeAgentSessionId);
  const setAgentBaseUrl = useAppStore((state) => state.setAgentBaseUrl);
  const setAgentModel = useAppStore((state) => state.setAgentModel);
  const toggleAgentInspector = useAppStore((state) => state.toggleAgentInspector);
  const createAgentConversation = useAppStore((state) => state.createAgentConversation);
  const selectAgentConversation = useAppStore((state) => state.selectAgentConversation);
  const deleteAgentConversation = useAppStore((state) => state.deleteAgentConversation);
  const renameAgentConversation = useAppStore((state) => state.renameAgentConversation);
  const setAgentConversationProject = useAppStore((state) => state.setAgentConversationProject);
  const setAgentConversationMessages = useAppStore((state) => state.setAgentConversationMessages);
  const clearAgentConversation = useAppStore((state) => state.clearAgentConversation);
  const [projects, setProjects] = useState<StudioProject[]>([]);
  const [apiKey, setApiKey] = useState("");
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [renamingId, setRenamingId] = useState("");
  const [renameDraft, setRenameDraft] = useState("");
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
  }, [messages, busy]);

  const selectedProject = useMemo(
    () => projects.find((project) => project.id === agentProjectId),
    [agentProjectId, projects],
  );

  const send = async (preset?: string) => {
    const content = (preset ?? draft).trim();
    if (!content || busy || !activeSession) return;
    if (!agentBaseUrl.trim() || !agentModel.trim()) {
      setError("请先填写 OpenAI 兼容 URL 和模型名称");
      return;
    }
    const sessionId = activeSession.id;
    const userMessage: AgentConversationMessage = { id: messageId("user"), role: "user", content };
    const history = [...messages, userMessage];
    setAgentConversationMessages(sessionId, history);
    if (activeSession.title === "新对话") {
      renameAgentConversation(sessionId, content.replace(/\s+/g, " ").slice(0, 30));
    }
    setDraft("");
    setError("");
    setBusy(true);
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    try {
      const result = await desktopGateway.runAgentTurn({
        baseUrl: agentBaseUrl.trim(),
        model: agentModel.trim(),
        apiKey,
        projectId: agentProjectId,
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
      setBusy(false);
    }
  };

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
        <button className="button ghost small" type="button" onClick={() => activeSession && clearAgentConversation(activeSession.id)} disabled={messages.length === 0 || busy}><Trash2 size={13} /> 清空对话</button>
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
                  <button type="button" aria-label={`删除对话 ${session.title}`} onClick={() => deleteAgentConversation(session.id)} disabled={busy}><Trash2 size={11} /></button>
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
                <p>{selectedProject ? `Agent 已绑定“${selectedProject.name}”，可以按需读取和修改项目文件。` : "选择右侧开发项目后，Agent 将启用六个内置 RPAZ 工具。"}</p>
                <div className="agent-suggestions">
                  {suggestions.map((suggestion) => <button type="button" key={suggestion} onClick={() => void send(suggestion)}><MessageSquarePlus size={14} /><span>{suggestion}</span></button>)}
                </div>
              </div>
            )}
            {messages.map((message) => (
              <article className={`agent-message ${message.role}`} key={message.id}>
                <div className="agent-message-avatar">{message.role === "assistant" ? <Bot size={15} /> : "你"}</div>
                <div className="agent-message-body">
                  <header><strong>{message.role === "assistant" ? "DRPA Agent" : "你"}</strong>{message.role === "assistant" && <span>{message.durationMs} ms · {message.tokens ?? 0} tokens</span>}</header>
                  {message.tools && message.tools.length > 0 && <div className="agent-tool-events">{message.tools.map((tool) => <ToolEvent event={tool} key={tool.callId} />)}</div>}
                  <div className="agent-message-content">{message.content}</div>
                </div>
              </article>
            ))}
            {busy && (
              <article className="agent-message assistant pending">
                <div className="agent-message-avatar"><Bot size={15} /></div>
                <div className="agent-message-body"><header><strong>DRPA Agent</strong><span>模型与本地工具协同中</span></header><div className="agent-thinking"><LoaderCircle className="spin" size={14} /> 正在分析任务…</div></div>
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
              <h2><FolderCode size={13} /> 工作上下文</h2>
              <label><span>开发项目</span><div className="agent-select"><FileCode2 size={13} /><select aria-label="Agent 开发项目" value={agentProjectId} onChange={(event) => activeSession && setAgentConversationProject(activeSession.id, event.target.value)}><option value="">不绑定项目</option>{projects.map((project) => <option value={project.id} key={project.id}>{project.name}</option>)}</select><ChevronDown size={13} /></div></label>
              {selectedProject && <div className="agent-project-summary"><strong>{selectedProject.name}</strong><span>{selectedProject.files.length} 个文件</span><code>{selectedProject.id}</code></div>}
            </section>

            <section className="agent-config-section agent-tools-section">
              <h2><Wrench size={13} /> 内置工具 <span>{agentProjectId ? "6 ACTIVE" : "LOCKED"}</span></h2>
              <div className="agent-tool-list">{Object.entries(toolLabels).map(([name, label]) => <div className={agentProjectId ? "active" : ""} key={name}><CheckCircle2 size={12} /><span><strong>{label}</strong><code>{name}</code></span></div>)}</div>
            </section>
          </div>
          <footer><span className="agent-limit-dot" /> 单 Agent · 最多 8 轮工具调用 · Python 30 秒</footer>
        </aside>}
      </div>
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
