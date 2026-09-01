import { describe, expect, it, vi } from "vitest";

import type { DashboardWidget, WorkspaceSnapshot } from "../../domain/models";
import type { DesktopGateway } from "../../infra/gateway";
import { dashboardRuntime } from "./dashboardRuntime";

const snapshot: WorkspaceSnapshot = {
  packages: [],
  automations: [],
  logs: [],
  stats: { activeRuns: 2, successRate: 95.5, packages: 4, savedHours: 13.5 },
  runs: [
    { id: "run-1", packageName: "A", profileName: "默认", status: "success", startedAt: "10:00", duration: "1.5s", durationMs: 1_500 },
    { id: "run-2", packageName: "B", profileName: "默认", status: "failed", startedAt: "11:00", duration: "800ms", durationMs: 800 },
    { id: "run-3", packageName: "A", profileName: "每日", status: "success", startedAt: "12:00", duration: "2s", durationMs: 2_000 },
  ],
};

function widget(source: DashboardWidget["source"]): DashboardWidget {
  return {
    id: "widget-test",
    title: "测试",
    kind: "bar",
    layout: { x: 0, y: 0, w: 4, h: 4 },
    source,
    encoding: { categoryField: "status", valueField: "count", seriesField: "" },
    options: { text: "", numberFormat: "number", color: "#4f6bed", showLegend: true, refreshSeconds: 0 },
  };
}

describe("dashboard data runtime", () => {
  it("adapts DRPA run state into chart-ready records", async () => {
    const result = await dashboardRuntime.load(widget({ kind: "builtin", dataset: "runStatus" }), {
      snapshot,
      gateway: {} as DesktopGateway,
      profilePasswords: {},
    });

    expect(result.columns).toEqual(["status", "count"]);
    expect(result.rows).toEqual(expect.arrayContaining([
      { status: "成功", count: 2 },
      { status: "失败", count: 1 },
    ]));
  });

  it("routes data-workbench widgets through the read-only dashboard query", async () => {
    const executeDashboardDatabaseQuery = vi.fn().mockResolvedValue({
      columns: ["month", "amount"],
      rows: [["一月", 42]],
      affectedRows: 0,
      durationMs: 7,
      truncated: false,
      statementType: "SELECT",
    });
    const gateway = { executeDashboardDatabaseQuery } as unknown as DesktopGateway;
    const result = await dashboardRuntime.load(widget({ kind: "database", profileId: "sales", sql: "SELECT month, amount FROM sales" }), {
      snapshot,
      gateway,
      profilePasswords: { sales: "session-secret" },
    });

    expect(executeDashboardDatabaseQuery).toHaveBeenCalledWith("sales", "session-secret", "SELECT month, amount FROM sales");
    expect(result.rows).toEqual([{ month: "一月", amount: 42 }]);
  });
});
