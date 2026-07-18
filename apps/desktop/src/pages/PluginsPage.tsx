import { open } from "@tauri-apps/plugin-dialog";
import {
  Bot,
  Box,
  CircleAlert,
  CircleStop,
  Copy,
  Download,
  ExternalLink,
  FileCode2,
  LoaderCircle,
  Play,
  PlugZap,
  RefreshCw,
  Save,
  Server,
  TerminalSquare,
  Trash2,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { useAppStore } from "../app/store";
import type { PluginLogLine, PluginSummary } from "../domain/models";
import { desktopGateway } from "../infra/gateway";

const statusLabels: Record<PluginSummary["status"], string> = {
  disabled: "未启用",
  stopped: "已停止",
  running: "运行中",
  error: "异常退出",
};

function formatTime(timestamp: number) {
  return new Intl.DateTimeFormat("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(timestamp);
}

export function PluginsPage() {
  const setActiveNavigation = useAppStore((state) => state.setActiveNavigation);
  const setAgentBaseUrl = useAppStore((state) => state.setAgentBaseUrl);
  const setAgentModel = useAppStore((state) => state.setAgentModel);
  const [plugins, setPlugins] = useState<PluginSummary[]>([]);
  const [selectedId, setSelectedId] = useState("");
  const [configDraft, setConfigDraft] = useState<Record<string, unknown>>({});
  const [autostart, setAutostart] = useState(false);
  const [logs, setLogs] = useState<PluginLogLine[]>([]);
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const [pendingUninstall, setPendingUninstall] = useState("");

  const selected = useMemo(
    () => plugins.find((plugin) => plugin.id === selectedId) ?? plugins[0],
    [plugins, selectedId],
  );

  const reload = async (preferred?: string) => {
    const next = await desktopGateway.listPlugins();
    setPlugins(next);
    const id = preferred && next.some((plugin) => plugin.id === preferred)
      ? preferred
      : selectedId && next.some((plugin) => plugin.id === selectedId)
        ? selectedId
        : next[0]?.id ?? "";
    setSelectedId(id);
    const plugin = next.find((item) => item.id === id);
    if (plugin) {
      setConfigDraft(structuredClone(plugin.config));
      setAutostart(plugin.autostart);
      setLogs(await desktopGateway.getPluginLogs(plugin.id));
    }
  };

  useEffect(() => {
    void reload().catch((error: unknown) => setNotice(String(error)));
    const timer = window.setInterval(() => {
      void desktopGateway.listPlugins().then((next) => {
        setPlugins(next);
        const current = next.find((plugin) => plugin.id === selectedId) ?? next[0];
        if (current) void desktopGateway.getPluginLogs(current.id).then(setLogs);
      }).catch(() => undefined);
    }, 1500);
    return () => window.clearInterval(timer);
  }, [selectedId]);

  const selectPlugin = async (plugin: PluginSummary) => {
    setSelectedId(plugin.id);
    setConfigDraft(structuredClone(plugin.config));
    setAutostart(plugin.autostart);
    setLogs(await desktopGateway.getPluginLogs(plugin.id));
    setNotice("");
  };

  const runOperation = async (operation: () => Promise<void>, success: string) => {
    setBusy(true);
    setNotice("");
    try {
      await operation();
      await reload(selected?.id);
      setNotice(success);
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy(false);
    }
  };

  const install = async () => {
    const selectedPath = await open({
      multiple: false,
      filters: [{ name: "DRPA Plugin", extensions: ["drpa-plugin"] }],
    });
    if (!selectedPath) return;
    setBusy(true);
    try {
      const plugin = await desktopGateway.installPlugin(selectedPath);
      await reload(plugin.id);
      setNotice(`插件已安装：${plugin.name}`);
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy(false);
    }
  };

  const save = () => selected && runOperation(
    () => desktopGateway.savePluginConfig(selected.id, configDraft, autostart),
    "插件配置已保存；运行中的服务需要重启后应用。",
  );

  const useAsProvider = () => {
    if (!selected?.endpoint) return;
    setAgentBaseUrl(selected.endpoint);
    setAgentModel(String(configDraft.model || selected.name));
    setActiveNavigation("agent");
  };

  const updateConfig = (key: string, value: unknown) => {
    setConfigDraft((current) => ({ ...current, [key]: value }));
  };

  return (
    <div className="page plugins-page">
      <header className="page-header plugins-header">
        <div>
          <div className="eyebrow">Agent 扩展基础设施</div>
          <h1>插件</h1>
          <p>管理离线服务、模型适配器和工具提供者。插件进程由 DRPA 启停、记录日志并隐藏控制台窗口。</p>
        </div>
        <div className="page-actions">
          <button className="button secondary" type="button" onClick={() => void reload(selected?.id)} disabled={busy}><RefreshCw size={13} /> 刷新</button>
          <button className="button primary" type="button" onClick={() => void install()} disabled={busy}><Download size={13} /> 安装本地插件</button>
        </div>
      </header>

      <div className="plugins-layout">
        <aside className="plugin-catalog">
          <header><PlugZap size={15} /><strong>已安装</strong><span>{plugins.length}</span></header>
          <div className="plugin-catalog-list">
            {plugins.map((plugin) => (
              <button className={plugin.id === selected?.id ? "active" : ""} type="button" onClick={() => void selectPlugin(plugin)} key={plugin.id}>
                <span className={`plugin-status-dot ${plugin.status}`} />
                <div><strong>{plugin.name}</strong><small>{plugin.description}</small><em>{plugin.types.join(" · ")}</em></div>
                <span>{plugin.version}</span>
              </button>
            ))}
            {plugins.length === 0 && <div className="plugin-empty"><Box size={24} /><span>暂无插件</span></div>}
          </div>
        </aside>

        {selected ? <main className="plugin-detail">
          <section className="plugin-detail-hero">
            <div className="plugin-logo"><Server size={22} /></div>
            <div><div><h2>{selected.name}</h2><span className={`plugin-status ${selected.status}`}>{statusLabels[selected.status]}</span></div><p>{selected.description}</p><code>{selected.id} · {selected.version}</code></div>
            <div className="plugin-runtime-actions">
              {selected.status === "running"
                ? <button className="button secondary" type="button" onClick={() => void runOperation(() => desktopGateway.stopPlugin(selected.id), "插件服务已停止。") } disabled={busy}><CircleStop size={13} /> 停止</button>
                : <button className="button primary" type="button" onClick={() => void runOperation(() => desktopGateway.startPlugin(selected.id), "插件服务已启动。") } disabled={busy}><Play size={13} /> 启动</button>}
              <button className="button secondary" type="button" onClick={() => void runOperation(() => desktopGateway.setPluginEnabled(selected.id, !selected.enabled), selected.enabled ? "插件已禁用。" : "插件已启用。") } disabled={busy}>{selected.enabled ? "禁用" : "启用"}</button>
            </div>
          </section>

          {selected.lastError && <div className="plugin-error"><CircleAlert size={15} /><span>{selected.lastError}</span></div>}
          {notice && <div className="plugin-notice">{busy && <LoaderCircle className="spin" size={13} />}{notice}</div>}

          <div className="plugin-detail-grid">
            <section className="plugin-panel plugin-config-panel">
              <header><FileCode2 size={15} /><div><h3>配置</h3><p>字段来自插件的 config.schema.json。</p></div><button className="button secondary small" type="button" onClick={() => void save()} disabled={busy}><Save size={12} /> 保存</button></header>
              <div className="plugin-config-fields">
                {Object.keys(configDraft).map((key) => {
                  const descriptor = selected.configSchema.properties?.[key] ?? {};
                  const value = configDraft[key];
                  if (descriptor.type === "boolean" || typeof value === "boolean") {
                    return <label className="plugin-boolean-field" key={key}><span><strong>{descriptor.title ?? key}</strong><small>{descriptor.description}</small></span><button className={`switch ${value ? "on" : ""}`} type="button" role="switch" aria-label={descriptor.title ?? key} aria-checked={Boolean(value)} onClick={() => updateConfig(key, !value)}><span /></button></label>;
                  }
                  if (descriptor.enum) {
                    return <label key={key}><span>{descriptor.title ?? key}</span><select value={String(value ?? "")} onChange={(event) => updateConfig(key, event.target.value)}>{descriptor.enum.map((option) => <option value={option} key={option}>{option}</option>)}</select><small>{descriptor.description}</small></label>;
                  }
                  const numeric = descriptor.type === "integer" || descriptor.type === "number" || typeof value === "number";
                  return <label key={key}><span>{descriptor.title ?? key}</span><input type={descriptor.secret ? "password" : numeric ? "number" : "text"} value={String(value ?? "")} min={descriptor.minimum} max={descriptor.maximum} onChange={(event) => updateConfig(key, numeric ? Number(event.target.value) : event.target.value)} /><small>{descriptor.description}</small></label>;
                })}
                <label className="plugin-boolean-field"><span><strong>随 DRPA 启动</strong><small>运行环境就绪后自动启动此服务。</small></span><button className={`switch ${autostart ? "on" : ""}`} type="button" role="switch" aria-label="插件自动启动" aria-checked={autostart} onClick={() => setAutostart(!autostart)}><span /></button></label>
              </div>
            </section>

            <section className="plugin-panel plugin-provider-panel">
              <header><Bot size={15} /><div><h3>Provider 与工具</h3><p>{selected.toolCount} 个 Tool · {selected.types.join(" · ")}</p></div></header>
              <label><span>OpenAI 兼容 Endpoint</span><div><code>{selected.endpoint || "未导出 Endpoint"}</code><button type="button" title="复制 Endpoint" onClick={() => void navigator.clipboard.writeText(selected.endpoint)} disabled={!selected.endpoint}><Copy size={12} /></button></div></label>
              <button className="button primary" type="button" onClick={useAsProvider} disabled={!selected.endpoint || selected.status !== "running"}><ExternalLink size={13} /> 用作 AI Agent Provider</button>
              <small>工具桥会把 Dify 的结构化决策转换为 OpenAI `tool_calls`，再交给 DRPA ToolRegistry 执行。</small>
            </section>

            <section className="plugin-panel plugin-log-panel">
              <header><TerminalSquare size={15} /><div><h3>服务日志</h3><p>最多保留最近 1000 行，异常字节按容错 UTF-8 解码。</p></div></header>
              <div className="plugin-log-view">
                {logs.map((line, index) => <div className={line.stream} key={`${line.timestamp}-${index}`}><time>{formatTime(line.timestamp)}</time><span>{line.stream}</span><pre>{line.message}</pre></div>)}
                {logs.length === 0 && <p>服务尚未产生日志。</p>}
              </div>
            </section>
          </div>

          <footer className="plugin-danger-zone">
            <div><Trash2 size={14} /><span><strong>卸载插件</strong><small>停止服务并删除插件程序、清单和本地配置。</small></span></div>
            <button className="button ghost small danger-text" type="button" onClick={() => setPendingUninstall(selected.id)}>卸载</button>
          </footer>
        </main> : <div className="empty-state"><PlugZap size={28} /><h2>选择一个插件</h2></div>}
      </div>

      {pendingUninstall && <div className="knowledge-confirm-overlay" role="dialog" aria-modal="true" aria-label="确认卸载插件"><section className="knowledge-confirm"><div className="knowledge-confirm-icon"><CircleAlert size={18} /></div><div><h2>卸载插件？</h2><p>“{pendingUninstall}”的服务和本地配置将被删除。</p></div><footer><button className="button secondary" type="button" onClick={() => setPendingUninstall("")}>取消</button><button className="button danger" type="button" onClick={() => void runOperation(async () => { await desktopGateway.uninstallPlugin(pendingUninstall); setPendingUninstall(""); setSelectedId(""); }, "插件已卸载。")}>确认卸载</button></footer></section></div>}
    </div>
  );
}
