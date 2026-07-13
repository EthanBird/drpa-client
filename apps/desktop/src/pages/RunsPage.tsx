import { Activity, CircleCheck, Play, Search, Square } from "lucide-react";
import { useMemo, useState } from "react";

import { useAppStore } from "../app/store";

export function RunsPage() {
  const snapshot = useAppStore((state) => state.snapshot);
  const [query, setQuery] = useState("");
  const runs = useMemo(() => snapshot?.runs.filter((run) => `${run.id} ${run.packageName} ${run.profileName}`.toLowerCase().includes(query.toLowerCase())) ?? [], [query, snapshot]);
  if (!snapshot) return null;
  const completed = snapshot.runs.filter((run) => run.status === "success" || run.status === "failed").length;
  const cancelled = snapshot.runs.filter((run) => run.status === "cancelled").length;
  return (
    <div className="page runs-page">
      <header className="page-header"><div><div className="eyebrow">执行与审计</div><h1>运行记录</h1><p>查看活动任务，并审计每一次脚本包执行。</p></div></header>
      <div className="runs-summary-strip"><span><Activity size={15} /><strong>{snapshot.stats.activeRuns}</strong> 活动</span><span><CircleCheck size={15} /><strong>{completed}</strong> 已完成</span><span><Square size={13} /><strong>{cancelled}</strong> 已取消</span></div>
      <section className="panel runs-detail-panel"><div className="library-toolbar"><div className="large-search"><Search size={15} /><input aria-label="搜索运行记录" placeholder="按运行 ID、脚本包或任务配置搜索" value={query} onChange={(event) => setQuery(event.target.value)} /></div></div><div className="full-runs-table"><div className="full-runs-header"><span>任务</span><span>状态</span><span>开始时间</span><span>耗时</span><span>进度</span><span /></div>{runs.map((run) => <div className="full-runs-row" key={run.id}><span><strong>{run.packageName}</strong><small>{run.profileName} · {run.id}</small></span><span className={`status-badge ${run.status}`}>{statusLabel(run.status)}</span><span className="tabular">{run.startedAt}</span><span className="tabular">{run.duration}</span><span>{run.progress !== undefined ? <span className="table-progress"><i style={{ width: `${run.progress}%` }} /><em>{run.progress}%</em></span> : "—"}</span><button className="icon-button subtle" type="button" aria-label={`查看 ${run.id}`} disabled><Play size={13} /></button></div>)}</div>{runs.length === 0 && <div className="empty-state"><Activity size={26} /><h2>暂无运行记录</h2><p>从运行工作台提交任务后会显示在这里。</p></div>}</section>
    </div>
  );
}

function statusLabel(status: string) {
  return ({ running: "运行中", queued: "排队中", success: "成功", failed: "失败", cancelled: "已取消" } as Record<string, string>)[status] ?? status;
}
