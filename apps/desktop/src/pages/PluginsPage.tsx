import { open } from "@tauri-apps/plugin-dialog";
import {
  Activity,
  Bot,
  Box,
  Braces,
  CheckCircle2,
  ChevronRight,
  CircleAlert,
  CircleStop,
  Copy,
  Download,
  FileCode2,
  FlaskConical,
  Globe2,
  Hammer,
  LayoutPanelTop,
  ListTree,
  LoaderCircle,
  Play,
  PlugZap,
  RefreshCw,
  Save,
  Send,
  Server,
  Settings2,
  TerminalSquare,
  Trash2,
  Wrench,
} from "lucide-react";
import { useEffect, useMemo, useState, type FormEvent, type ReactNode } from "react";

import { useAppStore } from "../app/store";
import { SidebarToggle, useSidebarCollapsed } from "../components/SidebarToggle";
import { desktopGateway } from "../infra/gateway";
import "../styles/plugins.css";

type PluginStatus = "disabled" | "stopped" | "running" | "error";
type CapabilityKind = "service" | "tools" | "debugger" | "panel" | "provider";
type ProjectKind = "tool" | "service" | "bundle";

interface ConfigFieldDescriptor {
  type?: "string" | "integer" | "number" | "boolean" | "object" | "array";
  title?: string;
  description?: string;
  enum?: Array<string | number>;
  secret?: boolean;
  minimum?: number;
  maximum?: number;
  format?: string;
  default?: unknown;
  multiline?: boolean;
}

interface PluginPanelField {
  id: string;
  label: string;
  description?: string;
  type: "string" | "number" | "boolean" | "textarea" | "select";
  options?: Array<{ label: string; value: string }>;
  default?: unknown;
  secret?: boolean;
}

interface PluginPanelAction {
  id: string;
  label: string;
  style?: "primary" | "secondary" | "danger";
}

interface PluginPanelDescriptor {
  renderer: "iframe" | "form" | "markdown";
  entry?: string;
  content?: string;
  fields?: PluginPanelField[];
  actions?: PluginPanelAction[];
}

interface PluginCapability {
  id: string;
  kind: CapabilityKind;
  title: string;
  description: string;
  endpoint?: string;
  protocol?: string;
  entry?: string;
  apiKeyConfigKey?: string;
  modelConfigKey?: string;
  providerId?: string;
  endpoints?: PluginDebuggerEndpoint[];
  panel?: PluginPanelDescriptor;
}

interface PluginDebuggerEndpoint {
  id: string;
  title: string;
  kind: string;
  method: "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
  endpoint: string;
  bearerConfigKey?: string;
  requestDefaults?: Record<string, unknown>;
  timeoutSeconds?: number;
  bearer_config_key?: string;
  request_defaults?: Record<string, unknown>;
  timeout_seconds?: number;
}

interface PluginDebuggerResponse {
  endpointId: string;
  status: number;
  durationMs: number;
  contentType: string;
  body: unknown;
  truncated: boolean;
}

interface PluginManifestView {
  schemaVersion: number;
  id: string;
  name: string;
  version: string;
  description: string;
  author?: string;
  homepage?: string;
  capabilities: PluginCapability[];
}

interface PluginRecord {
  id: string;
  name: string;
  version: string;
  description: string;
  types: string[];
  enabled: boolean;
  autostart: boolean;
  status: PluginStatus;
  endpoint: string;
  toolCount: number;
  serviceCount?: number;
  toolProviderCount?: number;
  services?: PluginServiceSummary[];
  providers?: PluginProviderSummary[];
  config: Record<string, unknown>;
  configuredSecrets?: Record<string, boolean>;
  configSchema: {
    type?: string;
    properties?: Record<string, ConfigFieldDescriptor>;
    required?: string[];
  };
  directory: string;
  lastError: string;
  manifest?: PluginManifestView;
  debugger?: {
    endpoints: PluginDebuggerEndpoint[];
    panels?: PluginDebuggerPanel[];
  };
}

interface PluginServiceSummary {
  id: string;
  title: string;
  primary: boolean;
  transport: string;
  status: string;
  endpoint: string;
  healthcheck: string;
}

interface PluginProviderSummary {
  id: string;
  title: string;
  protocol: string;
  serviceId: string;
  endpoint: string;
  modelConfigKey: string;
  apiKeyConfigKey: string;
}

interface PluginDebuggerPanel {
  id: string;
  title: string;
  kind: string;
  endpoint?: string;
  config?: Record<string, unknown>;
}

interface PluginLogLine {
  timestamp: number;
  stream: "stdout" | "stderr";
  message: string;
  serviceId?: string;
  event?: {
    kind?: string;
    req_id?: string;
    requestId?: string;
    title?: string;
    detail?: unknown;
    event?: unknown;
    serviceId?: string;
  };
}

interface PluginProject {
  id: string;
  name: string;
  version: string;
  description: string;
  types: string[];
  directory: string;
  valid: boolean;
  validationMessage: string;
}

interface PluginToolDescriptor {
  name: string;
  title: string;
  description: string;
  inputSchema: Record<string, unknown>;
}

interface PluginHealth {
  ok: boolean;
  status: "healthy" | "degraded" | "unhealthy" | "unknown";
  message: string;
  checkedAt: number;
  durationMs: number;
  details?: Record<string, unknown>;
}

interface PluginToolResult {
  ok: boolean;
  output: unknown;
  durationMs: number;
  error?: string;
}

interface PluginPanelResult {
  ok: boolean;
  message: string;
  data?: unknown;
}

interface PluginTrafficEvent {
  timestamp: number;
  kind: string;
  requestId: string;
  title: string;
  detail: unknown;
}

interface PluginWorkbenchGateway {
  listPlugins(): Promise<PluginRecord[]>;
  installPlugin(path: string): Promise<PluginRecord>;
  savePluginConfig(pluginId: string, config: Record<string, unknown>, autostart: boolean): Promise<void>;
  setPluginEnabled(pluginId: string, enabled: boolean): Promise<void>;
  startPlugin(pluginId: string): Promise<void>;
  stopPlugin(pluginId: string): Promise<void>;
  uninstallPlugin(pluginId: string): Promise<void>;
  getPluginLogs(pluginId: string): Promise<PluginLogLine[]>;
  testPluginConnection(pluginId: string): Promise<{ ok: boolean; message: string; duration_ms: number; details?: Record<string, unknown> }>;
  listPluginProjects(): Promise<PluginProject[]>;
  createPluginProject(pluginId: string, name: string, projectType: ProjectKind): Promise<PluginProject>;
  validatePluginProject(pluginId: string): Promise<PluginProject>;
  buildPluginProject(pluginId: string): Promise<string>;
  getPluginManifest?(pluginId: string): Promise<PluginManifestView>;
  listPluginTools?(pluginId: string): Promise<PluginToolDescriptor[]>;
  checkPluginHealth?(pluginId: string): Promise<PluginHealth>;
  invokePluginTool?(pluginId: string, toolName: string, input: Record<string, unknown>): Promise<PluginToolResult>;
  runPluginDebugger?(pluginId: string, endpointId: string, request?: unknown): Promise<PluginDebuggerResponse>;
  invokePluginPanelAction?(pluginId: string, panelId: string, actionId: string, values: Record<string, unknown>): Promise<PluginPanelResult>;
}

const pluginGateway = desktopGateway as unknown as PluginWorkbenchGateway;

const statusLabels: Record<PluginStatus, string> = {
  disabled: "未启用",
  stopped: "已停止",
  running: "运行中",
  error: "运行异常",
};

const capabilityLabels: Record<CapabilityKind, string> = {
  service: "服务",
  tools: "工具",
  debugger: "调试器",
  panel: "面板",
  provider: "Provider",
};

type WorkbenchTab =
  | { id: "overview" | "config" | "service" | "tools" | "debugger" | "events" | "logs"; title: string; kind: Exclude<CapabilityKind, "panel"> | "base" }
  | { id: `panel:${string}`; title: string; kind: "panel"; capability: PluginCapability };

export function PluginsPage() {
  const catalogCollapsed = useSidebarCollapsed("plugins-catalog");
  const setActiveNavigation = useAppStore((state) => state.setActiveNavigation);
  const setAgentBaseUrl = useAppStore((state) => state.setAgentBaseUrl);
  const setAgentModel = useAppStore((state) => state.setAgentModel);
  const setAgentApiKey = useAppStore((state) => state.setAgentApiKey);
  const setAgentProviderRef = useAppStore((state) => state.setAgentProviderRef);
  const [plugins, setPlugins] = useState<PluginRecord[]>([]);
  const [projects, setProjects] = useState<PluginProject[]>([]);
  const [selectedId, setSelectedId] = useState("");
  const [manifest, setManifest] = useState<PluginManifestView | null>(null);
  const [activeTab, setActiveTab] = useState<string>("overview");
  const [configDraft, setConfigDraft] = useState<Record<string, unknown>>({});
  const [autostart, setAutostart] = useState(false);
  const [tools, setTools] = useState<PluginToolDescriptor[]>([]);
  const [logs, setLogs] = useState<PluginLogLine[]>([]);
  const [health, setHealth] = useState<PluginHealth | null>(null);
  const [notice, setNotice] = useState("");
  const [noticeTone, setNoticeTone] = useState<"neutral" | "success" | "error">("neutral");
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [pendingUninstall, setPendingUninstall] = useState("");
  const [developerOpen, setDeveloperOpen] = useState(false);
  const [newProjectId, setNewProjectId] = useState("");
  const [newProjectName, setNewProjectName] = useState("");
  const [newProjectType, setNewProjectType] = useState<ProjectKind>("bundle");

  const selected = useMemo(
    () => plugins.find((plugin) => plugin.id === selectedId) ?? plugins[0] ?? null,
    [plugins, selectedId],
  );
  const capabilities = useMemo(
    () => manifest?.capabilities ?? (selected ? inferLegacyCapabilities(selected) : []),
    [manifest, selected],
  );
  const tabs = useMemo(() => buildTabs(capabilities, selected?.debugger?.panels), [capabilities, selected?.debugger?.panels]);

  const loadPluginDetails = async (plugin: PluginRecord) => {
    setConfigDraft(structuredClone(plugin.config));
    setAutostart(plugin.autostart);
    setHealth(null);
    const fallbackManifest = normalizeManifest(plugin);
    const nextManifest = pluginGateway.getPluginManifest
      ? await pluginGateway.getPluginManifest(plugin.id).catch(() => fallbackManifest)
      : fallbackManifest;
    setManifest(nextManifest);
    const nextCapabilities = nextManifest.capabilities;
    const [nextLogs, nextTools] = await Promise.all([
      hasCapability(nextCapabilities, "service")
        ? pluginGateway.getPluginLogs(plugin.id).catch(() => [])
        : Promise.resolve([]),
      hasCapability(nextCapabilities, "tools") && pluginGateway.listPluginTools
        ? pluginGateway.listPluginTools(plugin.id).catch(() => [])
        : Promise.resolve([]),
    ]);
    setLogs(nextLogs);
    setTools(nextTools);
    const nextTabs = buildTabs(nextCapabilities, plugin.debugger?.panels);
    setActiveTab((current) => nextTabs.some((tab) => tab.id === current) ? current : "overview");
  };

  const reload = async (preferredId?: string) => {
    setLoading(true);
    try {
      const [nextPlugins, nextProjects] = await Promise.all([
        pluginGateway.listPlugins(),
        pluginGateway.listPluginProjects(),
      ]);
      setPlugins(nextPlugins);
      setProjects(nextProjects);
      const id = preferredId && nextPlugins.some((plugin) => plugin.id === preferredId)
        ? preferredId
        : selectedId && nextPlugins.some((plugin) => plugin.id === selectedId)
          ? selectedId
          : nextPlugins[0]?.id ?? "";
      setSelectedId(id);
      const plugin = nextPlugins.find((item) => item.id === id);
      if (plugin) await loadPluginDetails(plugin);
      else {
        setManifest(null);
        setTools([]);
        setLogs([]);
      }
    } catch (error) {
      showNotice(`读取插件失败：${String(error)}`, "error");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void reload();
  }, []);

  useEffect(() => {
    if (!selected) return;
    const timer = window.setInterval(() => {
      void pluginGateway.listPlugins()
        .then((next) => {
          setPlugins(next);
          const current = next.find((plugin) => plugin.id === selected.id);
          if (current?.status === "running" && hasCapability(capabilities, "service")) {
            void pluginGateway.getPluginLogs(current.id).then(setLogs).catch(() => undefined);
          }
        })
        .catch(() => undefined);
    }, 1800);
    return () => window.clearInterval(timer);
  }, [capabilities, selected?.id]);

  const showNotice = (message: string, tone: "neutral" | "success" | "error" = "neutral") => {
    setNotice(message);
    setNoticeTone(tone);
  };

  const selectPlugin = async (plugin: PluginRecord) => {
    setSelectedId(plugin.id);
    setActiveTab("overview");
    setNotice("");
    setLoading(true);
    try {
      await loadPluginDetails(plugin);
    } catch (error) {
      showNotice(`读取插件清单失败：${String(error)}`, "error");
    } finally {
      setLoading(false);
    }
  };

  const runOperation = async (operation: () => Promise<void>, success: string) => {
    if (busy) return;
    setBusy(true);
    setNotice("");
    try {
      await operation();
      await reload(selected?.id);
      showNotice(success, "success");
    } catch (error) {
      showNotice(String(error), "error");
    } finally {
      setBusy(false);
    }
  };

  const install = async () => {
    const selectedPath = await open({
      multiple: false,
      filters: [{ name: "DRPA Plugin", extensions: ["drpa-plugin"] }],
    });
    if (!selectedPath || Array.isArray(selectedPath)) return;
    setBusy(true);
    try {
      const installed = await pluginGateway.installPlugin(selectedPath);
      await reload(installed.id);
      showNotice(`插件已安装：${installed.name}`, "success");
    } catch (error) {
      showNotice(`安装失败：${String(error)}`, "error");
    } finally {
      setBusy(false);
    }
  };

  const saveConfig = () => selected && runOperation(
    () => pluginGateway.savePluginConfig(selected.id, configDraft, autostart),
    "配置已保存；运行中的服务重启后应用新配置。",
  );

  const updateConfig = (key: string, value: unknown) => {
    setConfigDraft((current) => ({ ...current, [key]: value }));
  };

  const checkHealth = async () => {
    if (!selected || busy) return;
    setBusy(true);
    setNotice("");
    try {
      const healthEndpoint = (selected.debugger?.endpoints ?? [])
        .map(normalizeDebuggerEndpoint)
        .find((endpoint) => endpoint.kind === "health" || endpoint.id === "health");
      const result = pluginGateway.checkPluginHealth
        ? await pluginGateway.checkPluginHealth(selected.id)
        : pluginGateway.runPluginDebugger && healthEndpoint
          ? await pluginGateway.runPluginDebugger(selected.id, healthEndpoint.id).then((response) => {
            const ok = response.status >= 200 && response.status < 300;
            return {
              ok,
              status: ok ? "healthy" as const : "unhealthy" as const,
              message: ok ? "服务健康检查通过" : `服务健康检查返回 HTTP ${response.status}`,
              checkedAt: Date.now(),
              durationMs: response.durationMs,
              details: { response: response.body },
            };
          })
          : await pluginGateway.testPluginConnection(selected.id).then((legacy) => ({
            ok: legacy.ok,
            status: legacy.ok ? "healthy" as const : "unhealthy" as const,
            message: legacy.message,
            checkedAt: Date.now(),
            durationMs: legacy.duration_ms,
            details: legacy.details,
          }));
      setHealth(result);
      showNotice(`${result.message} · ${result.durationMs} ms`, result.ok ? "success" : "error");
    } catch (error) {
      const next: PluginHealth = {
        ok: false,
        status: "unhealthy",
        message: String(error),
        checkedAt: Date.now(),
        durationMs: 0,
      };
      setHealth(next);
      showNotice(next.message, "error");
    } finally {
      setBusy(false);
    }
  };

  const useAsProvider = (selectedProvider?: PluginCapability) => {
    if (!selected) return;
    const provider = selectedProvider?.kind === "provider"
      ? selectedProvider
      : capabilities.find((capability) => capability.kind === "provider");
    const service = capabilities.find((capability) => capability.kind === "service");
    const endpoint = provider?.endpoint || service?.endpoint || selected.endpoint;
    if (!endpoint) return;
    setAgentBaseUrl(endpoint);
    setAgentModel(String(configDraft[provider?.modelConfigKey || "model"] ?? configDraft.model ?? selected.name));
    setAgentApiKey("");
    setAgentProviderRef({
      pluginId: selected.id,
      providerId: provider?.providerId ?? provider?.id.replace(/^provider:/, "") ?? "default",
    });
    setActiveNavigation("agent");
  };

  const createProject = async () => {
    const id = newProjectId.trim().toLowerCase().replace(/\s+/g, "-");
    const name = newProjectName.trim() || id;
    if (!id || busy) return;
    setBusy(true);
    try {
      await pluginGateway.createPluginProject(id, name, newProjectType);
      setNewProjectId("");
      setNewProjectName("");
      await reload(selected?.id);
      showNotice(`插件项目已创建：${id}`, "success");
    } catch (error) {
      showNotice(String(error), "error");
    } finally {
      setBusy(false);
    }
  };

  const buildAndInstallProject = async (project: PluginProject) => {
    setBusy(true);
    try {
      await pluginGateway.validatePluginProject(project.id);
      const packagePath = await pluginGateway.buildPluginProject(project.id);
      const installed = await pluginGateway.installPlugin(packagePath);
      await reload(installed.id);
      showNotice(`已验证、构建并安装：${packagePath}`, "success");
    } catch (error) {
      showNotice(String(error), "error");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="page generic-plugins-page">
      <header className="page-header generic-plugins-header">
        <div>
          <div className="eyebrow">MANIFEST-DRIVEN EXTENSIONS</div>
          <h1>插件工作台</h1>
          <p>插件清单声明能力，DRPA 按需提供配置、服务、工具、调试和自定义面板。</p>
        </div>
        <div className="page-actions">
          <button className="button secondary" type="button" onClick={() => setDeveloperOpen((openState) => !openState)}>
            <Hammer size={13} /> 插件开发
          </button>
          <button className="button secondary" type="button" onClick={() => void reload(selected?.id)} disabled={busy || loading}>
            <RefreshCw className={loading ? "spin" : ""} size={13} /> 刷新
          </button>
          <button className="button primary" type="button" onClick={() => void install()} disabled={busy}>
            <Download size={13} /> 安装插件
          </button>
        </div>
      </header>

      {developerOpen && (
        <section className="plugin-workbench-developer">
          <header>
            <div><Hammer size={14} /><span><strong>插件开发</strong><small>创建能力清单项目，验证后构建离线 `.drpa-plugin`。</small></span></div>
            <em>{projects.length} 个项目</em>
          </header>
          <div className="plugin-workbench-project-create">
            <input aria-label="插件项目 ID" value={newProjectId} onChange={(event) => setNewProjectId(event.target.value.toLowerCase())} placeholder="example-plugin" />
            <input aria-label="插件项目名称" value={newProjectName} onChange={(event) => setNewProjectName(event.target.value)} placeholder="Example Plugin" />
            <select aria-label="插件项目能力模板" value={newProjectType} onChange={(event) => setNewProjectType(event.target.value as ProjectKind)}>
              <option value="bundle">组合能力包（推荐）</option>
              <option value="tool">工具提供者</option>
              <option value="service">后台服务 / API</option>
            </select>
            <button className="button primary small" type="button" onClick={() => void createProject()} disabled={!newProjectId.trim() || busy}>
              <FileCode2 size={12} /> 创建
            </button>
          </div>
          <div className="plugin-workbench-project-list">
            {projects.map((project) => (
              <article key={project.id}>
                <span className={project.valid ? "valid" : "invalid"}>{project.valid ? <CheckCircle2 size={14} /> : <CircleAlert size={14} />}</span>
                <div><strong>{project.name}</strong><code>{project.id} · v{project.version} · {project.types.join(" / ")}</code><small>{project.validationMessage}</small></div>
                <button className="button secondary small" type="button" onClick={() => void buildAndInstallProject(project)} disabled={!project.valid || busy}><Hammer size={12} /> 构建并安装</button>
              </article>
            ))}
            {projects.length === 0 && <p>还没有插件项目。项目模板不会绑定任何第三方实现。</p>}
          </div>
        </section>
      )}

      {notice && (
        <div className={`plugin-workbench-notice ${noticeTone}`} role="status" aria-live="polite">
          {busy ? <LoaderCircle className="spin" size={13} /> : noticeTone === "error" ? <CircleAlert size={13} /> : noticeTone === "success" ? <CheckCircle2 size={13} /> : <Activity size={13} />}
          <span>{notice}</span>
          <button type="button" aria-label="关闭提示" onClick={() => setNotice("")}>×</button>
        </div>
      )}

      <div className={`plugin-workbench-layout${catalogCollapsed ? " catalog-collapsed" : ""}`}>
        {catalogCollapsed ? (
          <SidebarToggle id="plugins-catalog" side="left" label="插件列表" restore />
        ) : (
          <aside className="plugin-workbench-catalog collapsible-sidebar" aria-label="已安装插件">
            <SidebarToggle id="plugins-catalog" side="left" label="插件列表" />
            <header><PlugZap size={15} /><strong>已安装插件</strong><span>{plugins.length}</span></header>
            <div className="plugin-workbench-catalog-list">
              {plugins.map((plugin) => {
                const inferred = plugin.manifest?.capabilities ?? inferLegacyCapabilities(plugin);
                return (
                  <button className={plugin.id === selected?.id ? "active" : ""} type="button" onClick={() => void selectPlugin(plugin)} key={plugin.id}>
                    <span className={`plugin-workbench-status-dot ${plugin.status}`} />
                    <div>
                      <strong>{plugin.name}</strong>
                      <small>{plugin.description}</small>
                      <span>{inferred.slice(0, 3).map((capability) => capabilityLabels[capability.kind]).join(" · ") || "基础插件"}</span>
                    </div>
                    <em>v{plugin.version}</em>
                  </button>
                );
              })}
              {!loading && plugins.length === 0 && <div className="plugin-workbench-empty"><Box size={25} /><strong>暂无插件</strong><span>安装 `.drpa-plugin` 后，能力会在这里自动呈现。</span></div>}
              {loading && plugins.length === 0 && <div className="plugin-workbench-empty"><LoaderCircle className="spin" size={22} /><span>正在读取插件清单…</span></div>}
            </div>
          </aside>
        )}

        {selected ? (
          <main className="plugin-workbench-main">
            <section className="plugin-workbench-hero">
              <div className="plugin-workbench-logo"><PlugZap size={22} /></div>
              <div className="plugin-workbench-identity">
                <div><h2>{selected.name}</h2><span className={`plugin-workbench-status ${selected.status}`}>{statusLabels[selected.status]}</span></div>
                <p>{manifest?.description || selected.description}</p>
                <code>{selected.id} · v{selected.version} · manifest v{manifest?.schemaVersion ?? 1}</code>
                <div className="plugin-workbench-capability-badges">
                  {capabilities.map((capability) => <span key={capability.id}>{capabilityIcon(capability.kind)} {capability.title}</span>)}
                  {capabilities.length === 0 && <span><Settings2 size={10} /> 仅配置</span>}
                </div>
              </div>
              <div className="plugin-workbench-runtime-actions">
                {hasCapability(capabilities, "service") && (
                  selected.status === "running"
                    ? <button className="button secondary" type="button" onClick={() => void runOperation(() => pluginGateway.stopPlugin(selected.id), "插件服务已停止。")} disabled={busy}><CircleStop size={13} /> 停止</button>
                    : <button className="button primary" type="button" onClick={() => void runOperation(() => pluginGateway.startPlugin(selected.id), "插件服务已启动。")} disabled={busy || !selected.enabled}><Play size={13} /> 启动</button>
                )}
                <button className="button secondary" type="button" onClick={() => void runOperation(() => pluginGateway.setPluginEnabled(selected.id, !selected.enabled), selected.enabled ? "插件已禁用。" : "插件已启用。")} disabled={busy}>
                  {selected.enabled ? "禁用" : "启用"}
                </button>
              </div>
            </section>

            {selected.lastError && <div className="plugin-workbench-runtime-error"><CircleAlert size={15} /><span>{selected.lastError}</span></div>}

            <nav className="plugin-workbench-tabs" role="tablist" aria-label="插件能力">
              {tabs.map((tab) => (
                <button className={activeTab === tab.id ? "active" : ""} type="button" role="tab" aria-selected={activeTab === tab.id} onClick={() => setActiveTab(tab.id)} key={tab.id}>
                  {tabIcon(tab.kind)}<span>{tab.title}</span>
                </button>
              ))}
            </nav>

            <div className="plugin-workbench-content">
              {activeTab === "overview" && (
                <OverviewPanel
                  plugin={selected}
                  manifest={manifest}
                  capabilities={capabilities}
                  health={health}
                  onSelectTab={setActiveTab}
                  onCheckHealth={checkHealth}
                  busy={busy}
                />
              )}
              {activeTab === "config" && (
                <ConfigPanel
                  plugin={selected}
                  config={configDraft}
                  configuredSecrets={selected.configuredSecrets ?? {}}
                  autostart={autostart}
                  busy={busy}
                  onChange={updateConfig}
                  onAutostart={setAutostart}
                  onSave={saveConfig}
                  onError={(message) => showNotice(message, "error")}
                />
              )}
              {activeTab === "service" && (
                <ServicePanel
                  plugin={selected}
                  capabilities={capabilities}
                  health={health}
                  busy={busy}
                  onCheckHealth={checkHealth}
                  onUseAsProvider={useAsProvider}
                />
              )}
              {activeTab === "tools" && (
                <ToolsPanel plugin={selected} tools={tools} gateway={pluginGateway} onNotice={showNotice} />
              )}
              {activeTab === "debugger" && (
                <DebuggerPanel plugin={selected} capabilities={capabilities} config={configDraft} gateway={pluginGateway} onNotice={showNotice} />
              )}
              {activeTab === "events" && <TrafficEventsPanel logs={logs} />}
              {activeTab === "logs" && <LogsPanel logs={logs} running={selected.status === "running"} />}
              {activeTab.startsWith("panel:") && (
                <CustomPanel
                  plugin={selected}
                  capability={capabilities.find((capability) => `panel:${capability.id}` === activeTab)}
                  gateway={pluginGateway}
                  onNotice={showNotice}
                />
              )}
            </div>

            <footer className="plugin-workbench-danger">
              <div><Trash2 size={14} /><span><strong>卸载插件</strong><small>停止插件服务，并删除程序、清单与当前工作区配置。</small></span></div>
              <button className="button danger small" type="button" onClick={() => setPendingUninstall(selected.id)}>卸载</button>
            </footer>
          </main>
        ) : (
          <div className="plugin-workbench-welcome">
            <PlugZap size={30} />
            <h2>通用插件工作台</h2>
            <p>安装插件后，界面会根据 manifest capabilities 自动组合需要的管理面板。</p>
            <button className="button primary" type="button" onClick={() => void install()}><Download size={14} /> 安装插件</button>
          </div>
        )}
      </div>

      {pendingUninstall && (
        <Modal title="卸载插件？" onClose={() => setPendingUninstall("")}>
          <div className="plugin-workbench-confirm">
            <CircleAlert size={22} />
            <p>插件 <strong>{pendingUninstall}</strong> 的服务、程序和本地配置将被删除。</p>
            <footer>
              <button className="button secondary" type="button" onClick={() => setPendingUninstall("")}>取消</button>
              <button className="button danger" type="button" onClick={() => void runOperation(async () => {
                await pluginGateway.uninstallPlugin(pendingUninstall);
                setPendingUninstall("");
                setSelectedId("");
              }, "插件已卸载。")}>确认卸载</button>
            </footer>
          </div>
        </Modal>
      )}
    </div>
  );
}

function OverviewPanel({
  plugin,
  manifest,
  capabilities,
  health,
  busy,
  onSelectTab,
  onCheckHealth,
}: {
  plugin: PluginRecord;
  manifest: PluginManifestView | null;
  capabilities: PluginCapability[];
  health: PluginHealth | null;
  busy: boolean;
  onSelectTab: (tab: string) => void;
  onCheckHealth: () => Promise<void>;
}) {
  return (
    <div className="plugin-workbench-overview">
      <section className="plugin-workbench-panel">
        <PanelHeader icon={<ListTree size={15} />} title="清单能力" description="页面由 manifest capabilities 自动生成。" />
        <div className="plugin-workbench-capability-grid">
          {capabilities.map((capability) => (
            <button type="button" key={capability.id} onClick={() => onSelectTab(capabilityTab(capability))}>
              <span>{capabilityIcon(capability.kind)}</span>
              <div><strong>{capability.title}</strong><small>{capability.description || capabilityLabels[capability.kind]}</small><code>{capability.kind} · {capability.id}</code></div>
              <ChevronRight size={14} />
            </button>
          ))}
          {capabilities.length === 0 && <div className="plugin-workbench-panel-empty"><Settings2 size={20} /><p>清单未声明扩展能力；仍可使用通用配置。</p></div>}
        </div>
      </section>
      <section className="plugin-workbench-panel">
        <PanelHeader
          icon={<Activity size={15} />}
          title="运行与健康"
          description={hasCapability(capabilities, "service") ? "后台服务由 DRPA 管理。" : "此插件没有声明后台服务。"}
          action={hasCapability(capabilities, "service") ? <button className="button secondary small" type="button" onClick={() => void onCheckHealth()} disabled={busy}><FlaskConical size={12} /> 健康检查</button> : undefined}
        />
        <div className="plugin-workbench-health-summary">
          <span className={`plugin-workbench-health-icon ${health?.status ?? plugin.status}`}>
            {health?.ok || plugin.status === "running" ? <CheckCircle2 size={18} /> : <Activity size={18} />}
          </span>
          <div>
            <strong>{health ? health.message : statusLabels[plugin.status]}</strong>
            <small>{health ? `${formatDateTime(health.checkedAt)} · ${health.durationMs} ms` : "尚未执行健康检查"}</small>
          </div>
        </div>
      </section>
      <section className="plugin-workbench-panel">
        <PanelHeader icon={<FileCode2 size={15} />} title="清单信息" description="用于定位插件来源和兼容性。" />
        <dl className="plugin-workbench-manifest-meta">
          <div><dt>ID</dt><dd>{plugin.id}</dd></div>
          <div><dt>版本</dt><dd>{plugin.version}</dd></div>
          <div><dt>Schema</dt><dd>v{manifest?.schemaVersion ?? 1}</dd></div>
          <div><dt>作者</dt><dd>{manifest?.author || "未声明"}</dd></div>
          <div className="wide"><dt>目录</dt><dd title={plugin.directory}>{plugin.directory}</dd></div>
          {manifest?.homepage && <div className="wide"><dt>主页</dt><dd>{manifest.homepage}</dd></div>}
        </dl>
      </section>
    </div>
  );
}

function ConfigPanel({
  plugin,
  config,
  configuredSecrets,
  autostart,
  busy,
  onChange,
  onAutostart,
  onSave,
  onError,
}: {
  plugin: PluginRecord;
  config: Record<string, unknown>;
  configuredSecrets: Record<string, boolean>;
  autostart: boolean;
  busy: boolean;
  onChange: (key: string, value: unknown) => void;
  onAutostart: (value: boolean) => void;
  onSave: () => void;
  onError: (message: string) => void;
}) {
  const properties = plugin.configSchema.properties ?? {};
  const keys = [...new Set([...Object.keys(properties), ...Object.keys(config)])];
  const required = new Set(plugin.configSchema.required ?? []);
  return (
    <section className="plugin-workbench-panel plugin-workbench-config">
      <PanelHeader
        icon={<Settings2 size={15} />}
        title="插件配置"
        description="字段、类型和说明来自 config.schema.json；工作台不内置特定插件配置。"
        action={<button className="button primary small" type="button" onClick={onSave} disabled={busy}><Save size={12} /> 保存配置</button>}
      />
      <div className="plugin-workbench-config-grid">
        {keys.map((key) => (
          <ConfigField
            key={key}
            fieldKey={key}
            descriptor={properties[key] ?? inferConfigDescriptor(config[key])}
            value={config[key]}
            configuredSecret={Boolean(configuredSecrets[key])}
            required={required.has(key)}
            onChange={(value) => onChange(key, value)}
            onError={onError}
          />
        ))}
        {keys.length === 0 && <div className="plugin-workbench-panel-empty"><Settings2 size={21} /><p>这个插件没有可编辑配置。</p></div>}
        <label className="plugin-workbench-switch-field">
          <span><strong>随 DRPA 启动</strong><small>仅对声明了 service capability 的插件生效。</small></span>
          <button className={`switch ${autostart ? "on" : ""}`} type="button" role="switch" aria-label="插件自动启动" aria-checked={autostart} onClick={() => onAutostart(!autostart)}><span /></button>
        </label>
      </div>
    </section>
  );
}

function ConfigField({
  fieldKey,
  descriptor,
  value,
  configuredSecret,
  required,
  onChange,
  onError,
}: {
  fieldKey: string;
  descriptor: ConfigFieldDescriptor;
  value: unknown;
  configuredSecret: boolean;
  required: boolean;
  onChange: (value: unknown) => void;
  onError: (message: string) => void;
}) {
  if (descriptor.type === "boolean" || typeof value === "boolean") {
    return (
      <label className="plugin-workbench-switch-field">
        <span><strong>{descriptor.title || fieldKey}{required && <em>必填</em>}</strong><small>{descriptor.description}</small></span>
        <button className={`switch ${value ? "on" : ""}`} type="button" role="switch" aria-label={descriptor.title || fieldKey} aria-checked={Boolean(value)} onClick={() => onChange(!value)}><span /></button>
      </label>
    );
  }
  if (descriptor.type === "object" || descriptor.type === "array" || (value !== null && typeof value === "object")) {
    return <JsonConfigField fieldKey={fieldKey} descriptor={descriptor} value={value} required={required} onChange={onChange} onError={onError} />;
  }
  if (descriptor.enum) {
    return (
      <label className="plugin-workbench-config-field">
        <span>{descriptor.title || fieldKey}{required && <em>必填</em>}</span>
        <select value={String(value ?? descriptor.default ?? "")} onChange={(event) => onChange(coerceEnumValue(event.target.value, descriptor.enum ?? []))}>
          {descriptor.enum.map((option) => <option value={String(option)} key={String(option)}>{String(option)}</option>)}
        </select>
        <small>{descriptor.description}</small>
      </label>
    );
  }
  const numeric = descriptor.type === "integer" || descriptor.type === "number" || typeof value === "number";
  const multiline = descriptor.multiline || ["textarea", "multiline"].includes(descriptor.format ?? "");
  return (
    <label className="plugin-workbench-config-field">
      <span>{descriptor.title || fieldKey}{required && <em>必填</em>}</span>
      {multiline
        ? <textarea value={String(value ?? descriptor.default ?? "")} onChange={(event) => onChange(event.target.value)} rows={4} />
        : <input
            type={descriptor.secret || descriptor.format === "password" ? "password" : numeric ? "number" : "text"}
            value={String(value ?? descriptor.default ?? "")}
            min={descriptor.minimum}
            max={descriptor.maximum}
            placeholder={configuredSecret ? "已配置，留空保持不变" : undefined}
            onChange={(event) => onChange(numeric ? Number(event.target.value) : event.target.value)}
          />}
      <small>{configuredSecret ? `${descriptor.description ? `${descriptor.description} · ` : ""}密钥已安全保存，页面不会读取原值。` : descriptor.description}</small>
    </label>
  );
}

function JsonConfigField({
  fieldKey,
  descriptor,
  value,
  required,
  onChange,
  onError,
}: {
  fieldKey: string;
  descriptor: ConfigFieldDescriptor;
  value: unknown;
  required: boolean;
  onChange: (value: unknown) => void;
  onError: (message: string) => void;
}) {
  const [source, setSource] = useState(() => prettyJson(value ?? descriptor.default ?? (descriptor.type === "array" ? [] : {})));
  useEffect(() => setSource(prettyJson(value ?? descriptor.default ?? (descriptor.type === "array" ? [] : {}))), [value]);
  const commit = () => {
    try {
      onChange(JSON.parse(source));
    } catch (error) {
      onError(`${descriptor.title || fieldKey} 不是有效 JSON：${String(error)}`);
    }
  };
  return (
    <label className="plugin-workbench-config-field wide">
      <span>{descriptor.title || fieldKey}{required && <em>必填</em>}</span>
      <textarea className="json" value={source} onChange={(event) => setSource(event.target.value)} onBlur={commit} rows={6} />
      <small>{descriptor.description || "请输入有效 JSON。"}</small>
    </label>
  );
}

function ServicePanel({
  plugin,
  capabilities,
  health,
  busy,
  onCheckHealth,
  onUseAsProvider,
}: {
  plugin: PluginRecord;
  capabilities: PluginCapability[];
  health: PluginHealth | null;
  busy: boolean;
  onCheckHealth: () => Promise<void>;
  onUseAsProvider: (provider: PluginCapability) => void;
}) {
  const serviceCapabilities = capabilities.filter((capability) => capability.kind === "service");
  const providerCapabilities = capabilities.filter((capability) => capability.kind === "provider");
  return (
    <div className="plugin-workbench-service-grid">
      <section className="plugin-workbench-panel">
        <PanelHeader icon={<Server size={15} />} title="后台服务" description="服务与 Endpoint 来自 manifest services[]。" />
        <div className="plugin-workbench-service-status">
          <span className={`plugin-workbench-status-dot ${plugin.status}`} />
          <div><strong>{statusLabels[plugin.status]}</strong><small>{plugin.enabled ? "插件已启用" : "插件当前被禁用"}</small></div>
        </div>
        <div className="plugin-workbench-service-list">
          {serviceCapabilities.map((service) => <article key={service.id}><div><strong>{service.title}</strong><small>{service.description}</small></div><code>{service.endpoint || "未导出 Endpoint"}</code><button type="button" aria-label={`复制 ${service.title} Endpoint`} disabled={!service.endpoint} onClick={() => void navigator.clipboard.writeText(service.endpoint ?? "")}><Copy size={12} /></button></article>)}
          {serviceCapabilities.length === 0 && <div className="plugin-workbench-panel-empty compact"><Server size={18} /><p>未声明后台服务。</p></div>}
        </div>
      </section>
      <section className="plugin-workbench-panel">
        <PanelHeader icon={<Bot size={15} />} title="Provider" description="只有 manifest providers[] 声明的 Provider 才能接入 AI Agent。" />
        <div className="plugin-workbench-provider-list">
          {providerCapabilities.map((provider) => <article key={provider.id}><div><strong>{provider.title}</strong><small>{provider.protocol || "provider"}</small><code>{provider.endpoint || "未导出 Endpoint"}</code></div><button className="button primary small" type="button" onClick={() => onUseAsProvider(provider)} disabled={!provider.endpoint || plugin.status !== "running"}><Bot size={12} /> 用于 AI Agent</button></article>)}
          {providerCapabilities.length === 0 && <div className="plugin-workbench-panel-empty compact"><Bot size={18} /><p>未声明模型 Provider。</p></div>}
        </div>
      </section>
      <section className="plugin-workbench-panel plugin-workbench-health-panel">
        <PanelHeader icon={<Activity size={15} />} title="健康检查" description="检查进程、端口及插件自定义探针。" action={<button className="button secondary small" type="button" onClick={() => void onCheckHealth()} disabled={busy}><FlaskConical size={12} /> 立即检查</button>} />
        {health ? <div className={`plugin-workbench-health-detail ${health.status}`}><span>{health.ok ? <CheckCircle2 size={20} /> : <CircleAlert size={20} />}</span><div><strong>{health.message}</strong><small>{formatDateTime(health.checkedAt)} · {health.durationMs} ms</small></div>{health.details && <pre>{prettyJson(health.details)}</pre>}</div> : <div className="plugin-workbench-panel-empty"><Activity size={20} /><p>尚未执行健康检查。</p></div>}
      </section>
    </div>
  );
}

function ToolsPanel({
  plugin,
  tools,
  gateway,
  onNotice,
}: {
  plugin: PluginRecord;
  tools: PluginToolDescriptor[];
  gateway: PluginWorkbenchGateway;
  onNotice: (message: string, tone?: "neutral" | "success" | "error") => void;
}) {
  const [selectedTool, setSelectedTool] = useState("");
  const [input, setInput] = useState("{}");
  const [result, setResult] = useState<PluginToolResult | null>(null);
  const [running, setRunning] = useState(false);
  const activeTool = tools.find((tool) => tool.name === selectedTool) ?? tools[0];
  useEffect(() => {
    if (activeTool && selectedTool !== activeTool.name) setSelectedTool(activeTool.name);
  }, [activeTool?.name]);
  const invoke = async () => {
    if (!activeTool || running) return;
    if (!gateway.invokePluginTool) {
      onNotice("后端尚未接入 invokePluginTool，当前只能查看工具清单。", "error");
      return;
    }
    let payload: Record<string, unknown>;
    try {
      payload = JSON.parse(input) as Record<string, unknown>;
    } catch (error) {
      onNotice(`工具参数不是有效 JSON：${String(error)}`, "error");
      return;
    }
    setRunning(true);
    setResult(null);
    try {
      const next = await gateway.invokePluginTool(plugin.id, activeTool.name, payload);
      setResult(next);
      onNotice(next.ok ? `工具 ${activeTool.name} 调用成功。` : next.error || "工具调用失败。", next.ok ? "success" : "error");
    } catch (error) {
      onNotice(String(error), "error");
    } finally {
      setRunning(false);
    }
  };
  return (
    <div className="plugin-workbench-tools">
      <section className="plugin-workbench-tool-list">
        <header><ListTree size={14} /><strong>工具清单</strong><span>{tools.length}</span></header>
        {tools.map((tool) => <button className={tool.name === activeTool?.name ? "active" : ""} type="button" key={tool.name} onClick={() => { setSelectedTool(tool.name); setInput("{}"); setResult(null); }}><strong>{tool.title || tool.name}</strong><code>{tool.name}</code><small>{tool.description}</small></button>)}
        {tools.length === 0 && <div className="plugin-workbench-panel-empty"><Wrench size={20} /><p>{gateway.listPluginTools ? "插件没有导出工具。" : "后端尚未提供工具清单接口。"}</p></div>}
      </section>
      <section className="plugin-workbench-panel plugin-workbench-tool-runner">
        <PanelHeader icon={<Wrench size={15} />} title={activeTool?.title || "工具调用"} description={activeTool?.description || "选择一个工具检查参数并执行。"} />
        {activeTool && <>
          <label>输入 Schema<pre>{prettyJson(activeTool.inputSchema)}</pre></label>
          <label>调用参数<textarea className="json" value={input} onChange={(event) => setInput(event.target.value)} rows={8} /></label>
          <button className="button primary" type="button" onClick={() => void invoke()} disabled={running}>{running ? <LoaderCircle className="spin" size={13} /> : <Play size={13} />} 执行工具</button>
          {result && <div className={`plugin-workbench-tool-result ${result.ok ? "success" : "error"}`}><strong>{result.ok ? `执行成功 · ${result.durationMs} ms` : "执行失败"}</strong><pre>{prettyJson(result.ok ? result.output : result.error)}</pre></div>}
        </>}
      </section>
    </div>
  );
}

function DebuggerPanel({
  plugin,
  capabilities,
  config,
  gateway,
  onNotice,
}: {
  plugin: PluginRecord;
  capabilities: PluginCapability[];
  config: Record<string, unknown>;
  gateway: PluginWorkbenchGateway;
  onNotice: (message: string, tone?: "neutral" | "success" | "error") => void;
}) {
  const service = capabilities.find((capability) => capability.kind === "service");
  const debuggerCapability = capabilities.find((capability) => capability.kind === "debugger");
  const endpoints = (plugin.debugger?.endpoints ?? debuggerCapability?.endpoints ?? []).map(normalizeDebuggerEndpoint);
  const serviceEndpoint = debuggerCapability?.endpoint || service?.endpoint || plugin.endpoint;
  const [selectedEndpointId, setSelectedEndpointId] = useState(endpoints[0]?.id ?? "");
  const activeEndpoint = endpoints.find((endpoint) => endpoint.id === selectedEndpointId) ?? endpoints[0];
  const [requestSource, setRequestSource] = useState(() => prettyJson(activeEndpoint?.requestDefaults ?? {}));
  const [response, setResponse] = useState<PluginDebuggerResponse | null>(null);
  const [running, setRunning] = useState(false);
  useEffect(() => {
    if (!activeEndpoint) return;
    if (selectedEndpointId !== activeEndpoint.id) setSelectedEndpointId(activeEndpoint.id);
    setRequestSource(prettyJson(activeEndpoint.requestDefaults ?? {}));
    setResponse(null);
  }, [activeEndpoint?.id]);
  const send = async (event: FormEvent) => {
    event.preventDefault();
    if (!activeEndpoint) return;
    if (!gateway.runPluginDebugger) {
      onNotice("后端尚未接入 runPluginDebugger。调试请求必须由宿主代理发送，不能由 WebView 绕过权限。", "error");
      return;
    }
    let request: unknown;
    try {
      request = JSON.parse(requestSource) as unknown;
    } catch (error) {
      onNotice(`请求 JSON 无效：${String(error)}`, "error");
      return;
    }
    setRunning(true);
    setResponse(null);
    try {
      const next = await gateway.runPluginDebugger(plugin.id, activeEndpoint.id, request);
      setResponse(next);
      onNotice(`API 返回 ${next.status} · ${next.durationMs} ms`, next.status < 400 ? "success" : "error");
    } catch (error) {
      onNotice(String(error), "error");
    } finally {
      setRunning(false);
    }
  };
  const selectEndpoint = (endpoint: PluginDebuggerEndpoint) => {
    setSelectedEndpointId(endpoint.id);
    setRequestSource(prettyJson(endpoint.requestDefaults ?? {}));
    setResponse(null);
  };
  return (
    <div className="plugin-workbench-debugger">
      <form className="plugin-workbench-panel" onSubmit={send}>
        <PanelHeader icon={<Globe2 size={15} />} title="API 调试器" description="请求由 DRPA 宿主代理发送，并受插件权限、清单端点和超时设置限制。" />
        {endpoints.length > 0
          ? <div className="plugin-workbench-debug-routes" aria-label="清单声明的调试端点">{endpoints.map((endpoint) => <button className={endpoint.id === activeEndpoint?.id ? "active" : ""} type="button" onClick={() => selectEndpoint(endpoint)} key={endpoint.id}><span>{endpoint.method}</span><div><strong>{endpoint.title}</strong><code>{endpoint.endpoint}</code><small>{endpoint.kind}{endpoint.timeoutSeconds ? ` · ${endpoint.timeoutSeconds}s` : ""}</small></div></button>)}</div>
          : <div className="plugin-workbench-panel-empty compact"><Braces size={18} /><p>manifest 没有声明 debugger.endpoints。</p></div>}
        {activeEndpoint && <>
          <div className="plugin-workbench-debug-endpoint"><span>{activeEndpoint.method}</span><code>{resolveDebugEndpoint(serviceEndpoint, renderConfigTemplate(activeEndpoint.endpoint, config))}</code></div>
          {activeEndpoint.bearerConfigKey && <div className="plugin-workbench-debug-credential"><Settings2 size={12} /><span>Bearer 凭据来自配置键 <code>{activeEndpoint.bearerConfigKey}</code>，不会显示在页面中。</span></div>}
          <label>请求覆盖 JSON<textarea className="json" value={requestSource} onChange={(event) => setRequestSource(event.target.value)} rows={activeEndpoint.method === "GET" ? 6 : 11} /><small>在清单 requestDefaults 基础上覆盖；可用于请求体、Header 或查询参数。</small></label>
          <button className="button primary" type="submit" disabled={running || (!serviceEndpoint && !/^https?:\/\//i.test(activeEndpoint.endpoint))}>{running ? <LoaderCircle className="spin" size={13} /> : <Send size={13} />} 运行端点</button>
        </>}
      </form>
      <section className="plugin-workbench-panel plugin-workbench-response">
        <PanelHeader icon={<Braces size={15} />} title="响应" description="宿主返回状态、Content-Type、耗时与响应正文。" />
        {response ? <>
          <div className={`plugin-workbench-response-status ${response.status < 400 ? "success" : "error"}`}><strong>HTTP {response.status}</strong><span>{response.durationMs} ms</span></div>
          <dl className="plugin-workbench-response-meta"><div><dt>端点</dt><dd>{response.endpointId}</dd></div><div><dt>Content-Type</dt><dd>{response.contentType || "unknown"}</dd></div><div><dt>截断</dt><dd>{response.truncated ? "是" : "否"}</dd></div></dl>
          <label>Body<pre>{typeof response.body === "string" ? response.body : prettyJson(response.body)}</pre></label>
        </> : <div className="plugin-workbench-panel-empty"><Braces size={20} /><p>运行清单端点后在这里查看响应。</p></div>}
      </section>
    </div>
  );
}

function TrafficEventsPanel({ logs }: { logs: PluginLogLine[] }) {
  const events = useMemo(() => extractTrafficEvents(logs), [logs]);
  const kinds = useMemo(() => [...new Set(events.map((event) => event.kind))].sort(), [events]);
  const [kind, setKind] = useState("all");
  const [query, setQuery] = useState("");
  const normalized = query.trim().toLowerCase();
  const filtered = events.filter((event) => {
    if (kind !== "all" && event.kind !== kind) return false;
    return !normalized || `${event.requestId} ${event.title} ${prettyJson(event.detail)}`.toLowerCase().includes(normalized);
  });
  return (
    <section className="plugin-workbench-panel plugin-workbench-events">
      <PanelHeader icon={<Activity size={15} />} title="流量事件" description="读取宿主解析后的结构化 event；旧插件仍兼容 DRPA_PLUGIN_EVENT stdout。" />
      <div className="plugin-workbench-event-toolbar">
        <select aria-label="按事件类型筛选" value={kind} onChange={(event) => setKind(event.target.value)}>
          <option value="all">全部类型</option>
          {kinds.map((item) => <option value={item} key={item}>{item}</option>)}
        </select>
        <input aria-label="搜索流量事件" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索 req_id、标题或详情…" />
        <span>{filtered.length} / {events.length}</span>
      </div>
      <div className="plugin-workbench-event-list">
        {filtered.map((event, index) => (
          <article key={`${event.timestamp}-${event.requestId}-${index}`}>
            <header><span>{event.kind || "event"}</span><strong>{event.title || "插件事件"}</strong><time>{formatTime(event.timestamp)}</time></header>
            <code>{event.requestId || "无 req_id"}</code>
            <pre>{prettyJson(event.detail)}</pre>
          </article>
        ))}
        {events.length === 0 && <div className="plugin-workbench-panel-empty"><Activity size={21} /><p>服务还没有输出结构化流量事件。</p><small>stdout 行格式：DRPA_PLUGIN_EVENT {"{"}"kind":"…","req_id":"…","title":"…","detail":…{"}"}</small></div>}
        {events.length > 0 && filtered.length === 0 && <div className="plugin-workbench-panel-empty"><Activity size={20} /><p>没有匹配当前筛选条件的事件。</p></div>}
      </div>
    </section>
  );
}

function LogsPanel({ logs, running }: { logs: PluginLogLine[]; running: boolean }) {
  const [stream, setStream] = useState<"all" | "stdout" | "stderr">("all");
  const filtered = stream === "all" ? logs : logs.filter((line) => line.stream === stream);
  return (
    <section className="plugin-workbench-panel plugin-workbench-logs">
      <PanelHeader
        icon={<TerminalSquare size={15} />}
        title="服务日志"
        description="日志按容错 UTF-8 解码，最多显示宿主保留的行数。"
        action={<div className="plugin-workbench-log-filter">{(["all", "stdout", "stderr"] as const).map((value) => <button className={stream === value ? "active" : ""} type="button" onClick={() => setStream(value)} key={value}>{value === "all" ? "全部" : value}</button>)}</div>}
      />
      <div className="plugin-workbench-log-view">
        {filtered.map((line, index) => <div className={line.stream} key={`${line.timestamp}-${index}`}><time>{formatTime(line.timestamp)}</time><span>{line.stream}</span><pre>{line.message}</pre></div>)}
        {filtered.length === 0 && <div className="plugin-workbench-panel-empty"><TerminalSquare size={20} /><p>{running ? "服务尚未产生日志。" : "服务未运行。"}</p></div>}
      </div>
    </section>
  );
}

function CustomPanel({
  plugin,
  capability,
  gateway,
  onNotice,
}: {
  plugin: PluginRecord;
  capability?: PluginCapability;
  gateway: PluginWorkbenchGateway;
  onNotice: (message: string, tone?: "neutral" | "success" | "error") => void;
}) {
  const descriptor = capability?.panel;
  const fields = descriptor?.fields ?? [];
  const [values, setValues] = useState<Record<string, unknown>>(() => Object.fromEntries(fields.map((field) => [field.id, field.default ?? ""])));
  const [result, setResult] = useState<PluginPanelResult | null>(null);
  const [busy, setBusy] = useState(false);
  if (!capability || !descriptor) return <div className="plugin-workbench-panel-empty"><LayoutPanelTop size={22} /><p>面板清单不完整。</p></div>;
  const entry = descriptor.entry || capability.entry;
  if (descriptor.renderer === "iframe" && entry) {
    const panelEntry = resolvePanelEntry(plugin.endpoint, entry);
    if (!panelEntry) {
      return <div className="plugin-workbench-panel-empty"><CircleAlert size={22} /><p>自定义面板只允许应用自身或 loopback HTTP 地址。</p></div>;
    }
    return (
      <section className="plugin-workbench-panel plugin-workbench-custom-frame">
        <PanelHeader icon={<LayoutPanelTop size={15} />} title={capability.title} description={capability.description || "插件自定义面板运行在受限 iframe 中。"} />
        <iframe title={capability.title} src={panelEntry} sandbox="allow-forms allow-scripts" />
      </section>
    );
  }
  if (descriptor.renderer === "markdown") {
    return (
      <section className="plugin-workbench-panel">
        <PanelHeader icon={<LayoutPanelTop size={15} />} title={capability.title} description={capability.description} />
        <pre className="plugin-workbench-panel-document">{descriptor.content || "面板没有内容。"}</pre>
      </section>
    );
  }
  const invoke = async (action: PluginPanelAction) => {
    if (!gateway.invokePluginPanelAction) {
      onNotice("后端尚未接入 invokePluginPanelAction。", "error");
      return;
    }
    setBusy(true);
    try {
      const next = await gateway.invokePluginPanelAction(plugin.id, capability.id, action.id, values);
      setResult(next);
      onNotice(next.message, next.ok ? "success" : "error");
    } catch (error) {
      onNotice(String(error), "error");
    } finally {
      setBusy(false);
    }
  };
  return (
    <section className="plugin-workbench-panel plugin-workbench-custom-form">
      <PanelHeader icon={<LayoutPanelTop size={15} />} title={capability.title} description={capability.description || "由插件清单声明的自定义表单。"} />
      <div className="plugin-workbench-panel-fields">
        {fields.map((field) => <PanelField field={field} value={values[field.id]} onChange={(value) => setValues((current) => ({ ...current, [field.id]: value }))} key={field.id} />)}
        {fields.length === 0 && <div className="plugin-workbench-panel-empty"><LayoutPanelTop size={20} /><p>此面板没有声明字段。</p></div>}
      </div>
      <footer>
        {(descriptor.actions ?? []).map((action) => <button className={`button ${action.style === "primary" ? "primary" : action.style === "danger" ? "danger" : "secondary"}`} type="button" onClick={() => void invoke(action)} disabled={busy} key={action.id}>{busy ? <LoaderCircle className="spin" size={12} /> : <Play size={12} />}{action.label}</button>)}
      </footer>
      {result?.data !== undefined && <pre className="plugin-workbench-panel-result">{prettyJson(result.data)}</pre>}
    </section>
  );
}

function PanelField({ field, value, onChange }: { field: PluginPanelField; value: unknown; onChange: (value: unknown) => void }) {
  if (field.type === "boolean") return <label className="plugin-workbench-switch-field"><span><strong>{field.label}</strong><small>{field.description}</small></span><button className={`switch ${value ? "on" : ""}`} type="button" role="switch" aria-checked={Boolean(value)} aria-label={field.label} onClick={() => onChange(!value)}><span /></button></label>;
  return (
    <label className="plugin-workbench-config-field">
      <span>{field.label}</span>
      {field.type === "select"
        ? <select value={String(value ?? "")} onChange={(event) => onChange(event.target.value)}>{(field.options ?? []).map((option) => <option value={option.value} key={option.value}>{option.label}</option>)}</select>
        : field.type === "textarea"
          ? <textarea value={String(value ?? "")} onChange={(event) => onChange(event.target.value)} rows={5} />
          : <input type={field.secret ? "password" : field.type === "number" ? "number" : "text"} value={String(value ?? "")} onChange={(event) => onChange(field.type === "number" ? Number(event.target.value) : event.target.value)} />}
      <small>{field.description}</small>
    </label>
  );
}

function PanelHeader({ icon, title, description, action }: { icon: ReactNode; title: string; description: string; action?: ReactNode }) {
  return <header className="plugin-workbench-panel-header"><span>{icon}</span><div><h3>{title}</h3><p>{description}</p></div>{action}</header>;
}

function Modal({ title, onClose, children }: { title: string; onClose: () => void; children: ReactNode }) {
  return <div className="plugin-workbench-modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}><section className="plugin-workbench-modal" role="dialog" aria-modal="true" aria-label={title}><header><h2>{title}</h2><button type="button" aria-label="关闭" onClick={onClose}>×</button></header>{children}</section></div>;
}

function normalizeManifest(plugin: PluginRecord): PluginManifestView {
  const raw = plugin.manifest;
  const capabilities = raw?.capabilities?.length ? raw.capabilities : inferLegacyCapabilities(plugin);
  return {
    schemaVersion: raw?.schemaVersion ?? 1,
    id: raw?.id || plugin.id,
    name: raw?.name || plugin.name,
    version: raw?.version || plugin.version,
    description: raw?.description || plugin.description,
    author: raw?.author,
    homepage: raw?.homepage,
    capabilities: capabilities.map((capability) => capability.kind === "debugger" && !capability.endpoints
      ? { ...capability, endpoints: plugin.debugger?.endpoints }
      : capability),
  };
}

function inferLegacyCapabilities(plugin: PluginRecord): PluginCapability[] {
  const lowered = plugin.types.map((type) => type.toLowerCase());
  const capabilities: PluginCapability[] = [];
  const services = plugin.services ?? [];
  if (services.length > 0) {
    for (const service of services) {
      capabilities.push({
        id: `service:${service.id}`,
        kind: "service",
        title: service.title || service.id,
        description: `${service.transport || "process"} 服务${service.primary ? " · 主服务" : ""}`,
        endpoint: service.endpoint,
      });
    }
  } else if (lowered.some((type) => type.includes("service")) || plugin.endpoint) {
    capabilities.push({ id: "service", kind: "service", title: "后台服务", description: "由旧版插件类型推导的服务能力。", endpoint: plugin.endpoint });
  }
  if (plugin.debugger?.endpoints.length || lowered.some((type) => type.includes("debugger"))) {
    capabilities.push({
      id: "debugger",
      kind: "debugger",
      title: "API 调试器",
      description: "运行插件清单声明的受控调试端点。",
      endpoint: services.find((service) => service.primary)?.endpoint || plugin.endpoint,
      endpoints: plugin.debugger?.endpoints,
    });
  }
  for (const panel of (plugin.debugger?.panels ?? []).filter((item) => !["structured-log", "log"].includes(item.kind))) {
    capabilities.push({
      id: panel.id,
      kind: "panel",
      title: panel.title || panel.id,
      description: `调试面板 · ${panel.kind}`,
      entry: panel.endpoint,
      panel: panel.endpoint
        ? { renderer: "iframe", entry: panel.endpoint }
        : { renderer: "markdown", content: prettyJson(panel.config ?? {}) },
    });
  }
  if (lowered.some((type) => type.includes("tool")) || plugin.toolCount > 0) {
    capabilities.push({ id: "tools", kind: "tools", title: "工具", description: `${plugin.toolCount} 个可供 Agent 调用的工具。` });
  }
  const providers = plugin.providers ?? [];
  if (providers.length > 0) {
    for (const provider of providers) {
      capabilities.push({
        id: `provider:${provider.id}`,
        kind: "provider",
        title: provider.title || provider.id,
        description: `${provider.protocol} Provider · 服务 ${provider.serviceId}`,
        endpoint: provider.endpoint,
        protocol: provider.protocol,
        modelConfigKey: provider.modelConfigKey,
        apiKeyConfigKey: provider.apiKeyConfigKey,
        providerId: provider.id,
      });
    }
  } else if (lowered.some((type) => type.includes("provider"))) {
    capabilities.push({ id: "provider", kind: "provider", title: "模型 Provider", description: "由旧版插件类型推导的 Provider。", endpoint: plugin.endpoint, protocol: "openai-compatible", apiKeyConfigKey: "proxy_api_key", providerId: "default" });
  }
  return capabilities;
}

function buildTabs(capabilities: PluginCapability[], debuggerPanels: PluginDebuggerPanel[] = []): WorkbenchTab[] {
  const tabs: WorkbenchTab[] = [
    { id: "overview", title: "概览", kind: "base" },
    { id: "config", title: "配置", kind: "base" },
  ];
  if (hasCapability(capabilities, "service") || hasCapability(capabilities, "provider")) tabs.push({ id: "service", title: "服务 / Provider", kind: "service" });
  if (hasCapability(capabilities, "tools")) tabs.push({ id: "tools", title: "工具", kind: "tools" });
  if (hasCapability(capabilities, "debugger")) tabs.push({ id: "debugger", title: "API 调试", kind: "debugger" });
  if (hasCapability(capabilities, "service")) {
    const eventPanel = debuggerPanels.find((panel) => panel.kind === "structured-log");
    const logPanel = debuggerPanels.find((panel) => panel.kind === "log");
    if (eventPanel || debuggerPanels.length === 0) tabs.push({ id: "events", title: eventPanel?.title || "流量事件", kind: "service" });
    if (logPanel || debuggerPanels.length === 0) tabs.push({ id: "logs", title: logPanel?.title || "日志", kind: "service" });
  }
  for (const capability of capabilities.filter((item) => item.kind === "panel")) {
    tabs.push({ id: `panel:${capability.id}`, title: capability.title, kind: "panel", capability });
  }
  return tabs;
}

function capabilityTab(capability: PluginCapability) {
  if (capability.kind === "panel") return `panel:${capability.id}`;
  if (capability.kind === "provider") return "service";
  if (capability.kind === "service") return "service";
  return capability.kind;
}

function hasCapability(capabilities: PluginCapability[], kind: CapabilityKind) {
  return capabilities.some((capability) => capability.kind === kind);
}

function capabilityIcon(kind: CapabilityKind) {
  if (kind === "service") return <Server size={11} />;
  if (kind === "tools") return <Wrench size={11} />;
  if (kind === "debugger") return <Braces size={11} />;
  if (kind === "panel") return <LayoutPanelTop size={11} />;
  return <Bot size={11} />;
}

function tabIcon(kind: WorkbenchTab["kind"]) {
  if (kind === "service") return <Server size={13} />;
  if (kind === "tools") return <Wrench size={13} />;
  if (kind === "debugger") return <Braces size={13} />;
  if (kind === "panel") return <LayoutPanelTop size={13} />;
  return <Settings2 size={13} />;
}

function inferConfigDescriptor(value: unknown): ConfigFieldDescriptor {
  if (typeof value === "boolean") return { type: "boolean" };
  if (typeof value === "number") return { type: "number" };
  if (Array.isArray(value)) return { type: "array" };
  if (value !== null && typeof value === "object") return { type: "object" };
  return { type: "string" };
}

function coerceEnumValue(value: string, options: Array<string | number>) {
  const matched = options.find((option) => String(option) === value);
  return matched ?? value;
}

function resolvePanelEntry(endpoint: string, entry: string) {
  try {
    const candidate = /^[a-z]+:/i.test(entry)
      ? new URL(entry)
      : new URL(entry, endpoint.endsWith("/") ? endpoint : `${endpoint}/`);
    const loopback = candidate.protocol === "http:"
      && (candidate.hostname === "127.0.0.1" || candidate.hostname === "[::1]" || candidate.hostname === "::1");
    const sameOrigin = typeof window !== "undefined" && candidate.origin === window.location.origin;
    return loopback || sameOrigin ? candidate.toString() : "";
  } catch {
    return "";
  }
}

function resolveDebugEndpoint(serviceEndpoint: string, endpoint: string) {
  if (/^https?:\/\//i.test(endpoint)) return endpoint;
  if (!serviceEndpoint) return endpoint;
  try {
    return new URL(endpoint, serviceEndpoint.endsWith("/") ? serviceEndpoint : `${serviceEndpoint}/`).toString();
  } catch {
    return endpoint;
  }
}

function renderConfigTemplate(source: string, config: Record<string, unknown>) {
  return source.replace(/\{config\.([A-Za-z0-9_.-]+)\}/g, (_match, key: string) => String(config[key] ?? `{config.${key}}`));
}

function normalizeDebuggerEndpoint(endpoint: PluginDebuggerEndpoint): PluginDebuggerEndpoint {
  return {
    ...endpoint,
    bearerConfigKey: endpoint.bearerConfigKey ?? endpoint.bearer_config_key,
    requestDefaults: endpoint.requestDefaults ?? endpoint.request_defaults ?? {},
    timeoutSeconds: endpoint.timeoutSeconds ?? endpoint.timeout_seconds,
  };
}

function prettyJson(value: unknown) {
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function formatTime(timestamp: number) {
  return new Intl.DateTimeFormat("zh-CN", { hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false }).format(timestamp);
}

function formatDateTime(timestamp: number) {
  return new Intl.DateTimeFormat("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", hour12: false }).format(timestamp);
}

function extractTrafficEvents(logs: PluginLogLine[]): PluginTrafficEvent[] {
  const marker = "DRPA_PLUGIN_EVENT";
  const events: PluginTrafficEvent[] = [];
  for (const line of logs) {
    if (line.event) {
      const envelope = line.event as Record<string, unknown>;
      const nested = envelope.event;
      const payload = nested && typeof nested === "object" && !Array.isArray(nested)
        ? nested as Record<string, unknown>
        : envelope;
      events.push({
        timestamp: line.timestamp,
        kind: String(payload.kind ?? "event"),
        requestId: String(payload.req_id ?? payload.requestId ?? ""),
        title: String(payload.title ?? ""),
        detail: payload.detail ?? payload,
      });
      continue;
    }
    const markerIndex = line.message.indexOf(marker);
    if (markerIndex < 0) continue;
    const source = line.message.slice(markerIndex + marker.length).replace(/^[\s:=|-]+/, "");
    try {
      const payload = JSON.parse(source) as Record<string, unknown>;
      events.push({
        timestamp: line.timestamp,
        kind: String(payload.kind ?? "event"),
        requestId: String(payload.req_id ?? payload.requestId ?? ""),
        title: String(payload.title ?? ""),
        detail: payload.detail ?? payload,
      });
    } catch {
      events.push({
        timestamp: line.timestamp,
        kind: "parse_error",
        requestId: "",
        title: "无法解析结构化插件事件",
        detail: source,
      });
    }
  }
  return events.reverse();
}
