import {
  Activity,
  AlertTriangle,
  Braces,
  ChevronRight,
  CircleCheck,
  Clock3,
  Copy,
  FileOutput,
  FolderOpen,
  LoaderCircle,
  Search,
  Square,
  TerminalSquare,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { useAppStore } from "../app/store";
import type { RunDetail } from "../domain/models";
import { desktopGateway } from "../infra/gateway";

type DetailTab = "overview" | "logs" | "artifacts" | "parameters";

export function RunsPage() {
  const snapshot = useAppStore((state) => state.snapshot);
  const [query, setQuery] = useState("");
  const [selectedRunId, setSelectedRunId] = useState("");
  const [detail, setDetail] = useState<RunDetail | null>(null);
  const [detailTab, setDetailTab] = useState<DetailTab>("overview");
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [detailError, setDetailError] = useState("");

  const runs = useMemo(
    () => snapshot?.runs.filter((run) => `${run.id} ${run.packageName} ${run.profileName}`.toLowerCase().includes(query.toLowerCase())) ?? [],
    [query, snapshot],
  );

  useEffect(() => {
    if (!selectedRunId && runs[0]) setSelectedRunId(runs[0].id);
    if (selectedRunId && !snapshot?.runs.some((run) => run.id === selectedRunId)) setSelectedRunId(runs[0]?.id ?? "");
  }, [runs, selectedRunId, snapshot]);

  useEffect(() => {
    if (!selectedRunId) {
      setDetail(null);
      return;
    }
    let active = true;
    setLoadingDetail(true);
    setDetailError("");
    void desktopGateway.getRunDetail(selectedRunId)
      .then((next) => { if (active) setDetail(next); })
      .catch((error: unknown) => { if (active) setDetailError(String(error)); })
      .finally(() => { if (active) setLoadingDetail(false); });
    return () => { active = false; };
  }, [selectedRunId, snapshot]);

  if (!snapshot) return null;
  const completed = snapshot.runs.filter((run) => run.status === "success" || run.status === "failed").length;
  const cancelled = snapshot.runs.filter((run) => run.status === "cancelled" || run.status === "interrupted").length;

  return (
    <div className="page runs-page">
      <header className="page-header"><div><div className="eyebrow">执行与审计</div><h1>运行记录</h1><p>查看活动任务，并审计每一次脚本包执行。</p></div></header>
      <div className="runs-summary-strip"><span><Activity size={15} /><strong>{snapshot.stats.activeRuns}</strong> 活动</span><span><CircleCheck size={15} /><strong>{completed}</strong> 已完成</span><span><Square size={13} /><strong>{cancelled}</strong> 已停止</span></div>
      <div className="runs-master-detail">
        <section className="panel runs-detail-panel">
          <div className="library-toolbar"><div className="large-search"><Search size={15} /><input aria-label="搜索运行记录" placeholder="按运行 ID、脚本包或任务配置搜索" value={query} onChange={(event) => setQuery(event.target.value)} /></div></div>
          <div className="full-runs-table">
            <div className="full-runs-header"><span>任务</span><span>状态</span><span>开始时间</span><span>耗时</span><span>进度</span><span /></div>
            {runs.map((run) => (
              <div
                className={`full-runs-row ${selectedRunId === run.id ? "selected" : ""}`}
                key={run.id}
                role="button"
                tabIndex={0}
                onClick={() => setSelectedRunId(run.id)}
                onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") setSelectedRunId(run.id); }}
              >
                <span><strong>{run.packageName}</strong><small>{run.profileName} · {run.id}</small></span>
                <span className={`status-badge ${run.status}`}>{statusLabel(run.status)}</span>
                <span className="tabular">{displayTimestamp(run.startedAt)}</span>
                <span className="tabular">{run.duration}</span>
                <span>{run.progress !== undefined ? <span className="table-progress"><i style={{ width: `${run.progress}%` }} /><em>{run.progress}%</em></span> : "—"}</span>
                <span className="run-row-open" aria-hidden="true"><ChevronRight size={14} /></span>
              </div>
            ))}
          </div>
          {runs.length === 0 && <div className="empty-state"><Activity size={26} /><h2>暂无运行记录</h2><p>从运行工作台提交任务后会显示在这里。</p></div>}
        </section>

        <aside className="panel run-inspector" aria-live="polite">
          {loadingDetail && !detail && <div className="run-inspector-loading"><LoaderCircle className="spin" size={20} /> 正在读取运行详情</div>}
          {detailError && <div className="run-inspector-error"><AlertTriangle size={18} /><span><strong>读取详情失败</strong>{detailError}</span></div>}
          {!selectedRunId && <div className="empty-state"><Activity size={26} /><h2>选择一条运行记录</h2><p>这里会展示完整事件、参数和产物。</p></div>}
          {detail && detail.summary.id === selectedRunId && (
            <>
              <header className="run-inspector-header">
                <div><span className={`status-badge ${detail.summary.status}`}>{statusLabel(detail.summary.status)}</span><h2>{detail.summary.packageName}</h2><p>{detail.summary.profileName} · {detail.summary.id}</p></div>
                <button className="button secondary small" type="button" onClick={() => void desktopGateway.openRunOutputDirectory(detail.summary.id)}><FolderOpen size={13} /> 输出目录</button>
              </header>
              <nav className="run-detail-tabs" aria-label="运行详情视图">
                <button type="button" className={detailTab === "overview" ? "active" : ""} onClick={() => setDetailTab("overview")}><Clock3 size={13} /> 概览</button>
                <button type="button" className={detailTab === "logs" ? "active" : ""} onClick={() => setDetailTab("logs")}><TerminalSquare size={13} /> 日志 <span>{detail.events.length}</span></button>
                <button type="button" className={detailTab === "artifacts" ? "active" : ""} onClick={() => setDetailTab("artifacts")}><FileOutput size={13} /> 产物 <span>{detail.artifacts.length}</span></button>
                <button type="button" className={detailTab === "parameters" ? "active" : ""} onClick={() => setDetailTab("parameters")}><Braces size={13} /> 参数</button>
              </nav>
              <div className="run-detail-content">
                {detailTab === "overview" && <RunOverview detail={detail} />}
                {detailTab === "logs" && <RunEvents detail={detail} />}
                {detailTab === "artifacts" && <RunArtifacts detail={detail} />}
                {detailTab === "parameters" && <pre className="run-json-view">{JSON.stringify(detail.parameters, null, 2)}</pre>}
              </div>
            </>
          )}
        </aside>
      </div>
    </div>
  );
}

function RunOverview({ detail }: { detail: RunDetail }) {
  const summary = detail.summary;
  return (
    <div className="run-overview-grid">
      <Info label="脚本包" value={`${summary.packageId || "—"} · ${summary.packageVersion || "—"}`} />
      <Info label="任务配置" value={`${summary.profileName} · ${summary.profileId || "—"}`} />
      <Info label="开始时间" value={displayTimestamp(summary.startedAt, true)} />
      <Info label="结束时间" value={summary.finishedAt ? displayTimestamp(summary.finishedAt, true) : "运行中"} />
      <Info label="耗时" value={summary.duration} />
      <Info label="退出码" value={summary.exitCode === undefined ? "—" : String(summary.exitCode)} />
      <Info label="输出目录" value={detail.outputDir} wide />
      {detail.errorMessage && <section className="run-error-card"><AlertTriangle size={17} /><div><strong>{detail.errorMessage}</strong>{detail.errorTraceback && <pre>{detail.errorTraceback}</pre>}</div></section>}
    </div>
  );
}

function RunEvents({ detail }: { detail: RunDetail }) {
  const [query, setQuery] = useState("");
  const [level, setLevel] = useState("all");
  const [copied, setCopied] = useState(false);
  if (detail.events.length === 0) return <div className="empty-inline">这次运行没有持久化事件。</div>;
  const normalized = query.trim().toLowerCase();
  const filtered = detail.events.filter((event) => {
    const eventLevel = event.level ?? "info";
    return (level === "all" || eventLevel === level)
      && (!normalized || `${event.message} ${event.scope ?? "runtime"} ${event.eventType} ${eventLevel}`.toLowerCase().includes(normalized));
  });
  const copyLogs = async () => {
    const text = filtered.map((event) => `${event.recordedAt}\t${(event.level ?? event.eventType).toUpperCase()}\t${event.scope ?? "runtime"}\t${event.message}`).join("\n");
    await navigator.clipboard.writeText(text);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1200);
  };
  return <div className="run-event-section">
    <div className="run-event-toolbar">
      <div><Search size={13} /><input aria-label="筛选运行日志" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="筛选消息、范围或类型" /></div>
      <select aria-label="运行日志级别" value={level} onChange={(event) => setLevel(event.target.value)}><option value="all">全部级别</option><option value="debug">DEBUG</option><option value="info">INFO</option><option value="warn">WARN</option><option value="error">ERROR</option></select>
      <button type="button" onClick={() => void copyLogs()} disabled={filtered.length === 0}><Copy size={12} /> {copied ? "已复制" : "复制日志"}</button>
    </div>
    <div className="run-event-count">显示 {filtered.length} / {detail.events.length} 条持久化事件</div>
    {filtered.length === 0 ? <div className="empty-inline">没有符合筛选条件的日志。</div> : <div className="run-event-list">{filtered.map((event) => <div className={`run-event level-${event.level ?? "info"}`} key={event.id}><time>{displayTimestamp(event.recordedAt)}</time><span>{event.level ?? event.eventType}</span><em>{event.scope ?? "runtime"}</em><p>{event.message}</p></div>)}</div>}
  </div>;
}

function RunArtifacts({ detail }: { detail: RunDetail }) {
  if (detail.artifacts.length === 0) return <div className="empty-inline">这次运行没有登记产物。</div>;
  return <div className="run-artifact-list">{detail.artifacts.map((artifact) => <div key={artifact.id}><FileOutput size={16} /><span><strong>{artifact.label}</strong><small title={artifact.path}>{artifact.path}</small></span><em>{formatBytes(artifact.size)}</em><button type="button" aria-label={`复制产物路径 ${artifact.label}`} title="复制完整路径" onClick={() => void navigator.clipboard.writeText(artifact.path)}><Copy size={12} /></button></div>)}</div>;
}

function Info({ label, value, wide = false }: { label: string; value: string; wide?: boolean }) {
  return <section className={wide ? "wide" : ""}><span>{label}</span><strong>{value}</strong></section>;
}

function statusLabel(status: string) {
  return ({ running: "运行中", queued: "排队中", success: "成功", failed: "失败", cancelled: "已取消", interrupted: "意外中断" } as Record<string, string>)[status] ?? status;
}

function displayTimestamp(value: string, full = false) {
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) return value;
  return new Intl.DateTimeFormat("zh-CN", full ? { dateStyle: "medium", timeStyle: "medium" } : { hour: "2-digit", minute: "2-digit", second: "2-digit" }).format(parsed);
}

function formatBytes(value?: number) {
  if (value === undefined) return "—";
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / 1024 / 1024).toFixed(1)} MB`;
}
