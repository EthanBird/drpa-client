import { CheckCircle2, Database, Languages, MonitorCog, ShieldCheck } from "lucide-react";

import { useAppStore } from "../app/store";

export function SettingsPage() {
  const compactMode = useAppStore((state) => state.compactMode);
  const toggleCompactMode = useAppStore((state) => state.toggleCompactMode);

  return (
    <div className="page settings-page">
      <header className="page-header">
        <div><div className="eyebrow">应用配置</div><h1>设置</h1><p>DRPA 默认使用中文，并以离线、便携和可审计为原则。</p></div>
      </header>
      <div className="settings-grid">
        <section className="settings-card">
          <header><Languages size={18} /><div><h2>语言</h2><p>界面、日志摘要和内置模板使用的语言。</p></div></header>
          <div className="setting-row"><div><strong>显示语言</strong><span>简体中文（默认）</span></div><span className="status-badge success"><CheckCircle2 size={12} /> 已启用</span></div>
        </section>
        <section className="settings-card">
          <header><MonitorCog size={18} /><div><h2>界面</h2><p>调整信息密度，不影响脚本运行。</p></div></header>
          <div className="setting-row"><div><strong>紧凑布局</strong><span>在同一屏幕显示更多任务信息</span></div><button className={`switch ${compactMode ? "on" : ""}`} type="button" onClick={toggleCompactMode} aria-pressed={compactMode}><span /></button></div>
        </section>
        <section className="settings-card">
          <header><ShieldCheck size={18} /><div><h2>Windows 便携模式</h2><p>正式发行版不安装服务、不创建卸载项、不主动写注册表。</p></div></header>
          <div className="setting-row"><div><strong>零安装策略</strong><span>Portable ZIP + Fixed WebView2 Runtime</span></div><span className="status-badge neutral">发布门禁</span></div>
        </section>
        <section className="settings-card">
          <header><Database size={18} /><div><h2>工作区数据</h2><p>项目、脚本包、运行记录与产物统一保存在工作区。</p></div></header>
          <div className="setting-row"><div><strong>存储策略</strong><span>本地优先 · 不依赖云端</span></div><span className="status-badge success">正常</span></div>
        </section>
      </div>
    </div>
  );
}
