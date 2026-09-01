import type { CSSProperties, ReactNode } from "react";

import type { DashboardDataset, DashboardNumberFormat, DashboardWidget } from "../../domain/models";

interface DashboardWidgetViewProps {
  widget: DashboardWidget;
  dataset?: DashboardDataset;
  loading: boolean;
  error?: string;
}

const CHART_PALETTE = ["#4f6bed", "#159570", "#8b5cf6", "#d97706", "#db5a6b", "#0891b2", "#64748b", "#84a22f"];

export function DashboardWidgetView({ widget, dataset, loading, error }: DashboardWidgetViewProps) {
  if (loading) return <div className="bi-widget-loading"><span /><span /><span /></div>;
  if (error) return <div className="bi-widget-error"><strong>数据加载失败</strong><span>{error}</span></div>;
  if (widget.kind === "markdown") return <DashboardMarkdown source={widget.options.text} />;
  const resolved = dataset ?? { columns: [], rows: [], durationMs: 0, truncated: false };
  if (widget.kind === "metric") return <MetricView widget={widget} dataset={resolved} />;
  if (widget.kind === "line" || widget.kind === "bar") return <CartesianChart widget={widget} dataset={resolved} />;
  if (widget.kind === "pie") return <PieChart widget={widget} dataset={resolved} />;
  return <TableView dataset={resolved} />;
}

function MetricView({ widget, dataset }: { widget: DashboardWidget; dataset: DashboardDataset }) {
  const field = widget.encoding.valueField || dataset.columns[0] || "";
  const raw = dataset.rows[0]?.[field];
  return (
    <div className="bi-metric" style={{ "--widget-accent": widget.options.color } as CSSProperties}>
      <strong>{formatValue(raw, widget.options.numberFormat)}</strong>
      <span>{field || "等待字段"}</span>
      <i />
    </div>
  );
}

function CartesianChart({ widget, dataset }: { widget: DashboardWidget; dataset: DashboardDataset }) {
  const categoryField = widget.encoding.categoryField || dataset.columns[0] || "";
  const valueField = widget.encoding.valueField || dataset.columns.find((field) => dataset.rows.some((row) => numeric(row[field]) !== null)) || "";
  const points = dataset.rows.slice(0, 32).map((row) => ({
    label: displayValue(row[categoryField]),
    value: numeric(row[valueField]) ?? 0,
  }));
  if (!points.length) return <EmptyWidget label="查询没有返回可绘制的数据" />;
  const width = 640;
  const height = 220;
  const pad = { left: 36, right: 16, top: 18, bottom: 34 };
  const max = Math.max(...points.map((point) => point.value), 1);
  const min = Math.min(...points.map((point) => point.value), 0);
  const range = Math.max(1, max - min);
  const x = (index: number) => pad.left + (points.length === 1 ? (width - pad.left - pad.right) / 2 : index * (width - pad.left - pad.right) / (points.length - 1));
  const y = (value: number) => pad.top + (max - value) / range * (height - pad.top - pad.bottom);
  const path = points.map((point, index) => `${index ? "L" : "M"}${x(index).toFixed(1)} ${y(point.value).toFixed(1)}`).join(" ");
  const labelStep = Math.max(1, Math.ceil(points.length / 6));
  return (
    <div className="bi-chart">
      <svg viewBox={`0 0 ${width} ${height}`} role="img" aria-label={`${widget.title}图表`}>
        {[0, 0.5, 1].map((ratio) => {
          const yy = pad.top + ratio * (height - pad.top - pad.bottom);
          const label = max - ratio * range;
          return <g key={ratio}><line x1={pad.left} y1={yy} x2={width - pad.right} y2={yy} className="bi-chart-grid" /><text x={pad.left - 7} y={yy + 3} textAnchor="end">{compactNumber(label)}</text></g>;
        })}
        {widget.kind === "line" ? (
          <>
            <path d={`${path} L${x(points.length - 1)} ${height - pad.bottom} L${x(0)} ${height - pad.bottom} Z`} fill={hexWithAlpha(widget.options.color, "1f")} />
            <path d={path} fill="none" stroke={widget.options.color} strokeWidth="3" strokeLinecap="round" strokeLinejoin="round" />
            {points.map((point, index) => <circle key={`${point.label}-${index}`} cx={x(index)} cy={y(point.value)} r="3.5" fill="var(--surface-1)" stroke={widget.options.color} strokeWidth="2" />)}
          </>
        ) : points.map((point, index) => {
          const slot = (width - pad.left - pad.right) / points.length;
          const barWidth = Math.max(5, Math.min(34, slot * 0.62));
          const yy = y(point.value);
          return <rect key={`${point.label}-${index}`} x={pad.left + slot * index + (slot - barWidth) / 2} y={yy} width={barWidth} height={Math.max(1, height - pad.bottom - yy)} rx="4" fill={widget.options.color} opacity=".86" />;
        })}
        {points.map((point, index) => index % labelStep === 0 ? <text key={`label-${index}`} x={x(index)} y={height - 10} textAnchor="middle">{truncate(point.label, 10)}</text> : null)}
      </svg>
      <div className="bi-chart-caption"><span>{categoryField}</span><strong>{valueField}</strong></div>
    </div>
  );
}

function PieChart({ widget, dataset }: { widget: DashboardWidget; dataset: DashboardDataset }) {
  const categoryField = widget.encoding.categoryField || dataset.columns[0] || "";
  const valueField = widget.encoding.valueField || dataset.columns[1] || "";
  const values = dataset.rows.slice(0, 8).map((row, index) => ({
    label: displayValue(row[categoryField]),
    value: Math.max(0, numeric(row[valueField]) ?? 0),
    color: index === 0 ? widget.options.color : CHART_PALETTE[index % CHART_PALETTE.length],
  })).filter((item) => item.value > 0);
  const total = values.reduce((sum, item) => sum + item.value, 0);
  if (!total) return <EmptyWidget label="查询没有返回可绘制的数据" />;
  let cursor = 0;
  const stops = values.map((item) => {
    const start = cursor;
    cursor += item.value / total * 100;
    return `${item.color} ${start}% ${cursor}%`;
  });
  return (
    <div className="bi-pie-wrap">
      <div className="bi-pie" style={{ background: `conic-gradient(${stops.join(",")})` }}><span><strong>{compactNumber(total)}</strong><small>总计</small></span></div>
      {widget.options.showLegend && <div className="bi-pie-legend">{values.map((item) => <div key={item.label}><i style={{ background: item.color }} /><span title={item.label}>{item.label}</span><strong>{formatValue(item.value, widget.options.numberFormat)}</strong></div>)}</div>}
    </div>
  );
}

function TableView({ dataset }: { dataset: DashboardDataset }) {
  if (!dataset.columns.length) return <EmptyWidget label="查询没有返回字段" />;
  const columns = dataset.columns.slice(0, 12);
  return (
    <div className="bi-table-wrap">
      <table className="bi-table"><thead><tr>{columns.map((column) => <th key={column}>{column}</th>)}</tr></thead><tbody>
        {dataset.rows.slice(0, 50).map((row, rowIndex) => <tr key={rowIndex}>{columns.map((column) => <td key={column} title={displayValue(row[column])}>{displayValue(row[column])}</td>)}</tr>)}
      </tbody></table>
      {!dataset.rows.length && <div className="bi-table-empty">0 行数据</div>}
    </div>
  );
}

function DashboardMarkdown({ source }: { source: string }) {
  const blocks = source.split(/\r?\n/);
  return <div className="bi-markdown">{blocks.map((line, index) => {
    if (line.startsWith("### ")) return <h4 key={index}>{inlineMarkdown(line.slice(4))}</h4>;
    if (line.startsWith("## ")) return <h3 key={index}>{inlineMarkdown(line.slice(3))}</h3>;
    if (line.startsWith("# ")) return <h2 key={index}>{inlineMarkdown(line.slice(2))}</h2>;
    if (/^[-*] /.test(line)) return <div className="bi-markdown-list" key={index}><i />{inlineMarkdown(line.slice(2))}</div>;
    if (!line.trim()) return <span className="bi-markdown-space" key={index} />;
    return <p key={index}>{inlineMarkdown(line)}</p>;
  })}</div>;
}

function inlineMarkdown(value: string): ReactNode {
  const parts = value.split(/(`[^`]+`|\*\*[^*]+\*\*)/g).filter(Boolean);
  return parts.map((part, index) => {
    if (part.startsWith("`") && part.endsWith("`")) return <code key={index}>{part.slice(1, -1)}</code>;
    if (part.startsWith("**") && part.endsWith("**")) return <strong key={index}>{part.slice(2, -2)}</strong>;
    return part;
  });
}

function EmptyWidget({ label }: { label: string }) {
  return <div className="bi-widget-empty"><span /><strong>{label}</strong></div>;
}

export function formatValue(value: unknown, format: DashboardNumberFormat): string {
  if (value === null || value === undefined || value === "") return "—";
  if (format === "text") return displayValue(value);
  const number = numeric(value);
  if (number === null) return displayValue(value);
  if (format === "percent") return `${new Intl.NumberFormat("zh-CN", { maximumFractionDigits: 1 }).format(number)}%`;
  if (format === "currency") return new Intl.NumberFormat("zh-CN", { style: "currency", currency: "CNY", maximumFractionDigits: 2 }).format(number);
  if (format === "hours") return `${new Intl.NumberFormat("zh-CN", { maximumFractionDigits: 1 }).format(number)} 小时`;
  if (format === "compact") return compactNumber(number);
  return new Intl.NumberFormat("zh-CN", { maximumFractionDigits: 2 }).format(number);
}

function numeric(value: unknown): number | null {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value === "string" && value.trim() && Number.isFinite(Number(value))) return Number(value);
  return null;
}

function compactNumber(value: number): string {
  return new Intl.NumberFormat("zh-CN", { notation: "compact", maximumFractionDigits: 1 }).format(value);
}

function displayValue(value: unknown): string {
  if (value === null || value === undefined) return "—";
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

function truncate(value: string, limit: number): string {
  return value.length > limit ? `${value.slice(0, limit - 1)}…` : value;
}

function hexWithAlpha(color: string, alpha: string): string {
  return /^#[0-9a-f]{6}$/i.test(color) ? `${color}${alpha}` : "rgba(79,107,237,.12)";
}
