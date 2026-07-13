import { Activity, ChevronDown, CircleCheck, Filter, Pause, Play, Search, Square } from "lucide-react";

import { useAppStore } from "../app/store";

export function RunsPage() {
  const snapshot = useAppStore((state) => state.snapshot);
  if (!snapshot) return null;
  return (
    <div className="page runs-page">
      <header className="page-header"><div><div className="eyebrow">Execution history</div><h1>Runs</h1><p>Observe active work and audit every completed package execution.</p></div><button className="button secondary" type="button"><Pause size={14} /> Pause queue</button></header>
      <div className="runs-summary-strip"><span><Activity size={15} /><strong>3</strong> active</span><span><CircleCheck size={15} /><strong>184</strong> completed this month</span><span><Square size={13} /><strong>1</strong> cancelled</span></div>
      <section className="panel runs-detail-panel"><div className="library-toolbar"><div className="large-search"><Search size={15} /><input aria-label="Search runs" placeholder="Search by run, package or profile" /></div><button className="button secondary" type="button"><Filter size={14} /> Status <ChevronDown size={12} /></button></div><div className="full-runs-table"><div className="full-runs-header"><span>Run</span><span>Status</span><span>Started</span><span>Duration</span><span>Progress</span><span /></div>{snapshot.runs.map((run) => <div className="full-runs-row" key={run.id}><span><strong>{run.packageName}</strong><small>{run.profileName} · {run.id}</small></span><span className={`status-badge ${run.status}`}>{run.status}</span><span className="tabular">{run.startedAt}</span><span className="tabular">{run.duration}</span><span>{run.progress !== undefined ? <span className="table-progress"><i style={{ width: `${run.progress}%` }} /><em>{run.progress}%</em></span> : "—"}</span><button className="icon-button subtle" type="button" aria-label={`Open ${run.id}`}><Play size={13} /></button></div>)}</div></section>
    </div>
  );
}
