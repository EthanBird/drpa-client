import type {
  DashboardDataSource,
  DashboardDataset,
  DashboardWidget,
  DatabaseQueryResult,
  WorkspaceSnapshot,
} from "../../domain/models";
import type { DesktopGateway } from "../../infra/gateway";

export interface DashboardRuntimeContext {
  snapshot: WorkspaceSnapshot;
  gateway: DesktopGateway;
  profilePasswords: Record<string, string>;
}

export interface DashboardDataAdapter {
  readonly kind: DashboardDataSource["kind"];
  load(source: DashboardDataSource, context: DashboardRuntimeContext): Promise<DashboardDataset>;
}

export class DashboardDataRuntime {
  private readonly adapters = new Map<DashboardDataSource["kind"], DashboardDataAdapter>();

  register(adapter: DashboardDataAdapter): DashboardDataRuntime {
    this.adapters.set(adapter.kind, adapter);
    return this;
  }

  async load(widget: DashboardWidget, context: DashboardRuntimeContext): Promise<DashboardDataset> {
    if (!widget.source) return emptyDataset();
    const adapter = this.adapters.get(widget.source.kind);
    if (!adapter) throw new Error(`未注册 BI 数据适配器：${widget.source.kind}`);
    return adapter.load(widget.source, context);
  }
}

class BuiltinDashboardAdapter implements DashboardDataAdapter {
  readonly kind = "builtin" as const;

  async load(source: DashboardDataSource, context: DashboardRuntimeContext): Promise<DashboardDataset> {
    if (source.kind !== "builtin") throw new Error("DRPA 内置数据适配器收到错误的数据源");
    const started = performance.now();
    const snapshot = context.snapshot;
    let rows: Array<Record<string, unknown>>;
    switch (source.dataset) {
      case "workspaceSummary":
        rows = [{
          activeRuns: snapshot.stats.activeRuns,
          successRate: snapshot.stats.successRate,
          packages: snapshot.stats.packages,
          savedHours: snapshot.stats.savedHours,
          totalRuns: snapshot.runs.length,
        }];
        break;
      case "runHistory":
        rows = snapshot.runs.map((run) => ({
          id: run.id,
          packageName: run.packageName,
          profileName: run.profileName,
          status: run.status,
          statusLabel: statusLabel(run.status),
          startedAt: run.startedAt,
          duration: run.duration,
          durationSeconds: Math.round((run.durationMs ?? parseDuration(run.duration)) / 100) / 10,
          progress: run.progress ?? (run.status === "success" ? 100 : 0),
        }));
        break;
      case "runStatus": {
        const counts = new Map<string, number>();
        for (const run of snapshot.runs) counts.set(statusLabel(run.status), (counts.get(statusLabel(run.status)) ?? 0) + 1);
        rows = [...counts].map(([status, count]) => ({ status, count }));
        break;
      }
      case "packages":
        rows = snapshot.packages.map((item) => ({
          id: item.id,
          name: item.name,
          version: item.version,
          runtime: item.runtime,
          trust: item.trust,
          profiles: item.profiles.length,
        }));
        break;
      default:
        rows = [];
    }
    return {
      columns: rows.length ? Object.keys(rows[0]) : builtinColumns(source.dataset),
      rows,
      durationMs: Math.max(0, Math.round(performance.now() - started)),
      truncated: false,
    };
  }
}

class DatabaseDashboardAdapter implements DashboardDataAdapter {
  readonly kind = "database" as const;

  async load(source: DashboardDataSource, context: DashboardRuntimeContext): Promise<DashboardDataset> {
    if (source.kind !== "database") throw new Error("数据库适配器收到错误的数据源");
    const password = context.profilePasswords[source.profileId] ?? "";
    const result = await context.gateway.executeDashboardDatabaseQuery(source.profileId, password, source.sql);
    return queryResultToDataset(result);
  }
}

export const dashboardRuntime = new DashboardDataRuntime()
  .register(new BuiltinDashboardAdapter())
  .register(new DatabaseDashboardAdapter());

export function queryResultToDataset(result: DatabaseQueryResult): DashboardDataset {
  return {
    columns: result.columns,
    rows: result.rows.map((row) => Object.fromEntries(result.columns.map((column, index) => [column, row[index]]))),
    durationMs: result.durationMs,
    truncated: result.truncated,
  };
}

export function emptyDataset(): DashboardDataset {
  return { columns: [], rows: [], durationMs: 0, truncated: false };
}

function builtinColumns(dataset: string): string[] {
  switch (dataset) {
    case "workspaceSummary": return ["activeRuns", "successRate", "packages", "savedHours", "totalRuns"];
    case "runHistory": return ["packageName", "profileName", "status", "startedAt", "duration", "durationSeconds"];
    case "runStatus": return ["status", "count"];
    case "packages": return ["name", "version", "runtime", "trust", "profiles"];
    default: return [];
  }
}

function statusLabel(status: string): string {
  return ({
    running: "运行中",
    queued: "排队中",
    success: "成功",
    failed: "失败",
    cancelled: "已取消",
    interrupted: "已中断",
  } as Record<string, string>)[status] ?? status;
}

function parseDuration(value: string): number {
  const normalized = value.trim().toLowerCase();
  const milliseconds = normalized.match(/^([\d.]+)\s*ms$/);
  if (milliseconds) return Number(milliseconds[1]);
  const seconds = normalized.match(/^([\d.]+)\s*s$/);
  if (seconds) return Number(seconds[1]) * 1_000;
  const minutes = normalized.match(/^([\d.]+)\s*m(?:in)?$/);
  if (minutes) return Number(minutes[1]) * 60_000;
  return 0;
}
