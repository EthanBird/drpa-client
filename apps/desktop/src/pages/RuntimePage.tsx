import { CheckCircle2, CircleAlert, Cpu, Database, Globe2, RefreshCw, ShieldCheck, Wrench } from "lucide-react";
import type { ReactNode } from "react";
import { useEffect, useState } from "react";

import type { RuntimeStatus } from "../domain/models";
import { desktopGateway } from "../infra/gateway";

export function RuntimePage() {
  const [status, setStatus] = useState<RuntimeStatus | null>(null);
  const [notice, setNotice] = useState("正在读取封装运行环境…");
  const [busy, setBusy] = useState(false);
  const [repairConfirmationOpen, setRepairConfirmationOpen] = useState(false);

  const load = async () => {
    try {
      const next = await desktopGateway.getRuntimeStatus();
      setStatus(next);
      setNotice(next.message);
    } catch (error) {
      setNotice(`无法读取运行环境：${String(error)}`);
    }
  };

  useEffect(() => { void load(); }, []);

  const initialize = async () => {
    setBusy(true);
    setNotice("正在从安装包内的 wheelhouse 初始化并验证，过程不访问网络…");
    try {
      const next = await desktopGateway.initializeRuntime();
      setStatus(next);
      setNotice("运行环境验证通过：RPA Runtime、Jupyter Kernel 与浏览器模块均可导入");
    } catch (error) {
      setNotice(`初始化失败：${String(error)}`);
      await load();
    } finally { setBusy(false); }
  };

  const repair = async () => {
    setBusy(true);
    setNotice("正在清理不完整环境并执行离线重建…");
    try {
      const next = await desktopGateway.repairRuntime();
      setStatus(next);
      setNotice("修复完成，全部运行依赖验证通过");
    } catch (error) {
      setNotice(`修复失败：${String(error)}`);
      await load();
    } finally { setBusy(false); }
  };

  const confirmRepair = () => {
    setRepairConfirmationOpen(false);
    void repair();
  };

  return (
    <div className="page runtime-page">
      <header className="page-header">
        <div><div className="eyebrow">Windows 离线运行基础设施</div><h1>运行环境</h1><p>这里展示的是真实安装内容和健康状态，不依赖系统 Python。</p></div>
        <div className="header-actions"><button className="button secondary" type="button" onClick={load} disabled={busy}><RefreshCw size={15} /> 刷新</button><button className="button primary" type="button" onClick={status?.state === "broken" ? () => setRepairConfirmationOpen(true) : initialize} disabled={busy}>{status?.state === "broken" ? <Wrench size={15} /> : <ShieldCheck size={15} />}{busy ? "处理中…" : status?.state === "broken" ? "修复环境" : "初始化并验证"}</button></div>
      </header>
      <div className={`runtime-banner ${status?.state ?? "loading"}`}><span>{status?.state === "ready" ? <CheckCircle2 size={20} /> : <Cpu size={20} />}</span><div><strong>{status?.state === "ready" ? "运行环境健康" : status?.state === "broken" ? "检测到不完整环境" : "等待首次初始化"}</strong><p>{notice}</p></div></div>
      {status && <div className="runtime-grid">
        <RuntimeCard icon={<Cpu size={18} />} title="Python + RPA" value={`Python ${status.pythonVersion}`} detail={`Bundle ${status.bundleVersion}`} path={status.runtimeRoot} />
        <RuntimeCard icon={<Database size={18} />} title="Jupyter Kernel" value="ipykernel · jupyter_client · ZMQ" detail="真实 Jupyter 消息协议，环境位于安装目录 data" path={status.environmentRoot} />
        <RuntimeCard icon={<Globe2 size={18} />} title="浏览器自动化" value="Chrome for Testing" detail="固定版本，与系统 Chrome 隔离" path={status.browserExecutable} />
      </div>}
      <section className="panel runtime-policy"><div><ShieldCheck size={18} /><span><strong>离线与数据策略</strong><small>Python、wheelhouse、Jupyter 与浏览器均随 Windows 安装包交付；生成环境、缓存、项目和日志只写入安装目录下的 data。</small></span></div><button className="button danger" type="button" onClick={() => setRepairConfirmationOpen(true)} disabled={busy}><Wrench size={14} /> 强制重建生成环境</button></section>
      {repairConfirmationOpen && <div className="knowledge-confirm-overlay" role="dialog" aria-modal="true" aria-label="确认强制重建运行环境" onMouseDown={() => setRepairConfirmationOpen(false)}>
        <section className="knowledge-confirm" onMouseDown={(event) => event.stopPropagation()}>
          <div className="knowledge-confirm-icon"><CircleAlert size={18} /></div>
          <div><h2>强制重建整个运行环境？</h2><p>该操作会停止 Studio Kernel，删除并离线重建 data\runtime-environment，通常需要数分钟。脚本包、项目和运行产物会保留。</p></div>
          <footer><button className="button secondary" type="button" onClick={() => setRepairConfirmationOpen(false)}>取消</button><button className="button danger" type="button" onClick={confirmRepair}>确认重建</button></footer>
        </section>
      </div>}
    </div>
  );
}

function RuntimeCard({ icon, title, value, detail, path }: { icon: ReactNode; title: string; value: string; detail: string; path: string }) {
  return <article className="runtime-card"><header>{icon}<span>{title}</span></header><strong>{value}</strong><p>{detail}</p><code title={path}>{path}</code></article>;
}
