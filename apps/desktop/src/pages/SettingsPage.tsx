import { open } from "@tauri-apps/plugin-dialog";
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
} from "lucide-react";
import { lazy, Suspense, useEffect, useRef, useState } from "react";

import { useAppStore, type FontScale } from "../app/store";
import type { AgentWorkspaceConfig, PlatformCapabilities, WindowsUpdateSession, WindowsUpdateStatus } from "../domain/models";
import { desktopGateway } from "../infra/gateway";

const SkillWorkspace = lazy(() => import("../components/SkillWorkspace").then((module) => ({ default: module.SkillWorkspace })));

const phaseLabel: Record<WindowsUpdateStatus["phase"], string> = {
  verifying: "读取更新包",
  applying: "替换应用文件",
  waitingForRestart: "准备重启",
  restarting: "正在重启",
  completed: "更新完成",
  failed: "更新失败",
};

const fontScaleOptions: Array<{ id: FontScale; label: string; detail: string }> = [
  { id: "small", label: "小", detail: "90%" },
  { id: "standard", label: "标准", detail: "100%" },
  { id: "large", label: "大", detail: "110%" },
  { id: "extraLarge", label: "特大", detail: "120%" },
];

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
}

export function SettingsPage() {
  const theme = useAppStore((state) => state.theme);
  const fontScale = useAppStore((state) => state.fontScale);
  const agentBaseUrl = useAppStore((state) => state.agentBaseUrl);
  const agentModel = useAppStore((state) => state.agentModel);
  const agentApiKey = useAppStore((state) => state.agentApiKey);
  const agentStreamEnabled = useAppStore((state) => state.agentStreamEnabled);
  const agentContextWindow = useAppStore((state) => state.agentContextWindow);
  const agentMaxOutputTokens = useAppStore((state) => state.agentMaxOutputTokens);
  const agentMaxRounds = useAppStore((state) => state.agentMaxRounds);
  const agentTemperature = useAppStore((state) => state.agentTemperature);
  const setTheme = useAppStore((state) => state.setTheme);
  const setFontScale = useAppStore((state) => state.setFontScale);
  const setAgentBaseUrl = useAppStore((state) => state.setAgentBaseUrl);
  const setAgentModel = useAppStore((state) => state.setAgentModel);
  const setAgentApiKey = useAppStore((state) => state.setAgentApiKey);
  const setAgentStreamEnabled = useAppStore((state) => state.setAgentStreamEnabled);
  const setAgentContextWindow = useAppStore((state) => state.setAgentContextWindow);
  const setAgentMaxOutputTokens = useAppStore((state) => state.setAgentMaxOutputTokens);
  const setAgentMaxRounds = useAppStore((state) => state.setAgentMaxRounds);
  const setAgentTemperature = useAppStore((state) => state.setAgentTemperature);
  const [dataDirectory, setDataDirectory] = useState("正在读取…");
  const [platform, setPlatform] = useState<PlatformCapabilities | null>(null);
  const [workspaceNotice, setWorkspaceNotice] = useState("");
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

  return (
    <div className="page settings-page">
      <header className="page-header">
        <div><div className="eyebrow">应用配置</div><h1>设置</h1><p>只保留界面、更新、工作区与 AI Agent 的有效配置。</p></div>
      </header>
      <div className="settings-grid">
        <section className="settings-card">
          <header><Languages size={18} /><div><h2>语言</h2><p>界面、日志摘要和内置模板的显示语言。</p></div></header>
          <div className="setting-row"><div><strong>显示语言</strong><span>当前发行版完整支持简体中文</span></div><select className="settings-select" aria-label="显示语言" value="zh-CN" disabled><option value="zh-CN">简体中文</option></select></div>
        </section>

        <section className="settings-card">
          <header><Palette size={18} /><div><h2>外观</h2><p>切换主题，并按等级调整全局文字大小。</p></div></header>
          <div className="setting-row"><div><strong>界面主题</strong><span>立即应用并保存在本机</span></div><div className="theme-picker" role="radiogroup" aria-label="界面主题"><button type="button" className={theme === "light" ? "active" : ""} onClick={() => setTheme("light")} role="radio" aria-checked={theme === "light"}><Sun size={14} /> 亮色</button><button type="button" className={theme === "dark" ? "active" : ""} onClick={() => setTheme("dark")} role="radio" aria-checked={theme === "dark"}><Moon size={14} /> 暗色</button></div></div>
          <div className="setting-row setting-row-divider"><div><strong>字号等级</strong><span>仅缩放文字，不改变窗口和控件密度</span></div><div className="font-scale-picker" role="radiogroup" aria-label="字号等级">{fontScaleOptions.map((option) => <button type="button" role="radio" aria-checked={fontScale === option.id} className={fontScale === option.id ? "active" : ""} onClick={() => setFontScale(option.id)} key={option.id}><strong>{option.label}</strong><small>{option.detail}</small></button>)}</div></div>
        </section>

        {platform?.supportsWindowsUpdates && <section className="settings-card settings-card-wide">
          <header><Download size={18} /><div><h2>Windows 轻量热更新</h2><p>基于安装文件清单执行差量替换；运行依赖只在内容真正变化时进入更新包。</p></div></header>
          <div className="setting-row"><div><strong>本地更新包</strong><span>WebView2 与用户 data 始终受保护；更新由独立 Worker 应用并保留回滚现场。</span></div><button className="button secondary" type="button" onClick={() => void applyUpdate()} disabled={updating}><Download size={13} /> {updating ? "更新进行中" : "选择更新包"}</button></div>
        </section>}

        <section className="settings-card">
          <header><Database size={18} /><div><h2>工作区数据</h2><p>项目、脚本包、会话、知识文档与产物统一保存在本地。</p></div></header>
          <div className="setting-row data-directory-row"><div><strong>当前数据目录</strong><code>{dataDirectory}</code><span>{workspaceNotice || `${platform?.dataDirectoryPolicy ?? "本地数据目录"} · 应用升级不会覆盖此目录`}</span></div><button className="button secondary small" type="button" onClick={() => void openWorkspaceDirectory()}><FolderOpen size={13} /> 在{platform?.fileManagerName ?? "文件管理器"}中打开</button></div>
        </section>

        <section className="settings-card">
          <header><Bot size={18} /><div><h2>AI Agent 配置</h2><p>OpenAI 兼容连接由设置页统一管理。</p></div></header>
          <div className="agent-settings-form">
            <label><span><Link2 size={12} /> OpenAI 兼容 URL</span><input aria-label="设置 Agent URL" value={agentBaseUrl} onChange={(event) => setAgentBaseUrl(event.target.value)} placeholder="https://api.openai.com/v1" /></label>
            <label><span><Cpu size={12} /> Model</span><input aria-label="设置 Agent 模型" value={agentModel} onChange={(event) => setAgentModel(event.target.value)} placeholder="gpt-5.4-mini" /></label>
            <label><span><KeyRound size={12} /> API Key（可选）</span><input aria-label="设置 Agent API Key" type="password" autoComplete="off" value={agentApiKey} onChange={(event) => setAgentApiKey(event.target.value)} placeholder="本地兼容服务可留空" /><small>只保留到当前应用会话，不写入磁盘。</small></label>
            <label className="agent-settings-switch"><span>流式输出</span><button className={`switch ${agentStreamEnabled ? "on" : ""}`} type="button" role="switch" aria-label="设置 Agent 流式输出" aria-checked={agentStreamEnabled} onClick={() => setAgentStreamEnabled(!agentStreamEnabled)}><span /></button></label>
            <label><span>上下文窗口（tokens）</span><input aria-label="设置 Agent 上下文窗口" type="number" min={1024} max={2000000} step={1024} value={agentContextWindow} onChange={(event) => { if (Number.isFinite(event.currentTarget.valueAsNumber)) setAgentContextWindow(event.currentTarget.valueAsNumber); }} /></label>
            <label><span>最大输出（tokens）</span><input aria-label="设置 Agent 最大输出" type="number" min={64} max={131072} step={64} value={agentMaxOutputTokens} onChange={(event) => { if (Number.isFinite(event.currentTarget.valueAsNumber)) setAgentMaxOutputTokens(event.currentTarget.valueAsNumber); }} /></label>
            <label><span>最大模型/工具循环</span><input aria-label="设置 Agent 最大模型工具循环" type="number" min={1} max={256} step={1} value={agentMaxRounds} onChange={(event) => { if (Number.isFinite(event.currentTarget.valueAsNumber)) setAgentMaxRounds(event.currentTarget.valueAsNumber); }} /></label>
            <label><span>Temperature</span><input aria-label="设置 Agent Temperature" type="number" min={0} max={2} step={0.1} value={agentTemperature} onChange={(event) => { if (Number.isFinite(event.currentTarget.valueAsNumber)) setAgentTemperature(event.currentTarget.valueAsNumber); }} /></label>
          </div>
        </section>

        <section className="settings-card settings-card-wide agent-workspace-card">
          <header><Brain size={18} /><div><h2>Agent 指令与记忆</h2><p>AGENTS.md 保存始终生效的工作约定；MEMORY.md 保存短小、稳定的跨会话事实。</p></div><code>{agentWorkspace?.rootDirectory ?? "正在初始化…"}</code></header>
          <div className="agent-document-toolbar"><div role="tablist" aria-label="Agent 文档"><button type="button" role="tab" aria-selected={documentTab === "agents"} className={documentTab === "agents" ? "active" : ""} onClick={() => changeDocumentTab("agents")}><FileText size={13} /> AGENTS.md</button><button type="button" role="tab" aria-selected={documentTab === "memory"} className={documentTab === "memory" ? "active" : ""} onClick={() => changeDocumentTab("memory")}><Brain size={13} /> MEMORY.md</button></div><span>{agentWorkspaceError}</span><button className="button secondary small" type="button" onClick={() => void saveAgentDocument()}><Save size={13} /> 保存文档</button></div>
          <textarea className="agent-document-editor" aria-label={documentTab === "agents" ? "编辑 AGENTS.md" : "编辑 MEMORY.md"} value={documentDraft} onChange={(event) => setDocumentDraft(event.target.value)} spellCheck={false} />
        </section>

        <Suspense fallback={<section className="settings-card settings-card-wide skill-workspace-loading">正在加载 Skills 代码编辑器…</section>}><SkillWorkspace /></Suspense>
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
