import { open } from "@tauri-apps/plugin-dialog";
import { CheckCircle2, Clipboard, Database, Download, Languages, MonitorCog, ShieldCheck, Wrench } from "lucide-react";
import { useEffect, useState } from "react";

import { useAppStore } from "../app/store";
import { desktopGateway } from "../infra/gateway";

export function SettingsPage() {
  const compactMode = useAppStore((state) => state.compactMode);
  const toggleCompactMode = useAppStore((state) => state.toggleCompactMode);
  const [dataDirectory, setDataDirectory] = useState("正在读取…");
  const [updateNotice, setUpdateNotice] = useState("选择离线 `.drpa-update` 文件；校验通过后仅替换清单列出的应用文件。");
  const [updating, setUpdating] = useState(false);

  useEffect(() => { void desktopGateway.getDataDirectory().then(setDataDirectory); }, []);

  const applyUpdate = async () => {
    setUpdating(true);
    try {
      const selected = await open({ multiple: false, filters: [{ name: "DRPA Windows 文件级更新", extensions: ["drpa-update"] }] });
      if (!selected) return;
      setUpdateNotice("正在校验文件清单与 SHA-256…");
      setUpdateNotice(await desktopGateway.applyWindowsUpdate(selected));
    } catch (error) { setUpdateNotice(`更新失败：${String(error)}`); }
    finally { setUpdating(false); }
  };

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
          <header><Download size={18} /><div><h2>Windows 文件级热更新</h2><p>无需重新运行安装器，不覆盖 data，也不写注册表。</p></div></header>
          <div className="setting-row"><div><strong>本地更新包</strong><span>{updateNotice}</span></div><button className="button secondary" type="button" onClick={applyUpdate} disabled={updating}><Download size={13} /> {updating ? "校验中…" : "安装更新"}</button></div>
        </section>
        <section className="settings-card">
          <header><MonitorCog size={18} /><div><h2>界面</h2><p>调整信息密度，不影响脚本运行。</p></div></header>
          <div className="setting-row"><div><strong>紧凑布局</strong><span>在同一屏幕显示更多任务信息</span></div><button className={`switch ${compactMode ? "on" : ""}`} type="button" onClick={toggleCompactMode} aria-pressed={compactMode}><span /></button></div>
        </section>
        <section className="settings-card">
          <header><ShieldCheck size={18} /><div><h2>Windows 无注册表安装</h2><p>图形安装向导只释放文件并创建快捷方式，不创建注册表项。</p></div></header>
          <div className="setting-row"><div><strong>安装策略</strong><span>引导安装器 + Fixed WebView2 Runtime</span></div><span className="status-badge success"><CheckCircle2 size={12} /> 已启用</span></div>
        </section>
        <section className="settings-card">
          <header><Database size={18} /><div><h2>工作区数据</h2><p>项目、脚本包、运行记录与产物统一保存在工作区。</p></div></header>
          <div className="setting-row data-directory-row"><div><strong>当前数据目录</strong><code>{dataDirectory}</code><span>Windows 固定使用应用安装目录下的 data，不回落到 AppData。</span></div><button className="button ghost small" type="button" onClick={() => void navigator.clipboard.writeText(dataDirectory)}><Clipboard size={13} /> 复制</button></div>
        </section>
        <section className="settings-card settings-card-wide">
          <header><Wrench size={18} /><div><h2>交互完整性审计</h2><p>可见按钮必须执行真实操作；未完成模块明确显示“开发中”。</p></div></header>
          <div className="interaction-audit">
            <span><CheckCircle2 size={13} /> 窗口拖动、最小化、最大化/还原、关闭</span>
            <span><CheckCircle2 size={13} /> RPAZ 安装、参数保存、预检与真实运行</span>
            <span><CheckCircle2 size={13} /> Studio 新建、编辑、直接运行、导出与已安装包工作副本</span>
            <span><CheckCircle2 size={13} /> `.ipynb` 编辑、持久 Kernel、单元格输出与变量浏览</span>
          </div>
        </section>
      </div>
    </div>
  );
}
