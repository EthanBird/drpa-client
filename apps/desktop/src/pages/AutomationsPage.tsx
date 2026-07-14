import { AlertTriangle, CalendarClock, CheckCircle2, Clock3, PauseCircle, Plus, ShieldCheck, Zap } from "lucide-react";

import { useAppStore } from "../app/store";
import type { AutomationStatus, AutomationSummary } from "../domain/models";

export function AutomationsPage() {
  const snapshot = useAppStore((state) => state.snapshot);
  const setActiveNavigation = useAppStore((state) => state.setActiveNavigation);

  if (!snapshot) return <div className="page loading-page"><div className="skeleton skeleton-title" /></div>;

  const enabled = snapshot.automations.filter((task) => task.status === "enabled").length;
  const attention = snapshot.automations.filter((task) => task.status === "needsAttention").length;
  const paused = snapshot.automations.filter((task) => task.status === "paused").length;

  return (
    <div className="page automations-page">
      <header className="page-header automations-header">
        <div>
          <div className="eyebrow">应用内调度 · 设计预览</div>
          <h1>自动化计划</h1>
          <p>根据开发文档 P2 阶段展示任务、下一次运行、最近结果和健康状态；调度执行仍由后续 Host Scheduler 接管。</p>
        </div>
        <div className="header-actions">
          <button className="button secondary" type="button" disabled><Plus size={15} /> 新建计划</button>
          <button className="button primary" type="button" onClick={() => setActiveNavigation("library")}><Zap size={15} /> 选择脚本包</button>
        </div>
      </header>

      <section className="automation-summary-strip" aria-label="自动化计划概览">
        <AutomationMetric icon={<CheckCircle2 size={16} />} label="已启用" value={String(enabled)} tone="green" />
        <AutomationMetric icon={<PauseCircle size={16} />} label="已暂停" value={String(paused)} tone="amber" />
        <AutomationMetric icon={<AlertTriangle size={16} />} label="需处理" value={String(attention)} tone="danger" />
        <AutomationMetric icon={<CalendarClock size={16} />} label="计划总数" value={String(snapshot.automations.length)} tone="blue" />
      </section>

      <section className="panel automation-board">
        <header className="panel-header">
          <div><h2>任务队列</h2><p>首版只呈现本地计划模型，不触发后台执行</p></div>
          <span className="status-badge queued">P2 Scheduler</span>
        </header>
        {snapshot.automations.length === 0 ? (
          <div className="empty-state">
            <CalendarClock size={28} />
            <h2>暂无自动化计划</h2>
            <p>为脚本包配置时间或事件触发器后，会在这里显示下一次运行和健康状态。</p>
          </div>
        ) : (
          <div className="automation-table">
            <div className="automation-table-header">
              <span>计划</span>
              <span>触发器</span>
              <span>下一次运行</span>
              <span>执行策略</span>
              <span>健康状态</span>
            </div>
            {snapshot.automations.map((task) => <AutomationRow key={task.id} task={task} />)}
          </div>
        )}
      </section>
    </div>
  );
}

function AutomationMetric({ icon, label, value, tone }: { icon: React.ReactNode; label: string; value: string; tone: string }) {
  return <article className={`automation-metric tone-${tone}`}><span>{icon}</span><div><strong>{value}</strong><small>{label}</small></div></article>;
}

function AutomationRow({ task }: { task: AutomationSummary }) {
  const StatusIcon = statusIcon(task.status);
  return (
    <article className="automation-row">
      <span className={`automation-state ${task.status}`}><StatusIcon size={15} /></span>
      <div className="automation-title">
        <strong>{task.name}</strong>
        <small>{task.packageName} · {task.profileName}</small>
      </div>
      <span className="automation-trigger"><Clock3 size={13} /> {task.triggerLabel}</span>
      <span className="tabular">{task.nextRun}</span>
      <span className="automation-policy">
        <ShieldCheck size={13} />
        <span>{concurrencyLabel(task.concurrencyPolicy)}<small>{task.retryPolicy}</small></span>
      </span>
      <span className="automation-health">
        <span className={`status-badge ${task.status}`}>{statusLabel(task.status)}</span>
        <small>{task.health}</small>
      </span>
    </article>
  );
}

function statusIcon(status: AutomationStatus) {
  return ({ enabled: CheckCircle2, paused: PauseCircle, needsAttention: AlertTriangle } satisfies Record<AutomationStatus, typeof CheckCircle2>)[status];
}

function statusLabel(status: AutomationStatus) {
  return ({ enabled: "已启用", paused: "已暂停", needsAttention: "需处理" } satisfies Record<AutomationStatus, string>)[status];
}

function concurrencyLabel(policy: AutomationSummary["concurrencyPolicy"]) {
  return ({ allow: "允许并发", forbid: "禁止并发", replace: "替换运行", queueOne: "排队一个" } satisfies Record<AutomationSummary["concurrencyPolicy"], string>)[policy];
}
