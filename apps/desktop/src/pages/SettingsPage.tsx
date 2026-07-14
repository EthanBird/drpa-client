import { open } from "@tauri-apps/plugin-dialog";
import {
  CheckCircle2,
  Clipboard,
  Database,
  Download,
  Languages,
  Moon,
  Palette,
  ShieldCheck,
  Sun,
  Wrench,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { useAppStore } from "../app/store";
import type { WindowsUpdateSession, WindowsUpdateStatus } from "../domain/models";
import { desktopGateway } from "../infra/gateway";

const phaseLabel: Record<WindowsUpdateStatus["phase"], string> = {
  verifying: "校验更新包",
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

export function SettingsPage() {
  const theme = useAppStore((state) => state.theme);
  const setTheme = useAppStore((state) => state.setTheme);
  const [dataDirectory, setDataDirectory] = useState("正在读取…");
  const [updateSession, setUpdateSession] = useState<WindowsUpdateSession | null>(null);
  const [updateStatus, setUpdateStatus] = useState<WindowsUpdateStatus | null>(null);
  const [updateError, setUpdateError] = useState("");
  const [updating, setUpdating] = useState(false);
  const pollTimer = useRef<number | undefined>(undefined);
  const restartRequested = useRef(false);

  useEffect(() => {
    void desktopGateway.getDataDirectory().then(setDataDirectory);
    return () => window.clearTimeout(pollTimer.current);
  }, []);

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
    setUpdateStatus({
      sessionId: "pending",
      version: "",
      phase: "verifying",
      progress: 0,
      completedFiles: 0,
      totalFiles: 0,
      message: "正在读取更新包并校验 SHA-256",
    });
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
    setUpdateSession(null);
    setUpdateStatus(null);
    setUpdateError("");
  };

  return (
    <div className="page settings-page">
      <header className="page-header">
        <div><div className="eyebrow">应用配置</div><h1>设置</h1><p>管理界面主题、离线更新与工作区数据。</p></div>
      </header>
      <div className="settings-grid">
        <section className="settings-card">
          <header><Languages size={18} /><div><h2>语言</h2><p>界面、日志摘要和内置模板使用的语言。</p></div></header>
          <div className="setting-row"><div><strong>显示语言</strong><span>简体中文（默认）</span></div><span className="status-badge success"><CheckCircle2 size={12} /> 已启用</span></div>
        </section>

        <section className="settings-card">
          <header><Palette size={18} /><div><h2>主题</h2><p>亮色为默认主题，也可以切换为暗色。</p></div></header>
          <div className="setting-row">
            <div><strong>界面主题</strong><span>立即应用并保存在本机</span></div>
            <div className="theme-picker" role="radiogroup" aria-label="界面主题">
              <button type="button" className={theme === "light" ? "active" : ""} onClick={() => setTheme("light")} role="radio" aria-checked={theme === "light"}><Sun size={14} /> 亮色</button>
              <button type="button" className={theme === "dark" ? "active" : ""} onClick={() => setTheme("dark")} role="radio" aria-checked={theme === "dark"}><Moon size={14} /> 暗色</button>
            </div>
          </div>
        </section>

        <section className="settings-card settings-card-wide">
          <header><Download size={18} /><div><h2>Windows 轻量热更新</h2><p>基于安装文件清单执行校验、差量替换与删除；运行依赖仅在内容真正变化时进入差量包。</p></div></header>
          <div className="setting-row"><div><strong>本地更新包</strong><span>WebView2 与用户 data 始终受保护；浏览器/runtime 仅在摘要变化时进入差量，主程序只在最后一步短暂重启。</span></div><button className="button secondary" type="button" onClick={() => void applyUpdate()} disabled={updating}><Download size={13} /> {updating ? "更新进行中" : "选择更新包"}</button></div>
        </section>

        <section className="settings-card">
          <header><ShieldCheck size={18} /><div><h2>离线运行策略</h2><p>完整安装包提供固定运行依赖，日常更新只传输实际变化的文件。</p></div></header>
          <div className="setting-row"><div><strong>更新模式</strong><span>文件清单 + SHA-256 + 自动回滚</span></div><span className="status-badge success"><CheckCircle2 size={12} /> 已启用</span></div>
        </section>

        <section className="settings-card">
          <header><Database size={18} /><div><h2>工作区数据</h2><p>项目、脚本包、运行记录与产物统一保存在工作区。</p></div></header>
          <div className="setting-row data-directory-row"><div><strong>当前数据目录</strong><code>{dataDirectory}</code><span>热更新不会覆盖 data 目录。</span></div><button className="button ghost small" type="button" onClick={() => void navigator.clipboard.writeText(dataDirectory)}><Clipboard size={13} /> 复制</button></div>
        </section>

        <section className="settings-card settings-card-wide">
          <header><Wrench size={18} /><div><h2>交互完整性</h2><p>所有可见操作均连接真实桌面命令，并在失败时保留应用与现场信息。</p></div></header>
          <div className="interaction-audit">
            <span><CheckCircle2 size={13} /> 更新过程持续显示阶段、文件名与完成比例</span>
            <span><CheckCircle2 size={13} /> 更新失败自动回滚，错误直接显示在当前窗口</span>
            <span><CheckCircle2 size={13} /> Studio 支持文件拖入、内联新建、重命名与删除</span>
            <span><CheckCircle2 size={13} /> 安装文件清单可用于生成下一版本精确差量包</span>
          </div>
        </section>
      </div>

      {updateStatus && (
        <div className="update-progress-overlay" role="dialog" aria-modal="true" aria-label="Windows 更新进度">
          <section className={`update-progress-window ${updateStatus.phase === "failed" ? "failed" : ""}`}>
            <header>
              <div className="update-progress-icon"><Download size={20} /></div>
              <div><span>DRPA 文件级更新</span><h2>{phaseLabel[updateStatus.phase]}</h2></div>
              <strong>{Math.max(0, Math.min(100, updateStatus.progress))}%</strong>
            </header>
            <div className="update-progress-track"><span style={{ width: `${Math.max(2, updateStatus.progress)}%` }} /></div>
            <div className="update-progress-meta">
              <span>{updateStatus.completedFiles} / {updateStatus.totalFiles || updateSession?.totalFiles || 0} 个文件</span>
              <span>{updateSession ? formatBytes(updateSession.totalBytes) : "正在计算大小"}</span>
            </div>
            <p className="update-progress-message">{updateError || updateStatus.message}</p>
            {updateStatus.currentFile && <code className="update-current-file">{updateStatus.currentFile}</code>}
            {updateStatus.phase === "waitingForRestart" && <p className="update-restart-note">文件替换已完成。窗口即将短暂关闭，更新 Worker 会替换主程序并自动重新启动。</p>}
            {(updateStatus.phase === "failed" || updateStatus.phase === "completed") && <footer><button className="button secondary" type="button" onClick={closeUpdate}>关闭</button></footer>}
          </section>
        </div>
      )}
    </div>
  );
}
