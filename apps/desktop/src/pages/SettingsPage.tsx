import { confirm, open, save as saveDialog } from "@tauri-apps/plugin-dialog";
import {
  Bot,
  Brain,
  CheckCircle2,
  Cpu,
  Database,
  Download,
  FileText,
  FolderOpen,
  KeyRound,
  Languages,
  Link2,
  Moon,
  Palette,
  Save,
  Sun,
  Upload,
} from "lucide-react";
import { lazy, Suspense, useEffect, useRef, useState } from "react";

import { useAppStore } from "../app/store";
import type { AgentWorkspaceConfig, PlatformCapabilities, WindowsUpdateSession, WindowsUpdateStatus } from "../domain/models";
import { desktopGateway } from "../infra/gateway";
import { useI18n } from "../i18n";

const SkillWorkspace = lazy(() => import("../components/SkillWorkspace").then((module) => ({ default: module.SkillWorkspace })));

const phaseLabel: Record<WindowsUpdateStatus["phase"], string> = {
  verifying: "读取更新包",
  applying: "替换应用文件",
  waitingForRestart: "准备重启",
  restarting: "正在重启",
  completed: "更新完成",
  failed: "更新失败",
};

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
}

function ensureUserDataExtension(path: string) {
  return path.toLowerCase().endsWith(".drpa-data") ? path : `${path}.drpa-data`;
}

export function SettingsPage() {
  const { language, t } = useI18n();
  const theme = useAppStore((state) => state.theme);
  const fontScale = useAppStore((state) => state.fontScale);
  const hidePageHeaders = useAppStore((state) => state.hidePageHeaders);
  const agentBaseUrl = useAppStore((state) => state.agentBaseUrl);
  const agentModel = useAppStore((state) => state.agentModel);
  const agentApiKey = useAppStore((state) => state.agentApiKey);
  const agentStreamEnabled = useAppStore((state) => state.agentStreamEnabled);
  const agentContextWindow = useAppStore((state) => state.agentContextWindow);
  const agentMaxOutputTokens = useAppStore((state) => state.agentMaxOutputTokens);
  const agentMaxRounds = useAppStore((state) => state.agentMaxRounds);
  const agentTemperature = useAppStore((state) => state.agentTemperature);
  const agentPythonTimeoutSeconds = useAppStore((state) => state.agentPythonTimeoutSeconds);
  const setTheme = useAppStore((state) => state.setTheme);
  const setFontScale = useAppStore((state) => state.setFontScale);
  const setLanguage = useAppStore((state) => state.setLanguage);
  const setHidePageHeaders = useAppStore((state) => state.setHidePageHeaders);
  const setAgentBaseUrl = useAppStore((state) => state.setAgentBaseUrl);
  const setAgentModel = useAppStore((state) => state.setAgentModel);
  const setAgentApiKey = useAppStore((state) => state.setAgentApiKey);
  const setAgentStreamEnabled = useAppStore((state) => state.setAgentStreamEnabled);
  const setAgentContextWindow = useAppStore((state) => state.setAgentContextWindow);
  const setAgentMaxOutputTokens = useAppStore((state) => state.setAgentMaxOutputTokens);
  const setAgentMaxRounds = useAppStore((state) => state.setAgentMaxRounds);
  const setAgentTemperature = useAppStore((state) => state.setAgentTemperature);
  const setAgentPythonTimeoutSeconds = useAppStore((state) => state.setAgentPythonTimeoutSeconds);
  const [dataDirectory, setDataDirectory] = useState("正在读取…");
  const [platform, setPlatform] = useState<PlatformCapabilities | null>(null);
  const [workspaceNotice, setWorkspaceNotice] = useState("");
  const [transferBusy, setTransferBusy] = useState(false);
  const [agentWorkspace, setAgentWorkspace] = useState<AgentWorkspaceConfig | null>(null);
  const [agentWorkspaceError, setAgentWorkspaceError] = useState("");
  const [documentTab, setDocumentTab] = useState<"agents" | "memory">("agents");
  const [documentDraft, setDocumentDraft] = useState("");
  const [updateSession, setUpdateSession] = useState<WindowsUpdateSession | null>(null);
  const [updateStatus, setUpdateStatus] = useState<WindowsUpdateStatus | null>(null);
  const [updateError, setUpdateError] = useState("");
  const [updating, setUpdating] = useState(false);
  const pollTimer = useRef<number | undefined>(undefined);
  const restartRequested = useRef(false);

  useEffect(() => {
    void desktopGateway.getDataDirectory().then(setDataDirectory);
    void desktopGateway.getPlatformCapabilities().then((capabilities) => {
      setPlatform(capabilities);
      if (capabilities.supportsWindowsUpdates) {
        void desktopGateway.getLatestWindowsUpdateStatus().then((status) => {
          if (status?.phase === "failed" && localStorage.getItem("drpa.dismissedUpdateSession") !== status.sessionId) {
            setUpdateStatus(status);
            setUpdateError(status.message);
          }
        });
      }
    });
    void desktopGateway.getAgentWorkspaceConfig().then((config) => {
      setAgentWorkspace(config);
      setDocumentDraft(config.agentsMarkdown);
    }).catch((error: unknown) => setAgentWorkspaceError(String(error)));
    return () => window.clearTimeout(pollTimer.current);
  }, []);

  const changeDocumentTab = (tab: "agents" | "memory") => {
    setDocumentTab(tab);
    if (agentWorkspace) setDocumentDraft(tab === "agents" ? agentWorkspace.agentsMarkdown : agentWorkspace.memoryMarkdown);
  };

  const saveAgentDocument = async () => {
    try {
      await desktopGateway.writeAgentWorkspaceDocument(documentTab, documentDraft);
      setAgentWorkspace((current) => current ? {
        ...current,
        agentsMarkdown: documentTab === "agents" ? documentDraft : current.agentsMarkdown,
        memoryMarkdown: documentTab === "memory" ? documentDraft : current.memoryMarkdown,
      } : current);
      setAgentWorkspaceError(documentTab === "agents" ? "AGENTS.md 已保存" : "MEMORY.md 已保存");
    } catch (error) {
      setAgentWorkspaceError(String(error));
    }
  };

  const monitorUpdate = (session: WindowsUpdateSession) => {
    const poll = async () => {
      try {
        const status = await desktopGateway.getWindowsUpdateStatus(session.id);
        setUpdateStatus(status);
        if (status.phase === "failed") {
          setUpdating(false);
          setUpdateError(status.message);
          return;
        }
        if (status.phase === "completed") {
          setUpdating(false);
          return;
        }
        if (status.phase === "waitingForRestart" && !restartRequested.current) {
          restartRequested.current = true;
          pollTimer.current = window.setTimeout(() => {
            void desktopGateway.restartForWindowsUpdate(session.id).catch((error: unknown) => {
              setUpdating(false);
              setUpdateError(String(error));
              setUpdateStatus((current) => current ? { ...current, phase: "failed", message: String(error) } : null);
            });
          }, 900);
          return;
        }
        pollTimer.current = window.setTimeout(poll, 280);
      } catch {
        pollTimer.current = window.setTimeout(poll, 380);
      }
    };
    void poll();
  };

  const applyUpdate = async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: "DRPA Windows 文件级更新", extensions: ["drpa-update"] }],
    });
    if (!selected) return;
    setUpdating(true);
    setUpdateError("");
    restartRequested.current = false;
    setUpdateStatus({ sessionId: "pending", version: "", phase: "verifying", progress: 0, completedFiles: 0, totalFiles: 0, message: "正在读取更新清单与文件结构" });
    try {
      const session = await desktopGateway.applyWindowsUpdate(selected);
      setUpdateSession(session);
      monitorUpdate(session);
    } catch (error) {
      setUpdating(false);
      setUpdateError(String(error));
      setUpdateStatus((status) => status ? { ...status, phase: "failed", message: String(error) } : null);
    }
  };

  const closeUpdate = () => {
    if (updateStatus?.sessionId) localStorage.setItem("drpa.dismissedUpdateSession", updateStatus.sessionId);
    setUpdateSession(null);
    setUpdateStatus(null);
    setUpdateError("");
  };

  const openWorkspaceDirectory = async () => {
    try {
      await desktopGateway.openWorkspaceDataDirectory();
      setWorkspaceNotice(`已在${platform?.fileManagerName ?? "文件管理器"}中打开工作区`);
    } catch (error) {
      setWorkspaceNotice(String(error));
    }
  };

  const exportUserData = async () => {
    if (!("__TAURI_INTERNALS__" in window) || transferBusy) return;
    setTransferBusy(true);
    setWorkspaceNotice("正在打开保存窗口…");
    try {
      const selected = await saveDialog({
        defaultPath: `drpa-user-data-${new Date().toISOString().slice(0, 10)}.drpa-data`,
        filters: [{ name: "DRPA 用户数据", extensions: ["drpa-data"] }],
      });
      if (!selected) {
        setWorkspaceNotice("已取消导出用户数据");
        return;
      }
      const target = ensureUserDataExtension(selected);
      setWorkspaceNotice("正在导出当前工作区用户数据…");
      const result = await desktopGateway.exportUserData(target);
      setWorkspaceNotice(`已导出 ${result.fileCount} 个文件（${formatBytes(result.totalBytes)}）到 ${result.path}`);
    } catch (error) {
      setWorkspaceNotice(`导出失败：${String(error)}`);
    } finally {
      setTransferBusy(false);
    }
  };

  const importUserData = async () => {
    if (!("__TAURI_INTERNALS__" in window) || transferBusy) return;
    setTransferBusy(true);
    setWorkspaceNotice("正在打开导入窗口…");
    try {
      const source = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "DRPA 用户数据", extensions: ["drpa-data", "zip"] }],
      });
      if (!source) {
        setWorkspaceNotice("已取消导入用户数据");
        return;
      }
      const approved = await confirm(
        "导入会创建一个新的隔离工作区、切换到该工作区并重启应用；当前工作区不会被覆盖。是否继续？",
        { title: "导入用户数据", kind: "warning" },
      );
      if (!approved) {
        setWorkspaceNotice("已取消导入用户数据");
        return;
      }
      setWorkspaceNotice("正在校验并导入用户数据…");
      const result = await desktopGateway.importUserData(source);
      setWorkspaceNotice(`已导入 ${result.fileCount} 个文件到“${result.workspaceName}”，正在重启…`);
    } catch (error) {
      setWorkspaceNotice(`导入失败：${String(error)}`);
    } finally {
      setTransferBusy(false);
    }
  };

  return (
    <div className="page settings-page">
      <header className="page-header">
        <div><div className="eyebrow">{t("settings.eyebrow")}</div><h1>{t("settings.title")}</h1><p>{t("settings.description")}</p></div>
      </header>
      <div className="settings-grid">
        <section className="settings-card">
          <header><Languages size={18} /><div><h2>{t("settings.language")}</h2><p>{t("settings.languageDescription")}</p></div></header>
          <div className="setting-row"><div><strong>{t("settings.displayLanguage")}</strong><span>{t("settings.languageReady")}</span></div><select className="settings-select" aria-label={t("settings.displayLanguage")} value={language} onChange={(event) => setLanguage(event.target.value as "zh-CN" | "en-US")}><option value="zh-CN">简体中文</option><option value="en-US">English</option></select></div>
        </section>

        <section className="settings-card">
          <header><Palette size={18} /><div><h2>{t("settings.appearance")}</h2><p>{t("settings.appearanceDescription")}</p></div></header>
          <div className="setting-row"><div><strong>{t("settings.theme")}</strong><span>{t("settings.themeDescription")}</span></div><div className="theme-picker" role="radiogroup" aria-label={t("settings.theme")}><button type="button" className={theme === "light" ? "active" : ""} onClick={() => setTheme("light")} role="radio" aria-checked={theme === "light"}><Sun size={14} /> {t("settings.light")}</button><button type="button" className={theme === "dark" ? "active" : ""} onClick={() => setTheme("dark")} role="radio" aria-checked={theme === "dark"}><Moon size={14} /> {t("settings.dark")}</button></div></div>
          <div className="setting-row setting-row-divider"><div><strong>{t("settings.fontScale")}</strong><span>{t("settings.fontScaleDescription")}</span></div><div className="font-scale-slider"><input type="range" min={75} max={200} step={5} value={fontScale} aria-label={t("settings.fontScale")} onChange={(event) => setFontScale(event.currentTarget.valueAsNumber)} /><span className="font-scale-value">{fontScale}%</span><button className="button ghost small" type="button" onClick={() => setFontScale(100)}>{t("settings.fontReset")}</button></div></div>
          <div className="setting-row setting-row-divider"><div><strong>{t("settings.hideHeaders")}</strong><span>{t("settings.hideHeadersDescription")}</span></div><button className={`switch ${hidePageHeaders ? "on" : ""}`} type="button" role="switch" aria-label={t("settings.hideHeaders")} aria-checked={hidePageHeaders} onClick={() => setHidePageHeaders(!hidePageHeaders)}><span /></button></div>
        </section>

        {platform?.supportsWindowsUpdates && <section className="settings-card settings-card-wide">
          <header><Download size={18} /><div><h2>{t("settings.windowsUpdate")}</h2><p>{t("settings.windowsUpdateDescription")}</p></div></header>
          <div className="setting-row"><div><strong>{t("settings.localUpdate")}</strong><span>{t("settings.localUpdateDescription")}</span></div><button className="button secondary" type="button" onClick={() => void applyUpdate()} disabled={updating}><Download size={13} /> {updating ? t("settings.updating") : t("settings.selectUpdate")}</button></div>
        </section>}

        <section className="settings-card">
          <header><Database size={18} /><div><h2>{t("settings.workspaceData")}</h2><p>{t("settings.workspaceDataDescription")}</p></div></header>
          <div className="setting-row data-directory-row"><div><strong>{t("settings.currentDataDirectory")}</strong><code>{dataDirectory}</code><span role="status" aria-live="polite">{workspaceNotice || `${platform?.dataDirectoryPolicy ?? "本地数据目录"} · 应用升级不会覆盖此目录`}</span></div><button className="button secondary small" type="button" onClick={() => void openWorkspaceDirectory()}><FolderOpen size={13} /> {language === "en-US" ? t("settings.openDirectory") : `在${platform?.fileManagerName ?? "文件管理器"}中打开`}</button></div>
          <div className="setting-row setting-row-divider"><div><strong>{t("settings.migrateWorkspace")}</strong><span>{t("settings.migrateWorkspaceDescription")}</span></div><div className="settings-transfer-actions"><button className="button secondary small" type="button" onClick={() => void exportUserData()} disabled={transferBusy || !("__TAURI_INTERNALS__" in window)}><Download size={13} /> {t("settings.exportData")}</button><button className="button secondary small" type="button" onClick={() => void importUserData()} disabled={transferBusy || !("__TAURI_INTERNALS__" in window)}><Upload size={13} /> {t("settings.importData")}</button></div></div>
        </section>

        <section className="settings-card">
          <header><Bot size={18} /><div><h2>{t("settings.agentConfig")}</h2><p>{t("settings.agentConfigDescription")}</p></div></header>
          <div className="agent-settings-form">
            <label><span><Link2 size={12} /> OpenAI 兼容 URL</span><input aria-label="设置 Agent URL" value={agentBaseUrl} onChange={(event) => setAgentBaseUrl(event.target.value)} placeholder="http://127.0.0.1/v1" /></label>
            <label><span><Cpu size={12} /> Model</span><input aria-label="设置 Agent 模型" value={agentModel} onChange={(event) => setAgentModel(event.target.value)} placeholder="deepseek-v4-flash" /></label>
            <label><span><KeyRound size={12} /> {t("settings.optionalApiKey")}</span><input aria-label="设置 Agent API Key" type="password" autoComplete="off" value={agentApiKey} onChange={(event) => setAgentApiKey(event.target.value)} placeholder="本地兼容服务可留空" /><small>{t("settings.apiKeySessionOnly")}</small></label>
            <label className="agent-settings-switch"><span>{t("settings.streaming")}</span><button className={`switch ${agentStreamEnabled ? "on" : ""}`} type="button" role="switch" aria-label="设置 Agent 流式输出" aria-checked={agentStreamEnabled} onClick={() => setAgentStreamEnabled(!agentStreamEnabled)}><span /></button></label>
            <label><span>{t("settings.contextWindow")}</span><input aria-label="设置 Agent 上下文窗口" type="number" min={1024} max={2000000} step={1024} value={agentContextWindow} onChange={(event) => { if (Number.isFinite(event.currentTarget.valueAsNumber)) setAgentContextWindow(event.currentTarget.valueAsNumber); }} /></label>
            <label><span>{t("settings.maxOutput")}</span><input aria-label="设置 Agent 最大输出" type="number" min={64} max={131072} step={64} value={agentMaxOutputTokens} onChange={(event) => { if (Number.isFinite(event.currentTarget.valueAsNumber)) setAgentMaxOutputTokens(event.currentTarget.valueAsNumber); }} /></label>
            <label><span>{t("settings.maxRounds")}</span><input aria-label="设置 Agent 最大模型工具循环" type="number" min={1} max={256} step={1} value={agentMaxRounds} onChange={(event) => { if (Number.isFinite(event.currentTarget.valueAsNumber)) setAgentMaxRounds(event.currentTarget.valueAsNumber); }} /></label>
            <label><span>{t("settings.pythonTimeout")}</span><input aria-label="设置 Agent Python 超时" type="number" min={1} max={86400} step={30} value={agentPythonTimeoutSeconds} onChange={(event) => { if (Number.isFinite(event.currentTarget.valueAsNumber)) setAgentPythonTimeoutSeconds(event.currentTarget.valueAsNumber); }} /><small>{t("settings.pythonTimeoutDescription")}</small></label>
            <label><span>Temperature</span><input aria-label="设置 Agent Temperature" type="number" min={0} max={2} step={0.1} value={agentTemperature} onChange={(event) => { if (Number.isFinite(event.currentTarget.valueAsNumber)) setAgentTemperature(event.currentTarget.valueAsNumber); }} /></label>
          </div>
        </section>

        <section className="settings-card settings-card-wide agent-workspace-card">
          <header><Brain size={18} /><div><h2>{t("settings.agentInstructions")}</h2><p>{t("settings.agentInstructionsDescription")}</p></div><code>{agentWorkspace?.rootDirectory ?? "正在初始化…"}</code></header>
          <div className="agent-document-toolbar"><div role="tablist" aria-label="Agent 文档"><button type="button" role="tab" aria-selected={documentTab === "agents"} className={documentTab === "agents" ? "active" : ""} onClick={() => changeDocumentTab("agents")}><FileText size={13} /> AGENTS.md</button><button type="button" role="tab" aria-selected={documentTab === "memory"} className={documentTab === "memory" ? "active" : ""} onClick={() => changeDocumentTab("memory")}><Brain size={13} /> MEMORY.md</button></div><span>{agentWorkspaceError}</span><button className="button secondary small" type="button" onClick={() => void saveAgentDocument()}><Save size={13} /> {t("settings.saveDocument")}</button></div>
          <textarea className="agent-document-editor" aria-label={documentTab === "agents" ? "编辑 AGENTS.md" : "编辑 MEMORY.md"} value={documentDraft} onChange={(event) => setDocumentDraft(event.target.value)} spellCheck={false} />
        </section>

        <Suspense fallback={<section className="settings-card settings-card-wide skill-workspace-loading">{t("settings.loadingSkills")}</section>}><SkillWorkspace /></Suspense>
      </div>

      {updateStatus && (
        <div className="update-progress-overlay" role="dialog" aria-modal="true" aria-label="Windows 更新进度">
          <section className={`update-progress-window ${updateStatus.phase === "failed" ? "failed" : ""}`}>
            <header><div className="update-progress-icon"><Download size={20} /></div><div><span>DRPA 文件级更新</span><h2>{phaseLabel[updateStatus.phase]}</h2></div><strong>{Math.max(0, Math.min(100, updateStatus.progress))}%</strong></header>
            <div className="update-progress-track"><span style={{ width: `${Math.max(2, updateStatus.progress)}%` }} /></div>
            <div className="update-progress-meta"><span>{updateStatus.completedFiles} / {updateStatus.totalFiles || updateSession?.totalFiles || 0} 个文件</span><span>{updateSession ? formatBytes(updateSession.totalBytes) : "正在计算大小"}</span></div>
            <p className="update-progress-message">{updateError || updateStatus.message}</p>
            {updateStatus.currentFile && <code className="update-current-file">{updateStatus.currentFile}</code>}
            {updateStatus.phase === "waitingForRestart" && <p className="update-restart-note">更新 Worker 已独立就绪。窗口即将短暂关闭；备份会保留到新版本确认主窗口启动成功。</p>}
            {(updateStatus.phase === "failed" || updateStatus.phase === "completed") && <footer><button className="button secondary" type="button" onClick={closeUpdate}>关闭</button></footer>}
          </section>
        </div>
      )}
    </div>
  );
}
