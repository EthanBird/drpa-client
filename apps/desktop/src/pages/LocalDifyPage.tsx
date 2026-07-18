import { open, save as saveDialog } from "@tauri-apps/plugin-dialog";
import {
  Activity,
  Bot,
  Braces,
  CheckCircle2,
  ChevronRight,
  CircleStop,
  CloudUpload,
  Copy,
  Download,
  GitBranch,
  KeyRound,
  LoaderCircle,
  MessageSquareText,
  Network,
  Play,
  Plus,
  RefreshCw,
  Save,
  Send,
  Server,
  Settings2,
  SlidersHorizontal,
  Sparkles,
  Trash2,
  Upload,
  Workflow,
  X,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import type {
  DifyCompatibilityReport,
  LocalDifyApp,
  LocalDifyAppMode,
  LocalDifyProvider,
  LocalDifyProviderInput,
  LocalDifyRunSummary,
  LocalDifyServiceStatus,
} from "../domain/models";
import { desktopGateway } from "../infra/gateway";
import { LocalDifyWorkflowDesigner } from "../components/LocalDifyWorkflowDesigner";

type StudioTab = "workflow" | "debug" | "config" | "runs" | "api";

interface PreviewMessage {
  id: string;
  role: "user" | "assistant";
  content: string;
}

const emptyProvider: LocalDifyProviderInput = {
  id: "",
  name: "",
  baseUrl: "https://api.openai.com/v1",
  model: "gpt-4o-mini",
  contextWindow: 128000,
  maxOutputTokens: 4096,
  temperature: 0.2,
  streaming: true,
  supportsTools: true,
  supportsJson: true,
  supportsVision: false,
  timeoutSeconds: 120,
  customHeaders: {},
  difyProvider: "langgenius/openai/openai",
  difyModel: "gpt-4o-mini",
  apiKey: "",
};

function providerInput(provider?: LocalDifyProvider): LocalDifyProviderInput {
  if (!provider) return { ...emptyProvider };
  return {
    id: provider.id,
    name: provider.name,
    baseUrl: provider.baseUrl,
    model: provider.model,
    contextWindow: provider.contextWindow,
    maxOutputTokens: provider.maxOutputTokens,
    temperature: provider.temperature,
    streaming: provider.streaming,
    supportsTools: provider.supportsTools,
    supportsJson: provider.supportsJson,
    supportsVision: provider.supportsVision,
    timeoutSeconds: provider.timeoutSeconds,
    customHeaders: provider.customHeaders,
    difyProvider: provider.difyProvider,
    difyModel: provider.difyModel,
    apiKey: "",
  };
}

function formatTime(value: number): string {
  return new Date(value > 10_000_000_000 ? value : value * 1000).toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

export function LocalDifyPage() {
  const [apps, setApps] = useState<LocalDifyApp[]>([]);
  const [providers, setProviders] = useState<LocalDifyProvider[]>([]);
  const [selectedId, setSelectedId] = useState("");
  const [draft, setDraft] = useState<LocalDifyApp | null>(null);
  const [tab, setTab] = useState<StudioTab>("debug");
  const [query, setQuery] = useState("");
  const [messages, setMessages] = useState<PreviewMessage[]>([]);
  const [runs, setRuns] = useState<LocalDifyRunSummary[]>([]);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [createOpen, setCreateOpen] = useState(false);
  const [createName, setCreateName] = useState("新 AI 应用");
  const [createMode, setCreateMode] = useState<LocalDifyAppMode>("chat");
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [providersOpen, setProvidersOpen] = useState(false);
  const [providerDraft, setProviderDraft] = useState<LocalDifyProviderInput>({ ...emptyProvider });
  const [providerNotice, setProviderNotice] = useState("");
  const [compatibility, setCompatibility] = useState<DifyCompatibilityReport | null>(null);
  const [service, setService] = useState<LocalDifyServiceStatus | null>(null);
  const [servicePort, setServicePort] = useState(34130);
  const [apiToken, setApiToken] = useState("");

  const selected = useMemo(() => apps.find((app) => app.id === selectedId) ?? null, [apps, selectedId]);
  const selectedProvider = useMemo(
    () => providers.find((provider) => provider.id === draft?.providerId),
    [draft?.providerId, providers],
  );

  const reload = async (preferredId?: string) => {
    const [nextApps, nextProviders, nextService] = await Promise.all([
      desktopGateway.listLocalDifyApps(),
      desktopGateway.listLocalDifyProviders(),
      desktopGateway.getLocalDifyServiceStatus(),
    ]);
    setApps(nextApps);
    setProviders(nextProviders);
    setService(nextService);
    setServicePort(nextService.port);
    setSelectedId((current) => {
      const requested = preferredId ?? current;
      return nextApps.some((app) => app.id === requested) ? requested : (nextApps[0]?.id ?? "");
    });
  };

  useEffect(() => {
    void reload().catch((error: unknown) => setNotice(String(error)));
  }, []);

  useEffect(() => {
    setDraft(selected ? structuredClone(selected) : null);
    setMessages(selected?.openingStatement
      ? [{ id: `opening-${selected.id}`, role: "assistant", content: selected.openingStatement }]
      : []);
    setApiToken("");
    setCompatibility(null);
    setTab((current) => selected && (selected.mode === "workflow" || selected.mode === "advanced-chat")
      ? "workflow"
      : current === "workflow" ? "debug" : current);
    if (selected) {
      void desktopGateway.listLocalDifyRuns(selected.id, 100).then(setRuns).catch((error: unknown) => setNotice(String(error)));
    } else {
      setRuns([]);
    }
  }, [selected]);

  const createApp = async () => {
    setBusy(true);
    try {
      const app = await desktopGateway.createLocalDifyApp(createName, createMode);
      await reload(app.id);
      setCreateOpen(false);
      setNotice("AI 应用已创建");
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy(false);
    }
  };

  const persistApp = async (app: LocalDifyApp): Promise<LocalDifyApp> => {
    setBusy(true);
    try {
      const saved = await desktopGateway.saveLocalDifyApp(app);
      await reload(saved.id);
      setNotice("应用配置已保存");
      return saved;
    } catch (error) {
      setNotice(String(error));
      throw error;
    } finally {
      setBusy(false);
    }
  };

  const saveApp = async () => {
    if (!draft) return;
    await persistApp(draft);
  };

  const removeApp = async () => {
    if (!draft) return;
    setBusy(true);
    try {
      await desktopGateway.deleteLocalDifyApp(draft.id);
      setDeleteOpen(false);
      await reload();
      setNotice("AI 应用已删除");
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy(false);
    }
  };

  const runApp = async () => {
    if (!draft || !query.trim() || busy) return;
    const text = query.trim();
    const requestId = `request-${Date.now()}-${Math.random().toString(16).slice(2)}`;
    const userMessage: PreviewMessage = { id: `user-${requestId}`, role: "user", content: text };
    const assistantId = `assistant-${requestId}`;
    setMessages((current) => [...current, userMessage, { id: assistantId, role: "assistant", content: "" }]);
    setQuery("");
    setBusy(true);
    setNotice("正在调用 Provider…");
    let streamed = "";
    let stop: (() => void) | undefined;
    try {
      stop = await desktopGateway.listenLocalDifyStream(requestId, (event) => {
        if (event.type === "delta") {
          streamed += event.content;
          setMessages((current) => current.map((message) => message.id === assistantId ? { ...message, content: streamed } : message));
        }
      });
      const result = await desktopGateway.runLocalDifyApp({
        requestId,
        appId: draft.id,
        query: text,
        inputs: { [draft.inputKey]: text },
        user: "local-developer",
        stream: selectedProvider?.streaming ?? true,
        conversationId: "",
      });
      setMessages((current) => current.map((message) => message.id === assistantId ? { ...message, content: result.answer } : message));
      setNotice(`运行完成 · ${result.durationMs}ms · ${result.usage.totalTokens} tokens`);
      setRuns(await desktopGateway.listLocalDifyRuns(draft.id, 100));
    } catch (error) {
      setMessages((current) => current.map((message) => message.id === assistantId ? { ...message, content: `**运行失败**\n\n${String(error)}` } : message));
      setNotice(String(error));
    } finally {
      stop?.();
      setBusy(false);
    }
  };

  const saveProvider = async () => {
    setBusy(true);
    setProviderNotice("");
    try {
      const provider = await desktopGateway.saveLocalDifyProvider(providerDraft);
      setProviders(await desktopGateway.listLocalDifyProviders());
      setProviderDraft(providerInput(provider));
      setProviderNotice("Provider 已保存");
    } catch (error) {
      setProviderNotice(String(error));
    } finally {
      setBusy(false);
    }
  };

  const testProvider = async () => {
    if (!providerDraft.id) {
      setProviderNotice("请先保存 Provider");
      return;
    }
    setBusy(true);
    try {
      const result = await desktopGateway.testLocalDifyProvider(providerDraft.id);
      setProviderNotice(`${result.message} · ${result.durationMs}ms`);
    } catch (error) {
      setProviderNotice(String(error));
    } finally {
      setBusy(false);
    }
  };

  const removeProvider = async () => {
    if (!providerDraft.id) return;
    setBusy(true);
    try {
      await desktopGateway.deleteLocalDifyProvider(providerDraft.id);
      const next = await desktopGateway.listLocalDifyProviders();
      setProviders(next);
      setProviderDraft(providerInput(next[0]));
      setProviderNotice("Provider 已删除");
    } catch (error) {
      setProviderNotice(String(error));
    } finally {
      setBusy(false);
    }
  };

  const importDsl = async () => {
    try {
      const source = "__TAURI_INTERNALS__" in window
        ? await open({ multiple: false, filters: [{ name: "Dify DSL", extensions: ["yml", "yaml"] }] })
        : "browser-preview.yml";
      if (typeof source !== "string") return;
      const app = await desktopGateway.importLocalDifyDsl(source);
      await reload(app.id);
      setNotice("Dify DSL 已导入，原始 YAML 已保留");
    } catch (error) {
      setNotice(String(error));
    }
  };

  const exportDsl = async () => {
    if (!draft) return;
    setBusy(true);
    try {
      const savedDraft = await desktopGateway.saveLocalDifyApp(draft);
      setDraft(savedDraft);
      const report = await desktopGateway.checkLocalDifyCompatibility(savedDraft.id);
      setCompatibility(report);
      if (!report.compatible) {
        setNotice("兼容性检查存在阻止导出的错误");
        setTab("api");
        return;
      }
      const target = "__TAURI_INTERNALS__" in window
        ? await saveDialog({ defaultPath: `${draft.name}.yml`, filters: [{ name: "Dify DSL", extensions: ["yml", "yaml"] }] })
        : `${draft.name}.yml`;
      if (!target) return;
      const exported = await desktopGateway.exportLocalDifyDsl(savedDraft.id, target);
      setNotice(`Dify DSL 已导出：${exported}`);
      await reload(savedDraft.id);
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy(false);
    }
  };

  const publish = async () => {
    if (!draft) return;
    setBusy(true);
    try {
      const savedDraft = await desktopGateway.saveLocalDifyApp(draft);
      setDraft(savedDraft);
      const report = await desktopGateway.checkLocalDifyCompatibility(savedDraft.id);
      setCompatibility(report);
      if (!report.compatible) {
        setNotice("发布检查未通过");
        return;
      }
      const published = await desktopGateway.publishLocalDifyApp(savedDraft.id);
      await reload(published.id);
      setApiToken(await desktopGateway.getLocalDifyAppApiToken(published.id));
      setNotice(`本地版本 v${published.publishedVersion} 已发布`);
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy(false);
    }
  };

  const toggleService = async () => {
    setBusy(true);
    try {
      const next = service?.running
        ? await desktopGateway.stopLocalDifyService()
        : await desktopGateway.startLocalDifyService(servicePort);
      setService(next);
      setNotice(next.running ? `Local Dify Service 已启动：${next.endpoint}` : "Local Dify Service 已停止");
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="page local-dify-page">
      <header className="page-header local-dify-header">
        <div><div className="eyebrow">LOCAL AI APPLICATION PLATFORM</div><h1>AI 应用</h1><p>兼容 Dify DSL 与 Service API 的本地开发、调试和发布工作台。</p></div>
        <div className="local-dify-header-actions">
          <button className="button secondary" type="button" onClick={() => void importDsl()}><Upload size={14} /> 导入 DSL</button>
          <button className="button secondary" type="button" onClick={() => { setProviderDraft(providerInput(providers[0])); setProvidersOpen(true); }}><Network size={14} /> Providers</button>
          <button className="button primary" type="button" onClick={() => setCreateOpen(true)}><Plus size={14} /> 新建应用</button>
        </div>
      </header>

      <div className={`local-dify-layout ${tab === "workflow" ? "workflow-focus" : ""}`}>
        <aside className="local-dify-catalog">
          <header><Sparkles size={14} /><strong>本地应用</strong><span>{apps.length}</span></header>
          <div className="local-dify-app-list">
            {apps.map((app) => (
              <button key={app.id} className={app.id === selectedId ? "active" : ""} type="button" onClick={() => setSelectedId(app.id)}>
                <span className={`dify-app-icon ${app.mode}`}><Bot size={15} /></span>
                <span><strong>{app.name}</strong><small>{app.mode} · {app.providerId ? "Provider 已连接" : "待配置 Provider"}</small></span>
                <ChevronRight size={13} />
              </button>
            ))}
            {apps.length === 0 && <div className="local-dify-empty"><Bot size={24} /><strong>创建第一个 AI 应用</strong><span>从 Chat 或 Completion 开始。</span></div>}
          </div>
          <footer><span className={service?.running ? "service-dot running" : "service-dot"} /><div><strong>Local Dify Service</strong><small>{service?.running ? service.endpoint : "服务未启动"}</small></div></footer>
        </aside>

        {draft ? (
          <main className="local-dify-workspace">
            <header className="local-dify-app-header">
              <div className={`dify-app-logo ${draft.mode}`}><Bot size={20} /></div>
              <div><div><h2>{draft.name}</h2><span className="status-badge neutral">{draft.mode}</span>{draft.apiEnabled && <span className="status-badge success">v{draft.publishedVersion} 已发布</span>}</div><p>{draft.description || "暂无应用描述"}</p></div>
              <div className="local-dify-app-actions">
                <button className="button ghost small" type="button" aria-label="删除 AI 应用" onClick={() => setDeleteOpen(true)}><Trash2 size={13} /></button>
                <button className="button secondary small" type="button" onClick={() => void exportDsl()} disabled={busy}><Download size={13} /> 导出 DSL</button>
                <button className="button primary small" type="button" onClick={() => void publish()} disabled={busy}><CloudUpload size={13} /> 发布</button>
              </div>
            </header>
            <nav className="local-dify-tabs" aria-label="AI 应用工作区">
              {(draft.mode === "workflow" || draft.mode === "advanced-chat") && <button className={tab === "workflow" ? "active" : ""} type="button" onClick={() => setTab("workflow")}><Workflow size={13} /> 工作流</button>}
              <button className={tab === "debug" ? "active" : ""} type="button" onClick={() => setTab("debug")}><MessageSquareText size={13} /> 调试预览</button>
              <button className={tab === "config" ? "active" : ""} type="button" onClick={() => setTab("config")}><SlidersHorizontal size={13} /> 应用配置</button>
              <button className={tab === "runs" ? "active" : ""} type="button" onClick={() => setTab("runs")}><Activity size={13} /> 运行记录 <span>{runs.length}</span></button>
              <button className={tab === "api" ? "active" : ""} type="button" onClick={() => setTab("api")}><Server size={13} /> API 与导出</button>
              <em>{notice}</em>
            </nav>

            {tab === "workflow" && (draft.mode === "workflow" || draft.mode === "advanced-chat") && <LocalDifyWorkflowDesigner
              app={draft}
              providers={providers}
              busy={busy}
              onChange={setDraft}
              onSave={persistApp}
              onRunCompleted={async () => setRuns(await desktopGateway.listLocalDifyRuns(draft.id, 100))}
              onNotice={setNotice}
            />}

            {tab === "debug" && <section className="local-dify-debug">
              <div className="dify-chat-stage">
                <div className="dify-chat-toolbar"><span><span className="service-dot running" /> Preview</span><code>{selectedProvider?.model ?? "未选择模型"}</code><button type="button" title="清空对话" onClick={() => setMessages(draft.openingStatement ? [{ id: `opening-${Date.now()}`, role: "assistant", content: draft.openingStatement }] : [])}><RefreshCw size={12} /></button></div>
                <div className="dify-chat-messages">
                  {messages.map((message) => <article key={message.id} className={message.role}>
                    <div>{message.role === "assistant" ? <Bot size={14} /> : "你"}</div>
                    <section>{message.content ? <ReactMarkdown remarkPlugins={[remarkGfm]}>{message.content}</ReactMarkdown> : <span className="typing-indicator"><i /><i /><i /></span>}</section>
                  </article>)}
                  {messages.length === 0 && <div className="dify-chat-welcome"><Sparkles size={26} /><h3>测试 {draft.name}</h3><p>输入消息以验证 Prompt、Provider、流式输出和运行记录。</p></div>}
                </div>
                <div className="dify-chat-composer"><textarea aria-label="Local Dify 调试输入" value={query} onChange={(event) => setQuery(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) void runApp(); }} placeholder="输入测试消息，Ctrl+Enter 运行…" /><footer><span>{draft.inputKey} · {selectedProvider?.streaming ? "Streaming" : "Blocking"}</span><button className="button primary" type="button" aria-label="运行 Local Dify 应用" onClick={() => void runApp()} disabled={busy || !query.trim()}>{busy ? <LoaderCircle className="spin" size={14} /> : <Send size={14} />} 运行</button></footer></div>
              </div>
              <aside className="dify-debug-inspector">
                <header><Settings2 size={14} /><div><strong>调试参数</strong><small>保存后立即用于下一次运行</small></div></header>
                <label><span>Provider</span><select value={draft.providerId} onChange={(event) => setDraft({ ...draft, providerId: event.target.value })}><option value="">请选择 Provider</option>{providers.map((provider) => <option key={provider.id} value={provider.id}>{provider.name}</option>)}</select></label>
                <label><span>Temperature</span><input type="number" min={0} max={2} step={0.1} value={draft.temperature} onChange={(event) => setDraft({ ...draft, temperature: event.currentTarget.valueAsNumber })} /></label>
                <label><span>最大输出 tokens</span><input type="number" min={64} max={131072} step={64} value={draft.maxOutputTokens} onChange={(event) => setDraft({ ...draft, maxOutputTokens: event.currentTarget.valueAsNumber })} /></label>
                <div className="dify-provider-summary"><Network size={14} /><span><strong>{selectedProvider?.name ?? "未配置 Provider"}</strong><small>{selectedProvider?.baseUrl ?? "打开 Providers 创建连接"}</small><code>{selectedProvider?.model ?? ""}</code></span></div>
                <button className="button secondary" type="button" onClick={() => void saveApp()} disabled={busy}><Save size={13} /> 保存调试配置</button>
              </aside>
            </section>}

            {tab === "config" && <section className="local-dify-config">
              <div className="dify-config-section"><header><Braces size={15} /><div><h3>基本信息</h3><p>这些字段会进入本地应用配置和导出的 Dify DSL。</p></div></header><div className="dify-form-grid">
                <label><span>应用名称</span><input aria-label="AI 应用名称" value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.target.value })} /></label>
                <label><span>应用模式</span><select aria-label="AI 应用模式" value={draft.mode} onChange={(event) => setDraft({ ...draft, mode: event.target.value as LocalDifyAppMode })}><option value="chat">Chat</option><option value="completion">Completion</option><option value="advanced-chat">Chatflow</option><option value="workflow">Workflow</option></select></label>
                <label className="wide"><span>应用描述</span><textarea value={draft.description} onChange={(event) => setDraft({ ...draft, description: event.target.value })} /></label>
                <label><span>输入变量</span><input value={draft.inputKey} onChange={(event) => setDraft({ ...draft, inputKey: event.target.value })} /></label>
                <label><span>Provider</span><select value={draft.providerId} onChange={(event) => setDraft({ ...draft, providerId: event.target.value })}><option value="">请选择 Provider</option>{providers.map((provider) => <option key={provider.id} value={provider.id}>{provider.name}</option>)}</select></label>
              </div></div>
              <div className="dify-config-section"><header><Sparkles size={15} /><div><h3>Prompt</h3><p>Chat 和 Completion 共用系统指令；导出时写入 pre_prompt。</p></div></header><div className="dify-form-grid">
                <label className="wide"><span>系统指令</span><textarea className="prompt" aria-label="Local Dify 系统指令" value={draft.systemPrompt} onChange={(event) => setDraft({ ...draft, systemPrompt: event.target.value })} /></label>
                <label className="wide"><span>开场白</span><textarea value={draft.openingStatement} onChange={(event) => setDraft({ ...draft, openingStatement: event.target.value })} /></label>
              </div></div>
              <footer><button className="button primary" type="button" onClick={() => void saveApp()} disabled={busy}><Save size={14} /> 保存应用配置</button></footer>
            </section>}

            {tab === "runs" && <section className="local-dify-runs">
              <header><div><h3>本地运行记录</h3><p>记录 Provider、模型、耗时、Token 和完整输入输出。</p></div><button className="button secondary small" type="button" onClick={() => void desktopGateway.listLocalDifyRuns(draft.id, 100).then(setRuns)}><RefreshCw size={12} /> 刷新</button></header>
              <div className="dify-runs-table"><div className="head"><span>状态</span><span>输入 / 输出</span><span>Provider</span><span>Token</span><span>耗时</span><span>时间</span></div>{runs.map((run) => <details key={run.id}><summary><span className={`status-badge ${run.status === "success" ? "success" : "failed"}`}>{run.status}</span><span><strong>{run.query}</strong><small>{run.answer || run.error}</small></span><code>{run.model}</code><code>{run.promptTokens + run.completionTokens}</code><code>{run.durationMs}ms</code><time>{formatTime(run.createdAt)}</time></summary><div><section><strong>输入</strong><pre>{run.query}</pre></section><section><strong>{run.error ? "错误" : "输出"}</strong><pre>{run.error || run.answer}</pre></section><small>run={run.id} · provider={run.providerId}</small></div></details>)}{runs.length === 0 && <div className="local-dify-empty"><Activity size={24} /><strong>还没有运行记录</strong><span>在调试预览中运行一次应用。</span></div>}</div>
            </section>}

            {tab === "api" && <section className="local-dify-api">
              <div className="dify-api-grid">
                <article><header><Server size={16} /><div><h3>Local Dify Service</h3><p>向本机程序暴露 Dify 兼容 API。</p></div><span className={`status-badge ${service?.running ? "success" : "neutral"}`}>{service?.running ? "运行中" : "已停止"}</span></header><label><span>监听端口</span><input type="number" min={1024} max={65535} value={servicePort} disabled={service?.running} onChange={(event) => setServicePort(event.currentTarget.valueAsNumber)} /></label><div className="dify-endpoint"><code>{service?.endpoint ?? `http://127.0.0.1:${servicePort}/v1`}</code><button type="button" aria-label="复制 Local Dify Endpoint" onClick={() => void navigator.clipboard?.writeText(service?.endpoint ?? "")}><Copy size={12} /></button></div><button className={`button ${service?.running ? "secondary" : "primary"}`} type="button" onClick={() => void toggleService()} disabled={busy}>{service?.running ? <CircleStop size={13} /> : <Play size={13} />}{service?.running ? "停止服务" : "启动服务"}</button></article>
                <article><header><KeyRound size={16} /><div><h3>应用 API Token</h3><p>发布后通过 Bearer Token 访问当前应用。</p></div></header><div className="dify-endpoint"><code>{apiToken || (draft.apiEnabled ? "点击读取当前 Token" : "发布应用后生成")}</code>{apiToken && <button type="button" aria-label="复制应用 API Token" onClick={() => void navigator.clipboard?.writeText(apiToken)}><Copy size={12} /></button>}</div><button className="button secondary" type="button" disabled={!draft.apiEnabled} onClick={() => void desktopGateway.getLocalDifyAppApiToken(draft.id).then(setApiToken).catch((error: unknown) => setNotice(String(error)))}><KeyRound size={13} /> 读取 Token</button><small>POST /chat-messages · /completion-messages · /workflows/run</small></article>
                <article className="compatibility-card"><header><CheckCircle2 size={16} /><div><h3>Dify 云端兼容性</h3><p>导出前检查模式、Provider 映射和本机依赖。</p></div></header><button className="button secondary" type="button" onClick={() => void desktopGateway.checkLocalDifyCompatibility(draft.id).then(setCompatibility)}><Activity size={13} /> 运行兼容性检查</button>{compatibility && <div className="dify-compatibility"><strong>{compatibility.compatible ? `可导出 · DSL ${compatibility.targetVersion}` : "存在阻止导出的错误"}</strong>{compatibility.issues.map((issue) => <p key={issue.code} className={issue.level}><span>{issue.level}</span>{issue.message}</p>)}{compatibility.issues.length === 0 && <p className="info"><span>ready</span>当前应用可导出为 Dify DSL。</p>}</div>}</article>
              </div>
            </section>}
          </main>
        ) : <main className="local-dify-no-selection"><Bot size={42} /><h2>Local Dify Studio</h2><p>创建应用，配置 OpenAI 兼容 Provider，然后在本机调试并导出 Dify DSL。</p><button className="button primary" type="button" onClick={() => setCreateOpen(true)}><Plus size={14} /> 新建 AI 应用</button></main>}
      </div>

      {createOpen && <div className="modal-backdrop" role="presentation"><section className="confirm-dialog dify-create-dialog" role="dialog" aria-modal="true" aria-label="新建 AI 应用"><header><div><Sparkles size={18} /><h2>新建 AI 应用</h2></div><button type="button" aria-label="关闭" onClick={() => setCreateOpen(false)}><X size={16} /></button></header><p>选择应用类型；Workflow 与 Chatflow 提供完整可视化编排画布。</p><label><span>应用名称</span><input aria-label="新建 AI 应用名称" value={createName} autoFocus onChange={(event) => setCreateName(event.target.value)} /></label><div className="dify-mode-picker"><button className={createMode === "chat" ? "active" : ""} type="button" onClick={() => setCreateMode("chat")}><MessageSquareText size={18} /><strong>Chat</strong><span>多轮对话和开场白</span></button><button className={createMode === "completion" ? "active" : ""} type="button" onClick={() => setCreateMode("completion")}><Braces size={18} /><strong>Completion</strong><span>单次文本生成</span></button><button className={createMode === "workflow" ? "active" : ""} type="button" onClick={() => setCreateMode("workflow")}><Workflow size={18} /><strong>Workflow</strong><span>自动化与批处理工作流</span></button><button className={createMode === "advanced-chat" ? "active" : ""} type="button" onClick={() => setCreateMode("advanced-chat")}><GitBranch size={18} /><strong>Chatflow</strong><span>带流程编排的对话应用</span></button></div><footer><button className="button ghost" type="button" onClick={() => setCreateOpen(false)}>取消</button><button className="button primary" type="button" onClick={() => void createApp()} disabled={busy || !createName.trim()}><Plus size={13} /> 创建应用</button></footer></section></div>}

      {deleteOpen && draft && <div className="modal-backdrop" role="presentation"><section className="confirm-dialog" role="dialog" aria-modal="true" aria-label="确认删除 AI 应用"><header><div><Trash2 size={18} /><h2>删除 AI 应用</h2></div></header><p>将删除“{draft.name}”的配置和本地 API Token；历史运行记录继续保留用于诊断。</p><footer><button className="button ghost" type="button" onClick={() => setDeleteOpen(false)}>取消</button><button className="button danger" type="button" onClick={() => void removeApp()} disabled={busy}>确认删除</button></footer></section></div>}

      {providersOpen && <div className="modal-backdrop dify-provider-backdrop" role="presentation"><section className="dify-provider-dialog" role="dialog" aria-modal="true" aria-label="Local Dify Providers"><aside><header><Network size={15} /><strong>Providers</strong><button type="button" aria-label="新建 Provider" onClick={() => setProviderDraft({ ...emptyProvider })}><Plus size={14} /></button></header>{providers.map((provider) => <button key={provider.id} className={providerDraft.id === provider.id ? "active" : ""} type="button" onClick={() => setProviderDraft(providerInput(provider))}><span className="service-dot running" /><span><strong>{provider.name}</strong><small>{provider.model}</small></span><ChevronRight size={12} /></button>)}</aside><main><header><div><h2>{providerDraft.id ? "编辑 Provider" : "新建 Provider"}</h2><p>OpenAI Chat Completions 兼容连接与 Dify 云端映射。</p></div><button type="button" aria-label="关闭 Providers" onClick={() => setProvidersOpen(false)}><X size={16} /></button></header><div className="dify-provider-form">
        <label><span>名称</span><input aria-label="Provider 名称" value={providerDraft.name} onChange={(event) => setProviderDraft({ ...providerDraft, name: event.target.value })} /></label><label><span>Model</span><input aria-label="Provider 模型" value={providerDraft.model} onChange={(event) => setProviderDraft({ ...providerDraft, model: event.target.value })} /></label><label className="wide"><span>OpenAI 兼容 URL</span><input aria-label="Provider URL" value={providerDraft.baseUrl} onChange={(event) => setProviderDraft({ ...providerDraft, baseUrl: event.target.value })} /></label><label className="wide"><span>API Key {providerDraft.id && providers.find((item) => item.id === providerDraft.id)?.hasApiKey ? "（已保存，留空则保持）" : "（可选）"}</span><input aria-label="Provider API Key" type="password" autoComplete="off" value={providerDraft.apiKey} onChange={(event) => setProviderDraft({ ...providerDraft, apiKey: event.target.value })} /></label><label><span>上下文窗口</span><input type="number" value={providerDraft.contextWindow} onChange={(event) => setProviderDraft({ ...providerDraft, contextWindow: event.currentTarget.valueAsNumber })} /></label><label><span>最大输出</span><input type="number" value={providerDraft.maxOutputTokens} onChange={(event) => setProviderDraft({ ...providerDraft, maxOutputTokens: event.currentTarget.valueAsNumber })} /></label><label><span>超时（秒）</span><input type="number" value={providerDraft.timeoutSeconds} onChange={(event) => setProviderDraft({ ...providerDraft, timeoutSeconds: event.currentTarget.valueAsNumber })} /></label><label className="switch-field"><span>流式输出</span><button className={`switch ${providerDraft.streaming ? "on" : ""}`} type="button" role="switch" aria-checked={providerDraft.streaming} onClick={() => setProviderDraft({ ...providerDraft, streaming: !providerDraft.streaming })}><span /></button></label><label><span>Dify 云端 Provider</span><input value={providerDraft.difyProvider} onChange={(event) => setProviderDraft({ ...providerDraft, difyProvider: event.target.value })} /></label><label><span>Dify 云端 Model</span><input value={providerDraft.difyModel} onChange={(event) => setProviderDraft({ ...providerDraft, difyModel: event.target.value })} /></label>
        </div><footer><span>{providerNotice}</span>{providerDraft.id && <button className="button danger-ghost" type="button" aria-label="删除 Provider" onClick={() => void removeProvider()}><Trash2 size={13} /></button>}<button className="button secondary" type="button" onClick={() => void testProvider()} disabled={!providerDraft.id || busy}><Play size={13} /> 测试</button><button className="button primary" type="button" onClick={() => void saveProvider()} disabled={busy}><Save size={13} /> 保存 Provider</button></footer></main></section></div>}
    </div>
  );
}
