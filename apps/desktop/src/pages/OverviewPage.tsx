import { Activity, ArrowUpRight, Box, CheckCircle2, Clock3, Play, TrendingUp, Zap } from "lucide-react";
import { useEffect, useState } from "react";

import { useAppStore } from "../app/store";
import { desktopGateway } from "../infra/gateway";

export function OverviewPage() {
  const snapshot = useAppStore((state) => state.snapshot);
  const setActiveNavigation = useAppStore((state) => state.setActiveNavigation);
  const [displayName, setDisplayName] = useState("本地用户");

  useEffect(() => {
    void desktopGateway.getCurrentUser().then((user) => setDisplayName(user.displayName)).catch(() => undefined);
  }, []);

  if (!snapshot) return <div className="page loading-page"><div className="skeleton skeleton-title" /></div>;

  return (
    <div className="page overview-page">
      <header className="page-header overview-header">
        <div><div className="eyebrow">本地自动化工作区</div><h1>你好，{displayName}</h1><p>工作区已就绪，当前有 {snapshot.stats.activeRuns} 个活动任务。</p></div>
        <button className="button primary" type="button" onClick={() => setActiveNavigation("workbench")}><Play size={15} fill="currentColor" /> 打开运行工作台</button>
      </header>
      <section className="metric-grid" aria-label="工作区指标">
        <MetricCard icon={<Activity size={17} />} label="活动任务" value={String(snapshot.stats.activeRuns)} detail="运行中与排队任务" tone="blue" />
        <MetricCard icon={<CheckCircle2 size={17} />} label="30 天成功率" value={`${snapshot.stats.successRate}%`} detail="基于已完成任务" tone="green" />
        <MetricCard icon={<Box size={17} />} label="已安装 RPAZ 包" value={String(snapshot.stats.packages)} detail="来自本地工作区" tone="violet" />
        <MetricCard icon={<Clock3 size={17} />} label="预计节省时间" value={`${snapshot.stats.savedHours} 小时`} detail="根据任务记录估算" tone="amber" />
      </section>
      <div className="overview-grid">
        <section className="panel activity-panel">
          <header className="panel-header"><div><h2>运行趋势</h2><p>最近七天</p></div><button className="button ghost small" type="button" onClick={() => setActiveNavigation("runs")}>查看记录 <ArrowUpRight size={13} /></button></header>
          <div className="chart-wrap" aria-label="运行趋势图">
            <div className="chart-y-axis"><span>60</span><span>40</span><span>20</span><span>0</span></div>
            <svg viewBox="0 0 700 220" role="img" aria-label="七天成功和失败任务">
              <defs>
                <linearGradient id="runArea" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0" stopColor="#7c9cff" stopOpacity="0.34" />
                  <stop offset="1" stopColor="#7c9cff" stopOpacity="0" />
                </linearGradient>
              </defs>
              <g className="chart-grid-lines"><path d="M0 10H700M0 70H700M0 130H700M0 190H700" /></g>
              <path className="chart-area" d="M0 174 C55 158,70 104,116 116 S190 138,232 91 S308 44,348 68 S425 119,466 72 S540 26,582 48 S655 75,700 31 V220 H0 Z" />
              <path className="chart-line" d="M0 174 C55 158,70 104,116 116 S190 138,232 91 S308 44,348 68 S425 119,466 72 S540 26,582 48 S655 75,700 31" />
              <path className="chart-failure-line" d="M0 194 C80 194,100 184,150 192 S240 178,300 188 S430 197,500 181 S610 190,700 179" />
            </svg>
            <div className="chart-x-axis"><span>周二</span><span>周三</span><span>周四</span><span>周五</span><span>周六</span><span>周日</span><span>周一</span></div>
          </div>
          <div className="chart-legend"><span><i className="legend-dot success" /> 成功</span><span><i className="legend-dot failed" /> 失败</span><strong>{snapshot.runs.length} 条记录</strong></div>
        </section>
        <section className="panel health-panel">
          <header className="panel-header"><div><h2>工作区健康状态</h2><p>Host 实时诊断</p></div><span className="status-badge success">正常</span></header>
          <HealthRow label="Host 控制平面" detail="桌面命令已连接" value="可用" />
          <HealthRow label="Python 3.11 运行环境" detail="封装运行时等待嵌入桌面发行物" value="待接入" />
          <HealthRow label="RPAZ 包存储" detail={`${snapshot.stats.packages} 个本地 RPAZ 包`} value="正常" />
          <HealthRow label="产物存储" detail="按运行隔离输出目录" value="正常" progress={0} />
          <button className="health-action" type="button" onClick={() => setActiveNavigation("runtimes")}>打开运行环境 <ArrowUpRight size={14} /></button>
        </section>
        <section className="panel recent-runs-panel">
          <header className="panel-header"><div><h2>最近运行</h2><p>当前工作区的任务活动</p></div><button className="button ghost small" type="button" onClick={() => setActiveNavigation("runs")}>查看全部 <ArrowUpRight size={13} /></button></header>
          <div className="runs-table">
            {snapshot.runs.map((run) => (
              <div className="run-table-row" key={run.id}>
                <span className={`run-state-icon ${run.status}`}><Zap size={14} /></span>
                <span><strong>{run.packageName}</strong><small>{run.profileName}</small></span>
                <span className={`status-badge ${run.status}`}>{statusLabel(run.status)}</span>
                <span className="tabular">{run.startedAt}</span>
                <span className="tabular">{run.duration}</span>
                <button className="icon-button subtle" type="button" aria-label={`打开 ${run.id}`} onClick={() => setActiveNavigation("runs")}><ArrowUpRight size={14} /></button>
              </div>
            ))}
          </div>
        </section>
      </div>
    </div>
  );
}

function statusLabel(status: string) {
  return ({ running: "运行中", queued: "排队中", success: "成功", failed: "失败", cancelled: "已取消" } as Record<string, string>)[status] ?? status;
}

function MetricCard({ icon, label, value, detail, tone }: { icon: React.ReactNode; label: string; value: string; detail: string; tone: string }) {
  return <article className={`metric-card tone-${tone}`}><header><span>{icon}</span><small>{label}</small><TrendingUp size={14} /></header><strong>{value}</strong><p>{detail}</p></article>;
}

function HealthRow({ label, detail, value, progress }: { label: string; detail: string; value: string; progress?: number }) {
  return <div className="health-row"><span className="health-check"><CheckCircle2 size={15} /></span><div><strong>{label}</strong><small>{detail}</small>{progress !== undefined && <span className="health-progress"><i style={{ width: `${progress}%` }} /></span>}</div><span>{value}</span></div>;
}
