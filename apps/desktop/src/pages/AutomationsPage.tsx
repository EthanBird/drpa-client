import {
  Activity,
  AlertCircle,
  ArrowRight,
  CalendarClock,
  CheckCircle2,
  ChevronRight,
  Clock3,
  FileBarChart,
  History,
  LoaderCircle,
  Mail,
  MessageSquareMore,
  Newspaper,
  Package,
  PauseCircle,
  Pencil,
  Play,
  Plus,
  RadioTower,
  RefreshCw,
  RotateCcw,
  Search,
  Send,
  Settings2,
  ShieldCheck,
  Sparkles,
  TimerReset,
  Trash2,
  Webhook,
  X,
  Zap,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

import { useAppStore } from "../app/store";
import { useNavigationSurfaceActive } from "../app/NavigationSurface";
import type { AutomationSummary, PackageSummary } from "../domain/models";
import { desktopGateway } from "../infra/gateway";
import "../styles/automations.css";

type ScheduleKind = "cron" | "daily" | "weekly" | "interval";
type ConcurrencyPolicy = "skip" | "queue" | "parallel";
type RunStatus = "queued" | "running" | "succeeded" | "failed" | "skipped";
type RunTrigger = "manual" | "scheduled";

interface AutomationSchedule {
  type: ScheduleKind;
  cron: string;
  time: string;
  daysOfWeek: number[];
  intervalMinutes: number;
  displayLabel?: string;
}

interface AutomationAction {
  type: string;
  packageId: string;
  entrypoint: string;
  parameters: Record<string, unknown>;
}

interface RetryPolicy {
  maxAttempts: number;
  delaySeconds: number;
  backoffMultiplier: number;
}

interface DeliveryTarget {
  id: string;
  type: string;
  name: string;
  enabled: boolean;
  configuration: Record<string, unknown>;
}

interface AutomationPlan {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  schedule: AutomationSchedule;
  action: AutomationAction;
  concurrencyPolicy: ConcurrencyPolicy;
  retryPolicy: RetryPolicy;
  timeoutSeconds: number;
  deliveryTargets: DeliveryTarget[];
  createdAt: number;
  updatedAt: number;
  lastRunAt?: number | null;
  nextRunAt?: number | null;
  lastScheduledMinute?: number | null;
}

interface AutomationPlanInput {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  schedule: AutomationSchedule;
  action: AutomationAction;
  concurrencyPolicy: ConcurrencyPolicy;
  retryPolicy: RetryPolicy;
  timeoutSeconds: number;
  deliveryTargets: DeliveryTarget[];
}

interface DeliveryResult {
  targetId: string;
  targetName: string;
  status: string;
  error: string;
}

interface AutomationRun {
  id: string;
  planId: string;
  planName: string;
  trigger: RunTrigger;
  status: RunStatus;
  queuedAt: number;
  startedAt?: number | null;
  finishedAt?: number | null;
  attempt: number;
  result?: unknown;
  error: string;
  deliveryResults: DeliveryResult[];
}

interface AutomationGateway {
  listAutomationPlans(): Promise<AutomationPlan[]>;
  createAutomationPlan(input: AutomationPlanInput): Promise<AutomationPlan>;
  updateAutomationPlan(input: AutomationPlanInput): Promise<AutomationPlan>;
  deleteAutomationPlan(planId: string): Promise<void>;
  setAutomationPlanEnabled(planId: string, enabled: boolean): Promise<AutomationPlan>;
  runAutomationPlanNow(planId: string): Promise<AutomationRun>;
  listAutomationRuns(planId?: string, limit?: number): Promise<AutomationRun[]>;
}

interface DraftDeliveryTarget extends DeliveryTarget {
  editorKey: string;
  configurationText: string;
}

interface AutomationDraft extends Omit<AutomationPlanInput, "deliveryTargets"> {
  deliveryTargets: DraftDeliveryTarget[];
  parametersText: string;
}

type TemplateKind = "news" | "report" | "broadcast" | "rpaz";
type NoticeTone = "neutral" | "success" | "error";

const automationGateway = desktopGateway as typeof desktopGateway & AutomationGateway;

const weekDays = [
  { value: 1, label: "一" },
  { value: 2, label: "二" },
  { value: 3, label: "三" },
  { value: 4, label: "四" },
  { value: 5, label: "五" },
  { value: 6, label: "六" },
  { value: 0, label: "日" },
];

const deliveryKinds = [
  { value: "in-app", label: "应用内通知" },
  { value: "email", label: "电子邮件" },
  { value: "webhook", label: "Webhook" },
  { value: "channel", label: "消息频道" },
  { value: "file", label: "保存到文件" },
];

const templates: Array<{
  kind: TemplateKind;
  title: string;
  description: string;
  tag: string;
}> = [
  {
    kind: "news",
    title: "新闻摘要",
    description: "每天早上汇总信息源，生成带引用的简报。",
    tag: "每日 08:00",
  },
  {
    kind: "report",
    title: "报告生成",
    description: "按周运行 RPAZ 包并生成结构化业务报告。",
    tag: "每周一",
  },
  {
    kind: "broadcast",
    title: "多频道广播",
    description: "一次运行，将结果投递到邮件、Webhook 与频道。",
    tag: "工作日 18:00",
  },
  {
    kind: "rpaz",
    title: "RPAZ 定时运行",
    description: "从已安装的 RPAZ 包开始，自定义时间与参数。",
    tag: "灵活配置",
  },
];

export function AutomationsPage() {
  const snapshot = useAppStore((state) => state.snapshot);
  const pageActive = useNavigationSurfaceActive();
  const packages = snapshot?.packages ?? [];
  const [plans, setPlans] = useState<AutomationPlan[]>([]);
  const [runs, setRuns] = useState<AutomationRun[]>([]);
  const [selectedId, setSelectedId] = useState("");
  const [query, setQuery] = useState("");
  const [draft, setDraft] = useState<AutomationDraft | null>(null);
  const [pendingDelete, setPendingDelete] = useState<AutomationPlan | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState("");
  const [notice, setNotice] = useState("正在读取当前工作区的自动化计划…");
  const [noticeTone, setNoticeTone] = useState<NoticeTone>("neutral");

  const backendAvailable = typeof automationGateway.listAutomationPlans === "function";

  const refresh = useCallback(async (preferredId?: string) => {
    if (!backendAvailable) {
      const previewPlans = legacyPlans(snapshot?.automations ?? [], snapshot?.packages ?? []);
      setPlans(previewPlans);
      setRuns([]);
      setSelectedId((current) => (
        previewPlans.some((plan) => plan.id === current) ? current : previewPlans[0]?.id ?? ""
      ));
      setLoading(false);
      setNotice("浏览器预览正在显示工作区快照；桌面端会连接真实本地调度器。");
      return;
    }
    setLoading(true);
    try {
      const [nextPlans, nextRuns] = await Promise.all([
        automationGateway.listAutomationPlans(),
        automationGateway.listAutomationRuns(undefined, 200),
      ]);
      const resolvedPlans = nextPlans.length === 0 && !("__TAURI_INTERNALS__" in window)
        ? legacyPlans(snapshot?.automations ?? [], snapshot?.packages ?? [])
        : nextPlans;
      setPlans(resolvedPlans);
      setRuns(nextRuns);
      setSelectedId((current) => {
        const candidate = preferredId || current;
        return resolvedPlans.some((plan) => plan.id === candidate)
          ? candidate
          : resolvedPlans[0]?.id ?? "";
      });
      setNotice(
        resolvedPlans.length
          ? `已同步 ${resolvedPlans.length} 个计划和 ${nextRuns.length} 条运行记录。`
          : "当前工作区还没有自动化计划，可以从模板开始。",
      );
      setNoticeTone("neutral");
    } catch (error) {
      setNotice(errorMessage(error));
      setNoticeTone("error");
    } finally {
      setLoading(false);
    }
  }, [backendAvailable, snapshot]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const hasActiveRun = runs.some((run) => run.status === "queued" || run.status === "running");
  useEffect(() => {
    if (!pageActive || !backendAvailable || !hasActiveRun) return;
    const timer = window.setInterval(() => {
      void automationGateway.listAutomationRuns(undefined, 200).then(setRuns).catch(() => undefined);
    }, 2_000);
    return () => window.clearInterval(timer);
  }, [pageActive, backendAvailable, hasActiveRun]);

  const selectedPlan = plans.find((plan) => plan.id === selectedId) ?? null;
  const selectedRuns = runs.filter((run) => run.planId === selectedId);
  const filteredPlans = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    if (!normalized) return plans;
    return plans.filter((plan) => {
      const packageName = packages.find((item) => item.id === plan.action.packageId)?.name ?? "";
      return `${plan.name} ${plan.description} ${packageName}`.toLowerCase().includes(normalized);
    });
  }, [packages, plans, query]);

  const enabledCount = plans.filter((plan) => plan.enabled).length;
  const runningCount = runs.filter((run) => run.status === "queued" || run.status === "running").length;
  const failedCount = runs.filter((run) => run.status === "failed").length;
  const deliveryCount = plans.reduce(
    (total, plan) => total + plan.deliveryTargets.filter((target) => target.enabled).length,
    0,
  );
  const nextPlan = plans
    .filter((plan) => plan.enabled && plan.nextRunAt)
    .sort((left, right) => (left.nextRunAt ?? Infinity) - (right.nextRunAt ?? Infinity))[0];

  function openNew(kind: TemplateKind = "rpaz") {
    setDraft(createTemplateDraft(kind, packages[0]));
  }

  function openEdit(plan: AutomationPlan) {
    setDraft(planToDraft(plan));
  }

  async function savePlan() {
    if (!draft) return;
    let input: AutomationPlanInput;
    try {
      input = validateAndBuildInput(draft);
    } catch (error) {
      setNotice(errorMessage(error));
      setNoticeTone("error");
      return;
    }
    setBusy("save");
    try {
      const saved = input.id
        ? await automationGateway.updateAutomationPlan(input)
        : await automationGateway.createAutomationPlan(input);
      setDraft(null);
      setNotice(input.id ? "计划配置已保存。" : "自动化计划已创建。");
      setNoticeTone("success");
      await refresh(saved.id);
    } catch (error) {
      setNotice(errorMessage(error));
      setNoticeTone("error");
    } finally {
      setBusy("");
    }
  }

  async function togglePlan(plan: AutomationPlan) {
    setBusy(`toggle:${plan.id}`);
    try {
      const updated = await automationGateway.setAutomationPlanEnabled(plan.id, !plan.enabled);
      setPlans((current) => current.map((item) => (item.id === updated.id ? updated : item)));
      setNotice(updated.enabled ? `“${updated.name}”已启用。` : `“${updated.name}”已暂停。`);
      setNoticeTone("success");
    } catch (error) {
      setNotice(errorMessage(error));
      setNoticeTone("error");
    } finally {
      setBusy("");
    }
  }

  async function runNow(plan: AutomationPlan) {
    setBusy(`run:${plan.id}`);
    try {
      const run = await automationGateway.runAutomationPlanNow(plan.id);
      setSelectedId(plan.id);
      setRuns((current) => [run, ...current.filter((item) => item.id !== run.id)]);
      setNotice(
        run.status === "skipped"
          ? `“${plan.name}”已有实例运行，本次按并发策略跳过。`
          : `“${plan.name}”已加入运行队列。`,
      );
      setNoticeTone(run.status === "skipped" ? "neutral" : "success");
      window.setTimeout(() => void refresh(plan.id), 800);
    } catch (error) {
      setNotice(errorMessage(error));
      setNoticeTone("error");
    } finally {
      setBusy("");
    }
  }

  async function removePlan() {
    if (!pendingDelete) return;
    setBusy(`delete:${pendingDelete.id}`);
    try {
      await automationGateway.deleteAutomationPlan(pendingDelete.id);
      const name = pendingDelete.name;
      setPendingDelete(null);
      setNotice(`“${name}”已删除，既有运行历史仍保留用于审计。`);
      setNoticeTone("success");
      await refresh();
    } catch (error) {
      setNotice(errorMessage(error));
      setNoticeTone("error");
    } finally {
      setBusy("");
    }
  }

  return (
    <div className="page automations-page automation-v2-page">
      <header className="page-header automation-v2-header">
        <div>
          <div className="eyebrow">Automation Scheduler · 工作区隔离</div>
          <h1>自动化计划</h1>
          <p>让 RPAZ 包按计划生成新闻摘要、报告，并将结果投递到多个频道。</p>
        </div>
        <div className="header-actions">
          <button
            className="button secondary"
            type="button"
            onClick={() => void refresh()}
            disabled={loading}
          >
            <RefreshCw className={loading ? "spin" : ""} size={14} />
            刷新
          </button>
          <button className="button primary" type="button" onClick={() => openNew()}>
            <Plus size={15} />
            新建计划
          </button>
        </div>
      </header>

      <main className="automation-v2-content">
        <section className="automation-v2-templates" aria-labelledby="automation-template-title">
          <div className="automation-v2-section-heading">
            <div>
              <Sparkles size={15} />
              <span>
                <strong id="automation-template-title">从场景模板开始</strong>
                <small>模板只预填配置，保存前仍可完整调整。</small>
              </span>
            </div>
            <span className="automation-v2-local-chip">
              <ShieldCheck size={12} />
              <span>P2 Scheduler</span>
              · 当前工作区
            </span>
          </div>
          <div className="automation-v2-template-grid">
            {templates.map((template) => (
              <button
                key={template.kind}
                className={`automation-v2-template template-${template.kind}`}
                type="button"
                onClick={() => openNew(template.kind)}
              >
                <span className="automation-v2-template-icon">{templateIcon(template.kind)}</span>
                <span>
                  <strong>{template.title}</strong>
                  <small>{template.description}</small>
                  <em>{template.tag}</em>
                </span>
                <ChevronRight size={14} />
              </button>
            ))}
          </div>
        </section>

        <section className="automation-v2-metrics" aria-label="自动化计划概览">
          <Metric icon={<CheckCircle2 size={15} />} label="已启用" value={String(enabledCount)} tone="green" />
          <Metric icon={<Activity size={15} />} label="运行中 / 排队" value={String(runningCount)} tone="blue" />
          <Metric icon={<Send size={15} />} label="启用的投递目标" value={String(deliveryCount)} tone="violet" />
          <Metric icon={<AlertCircle size={15} />} label="最近失败" value={String(failedCount)} tone={failedCount ? "danger" : "neutral"} />
          <div className="automation-v2-next-run">
            <CalendarClock size={16} />
            <span>
              <small>下一次运行</small>
              <strong>{nextPlan ? formatDate(nextPlan.nextRunAt) : "暂无已启用计划"}</strong>
              {nextPlan && <em>计划 · {nextPlan.name}</em>}
            </span>
          </div>
        </section>

        <div className="automation-v2-workspace">
          <aside className="automation-v2-plan-panel">
            <header>
              <div>
                <strong>计划</strong>
                <span>{plans.length}</span>
              </div>
              <button className="icon-button subtle" type="button" aria-label="新建自动化计划" onClick={() => openNew()}>
                <Plus size={15} />
              </button>
            </header>
            <label className="automation-v2-search">
              <Search size={13} />
              <input
                aria-label="搜索自动化计划"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder="搜索计划或 RPAZ 包"
              />
              {query && (
                <button type="button" aria-label="清空搜索" onClick={() => setQuery("")}>
                  <X size={12} />
                </button>
              )}
            </label>
            <div className="automation-v2-plan-list">
              {loading && plans.length === 0 ? (
                <div className="automation-v2-empty compact">
                  <LoaderCircle className="spin" size={20} />
                  <span>正在加载计划…</span>
                </div>
              ) : filteredPlans.length === 0 ? (
                <div className="automation-v2-empty compact">
                  <TimerReset size={22} />
                  <strong>{plans.length ? "没有匹配的计划" : "还没有自动化计划"}</strong>
                  <span>{plans.length ? "尝试其他关键词。" : "选择上方模板，几步即可开始。"}</span>
                </div>
              ) : (
                filteredPlans.map((plan) => {
                  const lastRun = runs.find((run) => run.planId === plan.id);
                  return (
                    <button
                      key={plan.id}
                      type="button"
                      className={`automation-v2-plan-item ${selectedId === plan.id ? "active" : ""}`}
                      onClick={() => setSelectedId(plan.id)}
                    >
                      <span className={`automation-v2-plan-state ${plan.enabled ? "enabled" : "paused"}`}>
                        {plan.enabled ? <Zap size={13} /> : <PauseCircle size={13} />}
                      </span>
                      <span className="automation-v2-plan-copy">
                        <span>
                          <strong>计划 · {plan.name}</strong>
                          <em>{plan.enabled ? "已启用" : "已暂停"}</em>
                        </span>
                        <small>规则 · {formatSchedule(plan.schedule)}</small>
                        <span className="automation-v2-plan-meta">
                          <span>{formatDate(plan.nextRunAt, "暂无下次运行")}</span>
                          {lastRun && <RunDot status={lastRun.status} />}
                        </span>
                      </span>
                      <ChevronRight size={13} />
                    </button>
                  );
                })
              )}
            </div>
          </aside>

          <section className="automation-v2-detail-panel">
            {selectedPlan ? (
              <PlanDetail
                plan={selectedPlan}
                runs={selectedRuns}
                packages={packages}
                busy={busy}
                onToggle={() => void togglePlan(selectedPlan)}
                onRun={() => void runNow(selectedPlan)}
                onEdit={() => openEdit(selectedPlan)}
                onDelete={() => setPendingDelete(selectedPlan)}
              />
            ) : (
              <div className="automation-v2-empty">
                <span className="automation-v2-empty-icon"><CalendarClock size={27} /></span>
                <h2>把重复工作交给调度器</h2>
                <p>创建计划后，可以随时启停、立即运行，并在这里查看每次尝试和投递结果。</p>
                <button className="button primary" type="button" onClick={() => openNew()}>
                  <Plus size={14} />
                  创建第一个计划
                </button>
              </div>
            )}
          </section>
        </div>
      </main>

      <div className={`automation-v2-notice ${noticeTone}`} role="status" aria-live="polite">
        {loading || busy === "save" ? (
          <LoaderCircle className="spin" size={13} />
        ) : noticeTone === "error" ? (
          <AlertCircle size={13} />
        ) : noticeTone === "success" ? (
          <CheckCircle2 size={13} />
        ) : (
          <Activity size={13} />
        )}
        <span>{notice}</span>
      </div>

      {draft && (
        <PlanEditor
          draft={draft}
          packages={packages}
          busy={busy === "save"}
          onChange={setDraft}
          onCancel={() => setDraft(null)}
          onSave={() => void savePlan()}
        />
      )}

      {pendingDelete && (
        <div className="automation-v2-dialog-backdrop" role="presentation" onMouseDown={(event) => {
          if (event.target === event.currentTarget && !busy) setPendingDelete(null);
        }}>
          <section className="automation-v2-confirm" role="dialog" aria-modal="true" aria-labelledby="automation-delete-title">
            <span className="automation-v2-confirm-icon"><Trash2 size={19} /></span>
            <div>
              <h2 id="automation-delete-title">删除“{pendingDelete.name}”？</h2>
              <p>计划配置会从当前工作区移除。已有运行历史会继续保留，正在运行的计划不能删除。</p>
            </div>
            <footer>
              <button className="button secondary" type="button" onClick={() => setPendingDelete(null)} disabled={Boolean(busy)}>取消</button>
              <button className="button danger" type="button" onClick={() => void removePlan()} disabled={Boolean(busy)}>
                {busy ? <LoaderCircle className="spin" size={13} /> : <Trash2 size={13} />}
                删除计划
              </button>
            </footer>
          </section>
        </div>
      )}
    </div>
  );
}

function PlanDetail({
  plan,
  runs,
  packages,
  busy,
  onToggle,
  onRun,
  onEdit,
  onDelete,
}: {
  plan: AutomationPlan;
  runs: AutomationRun[];
  packages: PackageSummary[];
  busy: string;
  onToggle: () => void;
  onRun: () => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const packageItem = packages.find((item) => item.id === plan.action.packageId);
  const profile = packageItem?.profiles.find((item) => item.id === plan.action.entrypoint);
  const isRunning = runs.some((run) => run.status === "queued" || run.status === "running");

  return (
    <>
      <header className="automation-v2-detail-header">
        <div className="automation-v2-detail-title">
          <span className={`automation-v2-plan-state large ${plan.enabled ? "enabled" : "paused"}`}>
            {plan.enabled ? <Zap size={16} /> : <PauseCircle size={16} />}
          </span>
          <span>
            <span>
              <h2>{plan.name}</h2>
              <span className={`status-badge ${plan.enabled ? "success" : "neutral"}`}>
                {plan.enabled ? "已启用" : "已暂停"}
              </span>
            </span>
            <p>{plan.description || "未填写计划描述。"}</p>
          </span>
        </div>
        <div className="automation-v2-detail-actions">
          <button className="button secondary small" type="button" onClick={onToggle} disabled={busy === `toggle:${plan.id}`}>
            {busy === `toggle:${plan.id}` ? <LoaderCircle className="spin" size={13} /> : plan.enabled ? <PauseCircle size={13} /> : <Zap size={13} />}
            {plan.enabled ? "暂停" : "启用"}
          </button>
          <button className="button secondary small" type="button" onClick={onEdit}>
            <Pencil size={13} />
            编辑
          </button>
          <button className="button primary small" type="button" onClick={onRun} disabled={busy === `run:${plan.id}`}>
            {busy === `run:${plan.id}` ? <LoaderCircle className="spin" size={13} /> : <Play size={13} fill="currentColor" />}
            {isRunning && plan.concurrencyPolicy !== "parallel" ? "再次运行" : "立即运行"}
          </button>
        </div>
      </header>

      <div className="automation-v2-detail-scroll">
        <section className="automation-v2-info-grid">
          <InfoCard
            icon={<CalendarClock size={15} />}
            label="触发时间"
            value={formatSchedule(plan.schedule)}
            detail={plan.nextRunAt ? `下次 ${formatDate(plan.nextRunAt)}` : "计划已暂停"}
          />
          <InfoCard
            icon={<Package size={15} />}
            label="运行目标"
            value={packageItem?.name ?? plan.action.packageId}
            detail={profile?.name ?? (plan.action.entrypoint || "默认运行配置")}
          />
          <InfoCard
            icon={<Settings2 size={15} />}
            label="执行策略"
            value={concurrencyLabel(plan.concurrencyPolicy)}
            detail={`${plan.timeoutSeconds} 秒超时 · 最多 ${plan.retryPolicy.maxAttempts} 次`}
          />
          <InfoCard
            icon={<Send size={15} />}
            label="结果投递"
            value={`${plan.deliveryTargets.filter((target) => target.enabled).length} 个目标`}
            detail={plan.deliveryTargets.length ? plan.deliveryTargets.map((target) => target.name).join("、") : "不额外投递"}
          />
        </section>

        <section className="automation-v2-section">
          <header>
            <div>
              <Send size={14} />
              <span><strong>投递目标</strong><small>一次运行可以广播到多个渠道</small></span>
            </div>
            <span>{plan.deliveryTargets.length}</span>
          </header>
          {plan.deliveryTargets.length ? (
            <div className="automation-v2-delivery-summary">
              {plan.deliveryTargets.map((target) => (
                <span key={target.id || `${target.type}-${target.name}`} className={target.enabled ? "" : "disabled"}>
                  {deliveryIcon(target.type)}
                  <span><strong>{target.name}</strong><small>{deliveryKindLabel(target.type)}</small></span>
                  <em>{target.enabled ? "启用" : "停用"}</em>
                </span>
              ))}
            </div>
          ) : (
            <div className="automation-v2-inline-empty">运行结果只保留在本地历史中。</div>
          )}
        </section>

        <section className="automation-v2-section automation-v2-history">
          <header>
            <div>
              <History size={14} />
              <span><strong>运行历史</strong><small>最近 {Math.min(runs.length, 50)} 次尝试</small></span>
            </div>
            {runs.some((run) => run.status === "running" || run.status === "queued") && (
              <span className="automation-v2-live"><Activity size={11} /> 实时更新</span>
            )}
          </header>
          {runs.length ? (
            <div className="automation-v2-run-list">
              {runs.slice(0, 50).map((run) => (
                <article key={run.id} className={`automation-v2-run status-${run.status}`}>
                  <span className="automation-v2-run-icon">{runIcon(run.status)}</span>
                  <div>
                    <span>
                      <strong>{runStatusLabel(run.status)}</strong>
                      <em>{run.trigger === "manual" ? "手动触发" : "定时触发"}</em>
                      {run.attempt > 1 && <em>第 {run.attempt} 次尝试</em>}
                    </span>
                    <small>{run.error || runResultSummary(run)}</small>
                  </div>
                  <span className="automation-v2-run-time">
                    <strong>{formatDate(run.startedAt ?? run.queuedAt)}</strong>
                    <small>{formatDuration(run.startedAt, run.finishedAt)}</small>
                  </span>
                </article>
              ))}
            </div>
          ) : (
            <div className="automation-v2-inline-empty tall">
              <History size={20} />
              <span>还没有运行记录。点击“立即运行”验证当前配置。</span>
            </div>
          )}
        </section>

        <footer className="automation-v2-danger-zone">
          <span><strong>计划管理</strong><small>运行历史会独立保留用于审计。</small></span>
          <button className="button danger-ghost small" type="button" onClick={onDelete}>
            <Trash2 size={13} />
            删除计划
          </button>
        </footer>
      </div>
    </>
  );
}

function PlanEditor({
  draft,
  packages,
  busy,
  onChange,
  onCancel,
  onSave,
}: {
  draft: AutomationDraft;
  packages: PackageSummary[];
  busy: boolean;
  onChange: (draft: AutomationDraft) => void;
  onCancel: () => void;
  onSave: () => void;
}) {
  const selectedPackage = packages.find((item) => item.id === draft.action.packageId);

  function updateSchedule(patch: Partial<AutomationSchedule>) {
    onChange({ ...draft, schedule: { ...draft.schedule, ...patch } });
  }

  function updateDelivery(editorKey: string, patch: Partial<DraftDeliveryTarget>) {
    onChange({
      ...draft,
      deliveryTargets: draft.deliveryTargets.map((target) => (
        target.editorKey === editorKey ? { ...target, ...patch } : target
      )),
    });
  }

  function addDelivery() {
    onChange({
      ...draft,
      deliveryTargets: [
        ...draft.deliveryTargets,
        {
          id: "",
          editorKey: `delivery-${Date.now()}-${draft.deliveryTargets.length}`,
          type: "in-app",
          name: "应用内通知",
          enabled: true,
          configuration: {},
          configurationText: "{}",
        },
      ],
    });
  }

  return (
    <div className="automation-v2-dialog-backdrop" role="presentation" onMouseDown={(event) => {
      if (event.target === event.currentTarget && !busy) onCancel();
    }}>
      <form className="automation-v2-editor" role="dialog" aria-modal="true" aria-labelledby="automation-editor-title" onSubmit={(event) => {
        event.preventDefault();
        onSave();
      }}>
        <header>
          <span className="automation-v2-editor-mark"><CalendarClock size={18} /></span>
          <div>
            <h2 id="automation-editor-title">{draft.id ? "编辑自动化计划" : "新建自动化计划"}</h2>
            <p>配置何时运行、如何处理并发，以及结果送到哪里。</p>
          </div>
          <label className="automation-v2-enable">
            <input
              type="checkbox"
              checked={draft.enabled}
              onChange={(event) => onChange({ ...draft, enabled: event.target.checked })}
            />
            <span>{draft.enabled ? "创建后启用" : "先保存为暂停"}</span>
          </label>
          <button className="icon-button subtle" type="button" aria-label="关闭编辑器" onClick={onCancel} disabled={busy}>
            <X size={16} />
          </button>
        </header>

        <div className="automation-v2-editor-body">
          <section className="automation-v2-form-section">
            <header>
              <span>01</span>
              <div><strong>基础信息</strong><small>名称、RPAZ 包及运行配置</small></div>
            </header>
            <div className="automation-v2-field-grid two">
              <label className="automation-v2-field">
                <span>计划名称</span>
                <input
                  autoFocus
                  value={draft.name}
                  maxLength={120}
                  onChange={(event) => onChange({ ...draft, name: event.target.value })}
                  placeholder="例如：每日新闻摘要"
                />
              </label>
              <label className="automation-v2-field">
                <span>RPAZ 包</span>
                <select
                  value={draft.action.packageId}
                  onChange={(event) => {
                    const packageItem = packages.find((item) => item.id === event.target.value);
                    onChange({
                      ...draft,
                      action: {
                        ...draft.action,
                        packageId: event.target.value,
                        entrypoint: packageItem?.profiles[0]?.id ?? "",
                      },
                    });
                  }}
                >
                  <option value="">选择已安装的 RPAZ 包</option>
                  {packages.map((item) => <option key={item.id} value={item.id}>{item.name} · {item.version}</option>)}
                </select>
              </label>
              <label className="automation-v2-field full">
                <span>描述</span>
                <input
                  value={draft.description}
                  maxLength={4_000}
                  onChange={(event) => onChange({ ...draft, description: event.target.value })}
                  placeholder="说明这项自动化会做什么"
                />
              </label>
              <label className="automation-v2-field">
                <span>运行配置</span>
                <select
                  value={draft.action.entrypoint}
                  onChange={(event) => onChange({ ...draft, action: { ...draft.action, entrypoint: event.target.value } })}
                  disabled={!selectedPackage}
                >
                  {!selectedPackage?.profiles.length && <option value="">默认运行配置</option>}
                  {selectedPackage?.profiles.map((profile) => <option key={profile.id} value={profile.id}>{profile.name}</option>)}
                </select>
              </label>
              <label className="automation-v2-field">
                <span>参数 JSON</span>
                <textarea
                  rows={3}
                  spellCheck={false}
                  value={draft.parametersText}
                  onChange={(event) => onChange({ ...draft, parametersText: event.target.value })}
                  placeholder={'{"topic": "今日新闻"}'}
                />
              </label>
            </div>
          </section>

          <section className="automation-v2-form-section">
            <header>
              <span>02</span>
              <div><strong>运行时间</strong><small>使用本机时区，精确到分钟</small></div>
            </header>
            <div className="automation-v2-schedule-tabs" role="tablist" aria-label="计划类型">
              {([
                ["daily", "每日"],
                ["weekly", "每周"],
                ["interval", "间隔"],
                ["cron", "Cron"],
              ] as Array<[ScheduleKind, string]>).map(([kind, label]) => (
                <button
                  key={kind}
                  type="button"
                  role="tab"
                  aria-selected={draft.schedule.type === kind}
                  className={draft.schedule.type === kind ? "active" : ""}
                  onClick={() => updateSchedule({ type: kind })}
                >
                  {label}
                </button>
              ))}
            </div>
            <div className="automation-v2-schedule-config">
              {draft.schedule.type === "daily" && (
                <label className="automation-v2-field">
                  <span>每天运行时间</span>
                  <input type="time" value={draft.schedule.time} onChange={(event) => updateSchedule({ time: event.target.value })} />
                </label>
              )}
              {draft.schedule.type === "weekly" && (
                <>
                  <label className="automation-v2-field">
                    <span>运行时间</span>
                    <input type="time" value={draft.schedule.time} onChange={(event) => updateSchedule({ time: event.target.value })} />
                  </label>
                  <div className="automation-v2-weekdays">
                    <span>运行日</span>
                    <div>
                      {weekDays.map((day) => (
                        <button
                          key={day.value}
                          type="button"
                          className={draft.schedule.daysOfWeek.includes(day.value) ? "active" : ""}
                          aria-pressed={draft.schedule.daysOfWeek.includes(day.value)}
                          onClick={() => updateSchedule({
                            daysOfWeek: draft.schedule.daysOfWeek.includes(day.value)
                              ? draft.schedule.daysOfWeek.filter((value) => value !== day.value)
                              : [...draft.schedule.daysOfWeek, day.value],
                          })}
                        >
                          {day.label}
                        </button>
                      ))}
                    </div>
                  </div>
                </>
              )}
              {draft.schedule.type === "interval" && (
                <label className="automation-v2-field">
                  <span>每隔多少分钟</span>
                  <div className="automation-v2-number-suffix">
                    <input
                      type="number"
                      min={1}
                      max={525_600}
                      value={draft.schedule.intervalMinutes}
                      onChange={(event) => updateSchedule({ intervalMinutes: Number(event.target.value) })}
                    />
                    <span>分钟</span>
                  </div>
                </label>
              )}
              {draft.schedule.type === "cron" && (
                <label className="automation-v2-field automation-v2-cron-field">
                  <span>5 段 Cron 表达式</span>
                  <input
                    className="tabular"
                    value={draft.schedule.cron}
                    onChange={(event) => updateSchedule({ cron: event.target.value })}
                    placeholder="0 8 * * 1-5"
                  />
                  <small>分　时　日　月　星期；支持 *、列表、范围和步长。</small>
                </label>
              )}
              <div className="automation-v2-schedule-preview">
                <Clock3 size={14} />
                <span><small>当前规则</small><strong>{formatSchedule(draft.schedule)}</strong></span>
              </div>
            </div>
          </section>

          <section className="automation-v2-form-section">
            <header>
              <span>03</span>
              <div><strong>可靠性策略</strong><small>并发、超时和失败重试</small></div>
            </header>
            <div className="automation-v2-policy-grid">
              {([
                ["skip", "跳过", "已有实例运行时不再启动"],
                ["queue", "排队", "等待前一个实例完成"],
                ["parallel", "并行", "允许多个实例同时运行"],
              ] as Array<[ConcurrencyPolicy, string, string]>).map(([policy, title, description]) => (
                <button
                  key={policy}
                  type="button"
                  className={draft.concurrencyPolicy === policy ? "active" : ""}
                  onClick={() => onChange({ ...draft, concurrencyPolicy: policy })}
                >
                  <span>{policyIcon(policy)}</span>
                  <span><strong>{title}</strong><small>{description}</small></span>
                  <span className="automation-v2-radio" />
                </button>
              ))}
            </div>
            <div className="automation-v2-field-grid four">
              <label className="automation-v2-field">
                <span>超时</span>
                <div className="automation-v2-number-suffix">
                  <input type="number" min={1} max={86_400} value={draft.timeoutSeconds} onChange={(event) => onChange({ ...draft, timeoutSeconds: Number(event.target.value) })} />
                  <span>秒</span>
                </div>
              </label>
              <label className="automation-v2-field">
                <span>最多尝试</span>
                <div className="automation-v2-number-suffix">
                  <input type="number" min={1} max={20} value={draft.retryPolicy.maxAttempts} onChange={(event) => onChange({ ...draft, retryPolicy: { ...draft.retryPolicy, maxAttempts: Number(event.target.value) } })} />
                  <span>次</span>
                </div>
              </label>
              <label className="automation-v2-field">
                <span>重试延迟</span>
                <div className="automation-v2-number-suffix">
                  <input type="number" min={0} max={86_400} value={draft.retryPolicy.delaySeconds} onChange={(event) => onChange({ ...draft, retryPolicy: { ...draft.retryPolicy, delaySeconds: Number(event.target.value) } })} />
                  <span>秒</span>
                </div>
              </label>
              <label className="automation-v2-field">
                <span>退避倍数</span>
                <div className="automation-v2-number-suffix">
                  <input type="number" min={1} max={10} step={0.5} value={draft.retryPolicy.backoffMultiplier} onChange={(event) => onChange({ ...draft, retryPolicy: { ...draft.retryPolicy, backoffMultiplier: Number(event.target.value) } })} />
                  <span>×</span>
                </div>
              </label>
            </div>
          </section>

          <section className="automation-v2-form-section">
            <header>
              <span>04</span>
              <div><strong>多目标投递</strong><small>RPAZ 运行结果可同时发送到多个位置</small></div>
              <button className="button secondary small" type="button" onClick={addDelivery}>
                <Plus size={12} />
                添加目标
              </button>
            </header>
            {draft.deliveryTargets.length ? (
              <div className="automation-v2-delivery-editors">
                {draft.deliveryTargets.map((target, index) => (
                  <article key={target.editorKey} className={target.enabled ? "" : "disabled"}>
                    <header>
                      <span className="automation-v2-delivery-index">{String(index + 1).padStart(2, "0")}</span>
                      <strong>{target.name || "未命名目标"}</strong>
                      <label className="automation-v2-mini-toggle">
                        <input type="checkbox" checked={target.enabled} onChange={(event) => updateDelivery(target.editorKey, { enabled: event.target.checked })} />
                        <span>{target.enabled ? "启用" : "停用"}</span>
                      </label>
                      <button className="icon-button subtle" type="button" aria-label={`删除投递目标 ${target.name}`} onClick={() => onChange({ ...draft, deliveryTargets: draft.deliveryTargets.filter((item) => item.editorKey !== target.editorKey) })}>
                        <Trash2 size={13} />
                      </button>
                    </header>
                    <div className="automation-v2-field-grid three">
                      <label className="automation-v2-field">
                        <span>类型</span>
                        <select value={target.type} onChange={(event) => updateDelivery(target.editorKey, { type: event.target.value })}>
                          {deliveryKinds.map((kind) => <option key={kind.value} value={kind.value}>{kind.label}</option>)}
                        </select>
                      </label>
                      <label className="automation-v2-field">
                        <span>名称</span>
                        <input value={target.name} maxLength={120} onChange={(event) => updateDelivery(target.editorKey, { name: event.target.value })} placeholder="例如：运营频道" />
                      </label>
                      <label className="automation-v2-field">
                        <span>目标配置 JSON</span>
                        <textarea rows={2} spellCheck={false} value={target.configurationText} onChange={(event) => updateDelivery(target.editorKey, { configurationText: event.target.value })} placeholder={'{"url": "https://…"}'} />
                      </label>
                    </div>
                  </article>
                ))}
              </div>
            ) : (
              <button className="automation-v2-add-delivery" type="button" onClick={addDelivery}>
                <Plus size={16} />
                <span><strong>添加第一个投递目标</strong><small>也可以不投递，只在本地保留运行结果。</small></span>
              </button>
            )}
          </section>
        </div>

        <footer>
          <span><ShieldCheck size={13} /> 计划与历史仅保存在当前工作区</span>
          <button className="button ghost" type="button" onClick={onCancel} disabled={busy}>取消</button>
          <button className="button primary" type="submit" disabled={busy}>
            {busy ? <LoaderCircle className="spin" size={14} /> : <CheckCircle2 size={14} />}
            {busy ? "正在保存…" : draft.id ? "保存更改" : "创建计划"}
          </button>
        </footer>
      </form>
    </div>
  );
}

function Metric({
  icon,
  label,
  value,
  tone,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  tone: string;
}) {
  return (
    <article className={`automation-v2-metric tone-${tone}`}>
      <span>{icon}</span>
      <div><strong>{value}</strong><small>{label}</small></div>
    </article>
  );
}

function InfoCard({
  icon,
  label,
  value,
  detail,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  detail: string;
}) {
  return (
    <article className="automation-v2-info-card">
      <span>{icon}</span>
      <div><small>{label}</small><strong>{value}</strong><em title={detail}>{detail}</em></div>
    </article>
  );
}

function RunDot({ status }: { status: RunStatus }) {
  return <span className={`automation-v2-run-dot status-${status}`} title={runStatusLabel(status)} />;
}

function legacyPlans(items: AutomationSummary[], packages: PackageSummary[]): AutomationPlan[] {
  return items.map((item, index) => {
    const packageItem = packages.find((candidate) => candidate.name === item.packageName);
    const profile = packageItem?.profiles.find((candidate) => candidate.name === item.profileName);
    const attempts = Number(item.retryPolicy.match(/\d+/)?.[0] ?? 1);
    const concurrencyPolicy: ConcurrencyPolicy = item.concurrencyPolicy === "allow"
      ? "parallel"
      : item.concurrencyPolicy === "queueOne"
        ? "queue"
        : "skip";
    return {
      id: item.id,
      name: item.name,
      description: item.health,
      enabled: item.status !== "paused",
      schedule: {
        type: "cron",
        cron: "0 8 * * *",
        time: "08:00",
        daysOfWeek: [],
        intervalMinutes: 60,
        displayLabel: item.triggerLabel,
      },
      action: {
        type: "rpaz-package",
        packageId: packageItem?.id ?? item.packageName,
        entrypoint: profile?.id ?? "",
        parameters: {},
      },
      concurrencyPolicy,
      retryPolicy: {
        maxAttempts: Math.max(1, attempts),
        delaySeconds: item.retryPolicy.includes("5 分钟") ? 300 : 10,
        backoffMultiplier: item.retryPolicy.includes("指数") ? 2 : 1,
      },
      timeoutSeconds: 600,
      deliveryTargets: [],
      createdAt: Date.now() - (index + 1) * 60_000,
      updatedAt: Date.now(),
      lastRunAt: null,
      nextRunAt: parseLegacyNextRun(item.nextRun),
      lastScheduledMinute: null,
    };
  });
}

function parseLegacyNextRun(value: string): number | null {
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? null : parsed;
}

function createTemplateDraft(kind: TemplateKind, packageItem?: PackageSummary): AutomationDraft {
  const common: AutomationDraft = {
    id: "",
    name: "",
    description: "",
    enabled: true,
    schedule: {
      type: "daily",
      cron: "0 8 * * *",
      time: "08:00",
      daysOfWeek: [1],
      intervalMinutes: 60,
    },
    action: {
      type: "rpaz-package",
      packageId: packageItem?.id ?? "",
      entrypoint: packageItem?.profiles[0]?.id ?? "",
      parameters: {},
    },
    parametersText: "{}",
    concurrencyPolicy: "skip",
    retryPolicy: {
      maxAttempts: 2,
      delaySeconds: 30,
      backoffMultiplier: 2,
    },
    timeoutSeconds: 600,
    deliveryTargets: [],
  };
  if (kind === "news") {
    return {
      ...common,
      name: "每日新闻摘要",
      description: "每天早上生成带来源引用的新闻摘要，并发送应用内通知。",
      parametersText: JSON.stringify({ task: "生成今日新闻摘要", language: "zh-CN" }, null, 2),
      deliveryTargets: [draftTarget("in-app", "DRPA 通知", {})],
    };
  }
  if (kind === "report") {
    return {
      ...common,
      name: "每周报告生成",
      description: "每周一生成上周工作或业务报告并保存到文件。",
      schedule: { ...common.schedule, type: "weekly", time: "09:00", daysOfWeek: [1] },
      parametersText: JSON.stringify({ task: "生成上周总结报告", format: "docx" }, null, 2),
      timeoutSeconds: 1_200,
      retryPolicy: { maxAttempts: 3, delaySeconds: 60, backoffMultiplier: 2 },
      concurrencyPolicy: "queue",
      deliveryTargets: [draftTarget("file", "报告归档", { directory: "reports" })],
    };
  }
  if (kind === "broadcast") {
    return {
      ...common,
      name: "工作日多频道广播",
      description: "每个工作日汇总最新内容，并广播到邮件、Webhook 和消息频道。",
      schedule: { ...common.schedule, type: "cron", cron: "0 18 * * 1-5" },
      parametersText: JSON.stringify({ task: "生成并广播今日简报" }, null, 2),
      concurrencyPolicy: "queue",
      deliveryTargets: [
        draftTarget("email", "邮件订阅组", { recipients: [] }),
        draftTarget("webhook", "业务 Webhook", { url: "" }),
        draftTarget("channel", "运营频道", { channel: "" }),
      ],
    };
  }
  return {
    ...common,
    name: packageItem ? `${packageItem.name} 定时运行` : "RPAZ 定时运行",
    description: "按指定时间自动运行已安装的 RPAZ 包。",
    schedule: { ...common.schedule, type: "interval", intervalMinutes: 60 },
  };
}

function draftTarget(type: string, name: string, configuration: Record<string, unknown>): DraftDeliveryTarget {
  return {
    id: "",
    editorKey: `delivery-${type}-${Math.random().toString(36).slice(2)}`,
    type,
    name,
    enabled: true,
    configuration,
    configurationText: JSON.stringify(configuration, null, 2),
  };
}

function planToDraft(plan: AutomationPlan): AutomationDraft {
  return {
    id: plan.id,
    name: plan.name,
    description: plan.description,
    enabled: plan.enabled,
    schedule: { ...plan.schedule, daysOfWeek: [...plan.schedule.daysOfWeek] },
    action: { ...plan.action },
    parametersText: JSON.stringify(plan.action.parameters ?? {}, null, 2),
    concurrencyPolicy: plan.concurrencyPolicy,
    retryPolicy: { ...plan.retryPolicy },
    timeoutSeconds: plan.timeoutSeconds,
    deliveryTargets: plan.deliveryTargets.map((target, index) => ({
      ...target,
      configuration: { ...target.configuration },
      configurationText: JSON.stringify(target.configuration ?? {}, null, 2),
      editorKey: target.id || `delivery-existing-${index}`,
    })),
  };
}

function validateAndBuildInput(draft: AutomationDraft): AutomationPlanInput {
  if (!draft.name.trim()) throw new Error("请填写计划名称。");
  if (!draft.action.packageId) throw new Error("请选择要运行的 RPAZ 包。");
  if (draft.schedule.type === "cron" && draft.schedule.cron.trim().split(/\s+/).length !== 5) {
    throw new Error("Cron 表达式必须包含 5 段：分、时、日、月、星期。");
  }
  if ((draft.schedule.type === "daily" || draft.schedule.type === "weekly") && !draft.schedule.time) {
    throw new Error("请选择运行时间。");
  }
  if (draft.schedule.type === "weekly" && draft.schedule.daysOfWeek.length === 0) {
    throw new Error("每周计划至少选择一天。");
  }
  if (draft.schedule.type === "interval" && !(draft.schedule.intervalMinutes >= 1)) {
    throw new Error("运行间隔至少为 1 分钟。");
  }
  const parameters = parseJson(draft.parametersText, "任务参数");
  if (!isRecord(parameters)) throw new Error("任务参数必须是 JSON 对象。");
  const deliveryTargets = draft.deliveryTargets.map((target) => {
    if (!target.name.trim()) throw new Error("每个投递目标都需要名称。");
    const configuration = parseJson(target.configurationText, `投递目标“${target.name}”的配置`);
    if (!isRecord(configuration)) throw new Error(`投递目标“${target.name}”的配置必须是 JSON 对象。`);
    return {
      id: target.id,
      type: target.type,
      name: target.name.trim(),
      enabled: target.enabled,
      configuration,
    };
  });
  return {
    id: draft.id,
    name: draft.name.trim(),
    description: draft.description.trim(),
    enabled: draft.enabled,
    schedule: {
      type: draft.schedule.type,
      cron: draft.schedule.cron.trim(),
      time: draft.schedule.time,
      daysOfWeek: [...new Set(draft.schedule.daysOfWeek)],
      intervalMinutes: draft.schedule.intervalMinutes,
    },
    action: {
      ...draft.action,
      parameters,
    },
    concurrencyPolicy: draft.concurrencyPolicy,
    retryPolicy: { ...draft.retryPolicy },
    timeoutSeconds: draft.timeoutSeconds,
    deliveryTargets,
  };
}

function parseJson(source: string, label: string): unknown {
  try {
    return JSON.parse(source || "{}") as unknown;
  } catch {
    throw new Error(`${label}不是有效的 JSON。`);
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function formatSchedule(schedule: AutomationSchedule): string {
  if (schedule.displayLabel) return schedule.displayLabel;
  if (schedule.type === "daily") return `每天 ${schedule.time || "--:--"}`;
  if (schedule.type === "weekly") {
    const days = weekDays
      .filter((day) => schedule.daysOfWeek.includes(day.value))
      .map((day) => `周${day.label}`)
      .join("、");
    return `${days || "未选择日期"} ${schedule.time || "--:--"}`;
  }
  if (schedule.type === "interval") {
    if (schedule.intervalMinutes % 1_440 === 0) return `每 ${schedule.intervalMinutes / 1_440} 天`;
    if (schedule.intervalMinutes % 60 === 0) return `每 ${schedule.intervalMinutes / 60} 小时`;
    return `每 ${schedule.intervalMinutes || 0} 分钟`;
  }
  return `Cron · ${schedule.cron || "未配置"}`;
}

function formatDate(value?: number | null, fallback = "—"): string {
  if (!value) return fallback;
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return fallback;
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).format(date);
}

function formatDuration(start?: number | null, finish?: number | null): string {
  if (!start) return "等待开始";
  if (!finish) return "进行中";
  const seconds = Math.max(0, Math.round((finish - start) / 1_000));
  if (seconds < 60) return `${seconds} 秒`;
  const minutes = Math.floor(seconds / 60);
  return `${minutes} 分 ${seconds % 60} 秒`;
}

function runResultSummary(run: AutomationRun): string {
  if (run.status === "queued") return "正在等待可用运行槽位";
  if (run.status === "running") return "RPAZ 包正在执行";
  if (run.status === "skipped") return "并发策略阻止了重复运行";
  if (run.deliveryResults.length) {
    return `${run.deliveryResults.length} 个投递目标已处理`;
  }
  return run.status === "succeeded" ? "运行成功，结果已写入本地历史" : "运行未完成";
}

function concurrencyLabel(policy: ConcurrencyPolicy): string {
  return {
    skip: "冲突时跳过",
    queue: "依次排队",
    parallel: "允许并行",
  }[policy];
}

function runStatusLabel(status: RunStatus): string {
  return {
    queued: "排队中",
    running: "运行中",
    succeeded: "运行成功",
    failed: "运行失败",
    skipped: "已跳过",
  }[status];
}

function deliveryKindLabel(kind: string): string {
  return deliveryKinds.find((item) => item.value === kind)?.label ?? kind;
}

function templateIcon(kind: TemplateKind) {
  if (kind === "news") return <Newspaper size={17} />;
  if (kind === "report") return <FileBarChart size={17} />;
  if (kind === "broadcast") return <RadioTower size={17} />;
  return <Package size={17} />;
}

function deliveryIcon(kind: string) {
  if (kind === "email") return <Mail size={14} />;
  if (kind === "webhook") return <Webhook size={14} />;
  if (kind === "channel") return <MessageSquareMore size={14} />;
  if (kind === "file") return <FileBarChart size={14} />;
  return <Activity size={14} />;
}

function policyIcon(policy: ConcurrencyPolicy) {
  if (policy === "queue") return <ArrowRight size={15} />;
  if (policy === "parallel") return <Activity size={15} />;
  return <ShieldCheck size={15} />;
}

function runIcon(status: RunStatus) {
  if (status === "running" || status === "queued") return <LoaderCircle className="spin" size={14} />;
  if (status === "succeeded") return <CheckCircle2 size={14} />;
  if (status === "failed") return <AlertCircle size={14} />;
  return <RotateCcw size={14} />;
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  return typeof error === "string" ? error : "操作失败，请检查计划配置后重试。";
}
