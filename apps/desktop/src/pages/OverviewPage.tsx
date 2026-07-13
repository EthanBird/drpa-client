import { Activity, ArrowUpRight, Box, CheckCircle2, Clock3, Play, TrendingUp, Zap } from "lucide-react";

import { useAppStore } from "../app/store";

export function OverviewPage() {
  const snapshot = useAppStore((state) => state.snapshot);
  const setActiveNavigation = useAppStore((state) => state.setActiveNavigation);

  if (!snapshot) return <div className="page loading-page"><div className="skeleton skeleton-title" /></div>;

  return (
    <div className="page overview-page">
      <header className="page-header overview-header">
        <div><div className="eyebrow">Monday · July 13</div><h1>Good morning, Ethan</h1><p>Your automation workspace is healthy and three runs are active.</p></div>
        <button className="button primary" type="button" onClick={() => setActiveNavigation("workbench")}><Play size={15} fill="currentColor" /> Open Workbench</button>
      </header>
      <section className="metric-grid" aria-label="Workspace metrics">
        <MetricCard icon={<Activity size={17} />} label="Active runs" value={String(snapshot.stats.activeRuns)} detail="2 running · 1 queued" tone="blue" />
        <MetricCard icon={<CheckCircle2 size={17} />} label="30-day success" value={`${snapshot.stats.successRate}%`} detail="+1.2% from last period" tone="green" />
        <MetricCard icon={<Box size={17} />} label="Installed packages" value={String(snapshot.stats.packages)} detail="All packages compatible" tone="violet" />
        <MetricCard icon={<Clock3 size={17} />} label="Estimated time saved" value={`${snapshot.stats.savedHours}h`} detail="Across 184 completed runs" tone="amber" />
      </section>
      <div className="overview-grid">
        <section className="panel activity-panel">
          <header className="panel-header"><div><h2>Run activity</h2><p>Last seven days</p></div><button className="button ghost small" type="button">View report <ArrowUpRight size={13} /></button></header>
          <div className="chart-wrap" aria-label="Run activity chart">
            <div className="chart-y-axis"><span>60</span><span>40</span><span>20</span><span>0</span></div>
            <svg viewBox="0 0 700 220" role="img" aria-label="Successful and failed runs over seven days">
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
            <div className="chart-x-axis"><span>Tue</span><span>Wed</span><span>Thu</span><span>Fri</span><span>Sat</span><span>Sun</span><span>Mon</span></div>
          </div>
          <div className="chart-legend"><span><i className="legend-dot success" /> Successful runs</span><span><i className="legend-dot failed" /> Failed runs</span><strong>312 total</strong></div>
        </section>
        <section className="panel health-panel">
          <header className="panel-header"><div><h2>Workspace health</h2><p>Live host diagnostics</p></div><span className="status-badge success">Healthy</span></header>
          <HealthRow label="Host control plane" detail="Responding in 8 ms" value="Operational" />
          <HealthRow label="Python 3.11 runtime" detail="4 cached environments" value="Ready" />
          <HealthRow label="Package trust store" detail="12 verified · 1 local" value="Current" />
          <HealthRow label="Artifact storage" detail="18.4 GB of 120 GB" value="15%" progress={15} />
          <button className="health-action" type="button">Open Runtime Center <ArrowUpRight size={14} /></button>
        </section>
        <section className="panel recent-runs-panel">
          <header className="panel-header"><div><h2>Recent runs</h2><p>Activity across this workspace</p></div><button className="button ghost small" type="button" onClick={() => setActiveNavigation("runs")}>View all <ArrowUpRight size={13} /></button></header>
          <div className="runs-table">
            {snapshot.runs.map((run) => (
              <div className="run-table-row" key={run.id}>
                <span className={`run-state-icon ${run.status}`}><Zap size={14} /></span>
                <span><strong>{run.packageName}</strong><small>{run.profileName}</small></span>
                <span className={`status-badge ${run.status}`}>{run.status}</span>
                <span className="tabular">{run.startedAt}</span>
                <span className="tabular">{run.duration}</span>
                <button className="icon-button subtle" type="button" aria-label={`Open ${run.id}`}><ArrowUpRight size={14} /></button>
              </div>
            ))}
          </div>
        </section>
      </div>
    </div>
  );
}

function MetricCard({ icon, label, value, detail, tone }: { icon: React.ReactNode; label: string; value: string; detail: string; tone: string }) {
  return <article className={`metric-card tone-${tone}`}><header><span>{icon}</span><small>{label}</small><TrendingUp size={14} /></header><strong>{value}</strong><p>{detail}</p></article>;
}

function HealthRow({ label, detail, value, progress }: { label: string; detail: string; value: string; progress?: number }) {
  return <div className="health-row"><span className="health-check"><CheckCircle2 size={15} /></span><div><strong>{label}</strong><small>{detail}</small>{progress !== undefined && <span className="health-progress"><i style={{ width: `${progress}%` }} /></span>}</div><span>{value}</span></div>;
}
