import { describe, expect, it } from "vitest";

import type { DashboardWidget } from "../../domain/models";
import { compactDashboardWidgets, moveWidgetAndReflow, projectResponsiveLayouts } from "./dashboardLayout";

function widget(id: string, layout: DashboardWidget["layout"]): DashboardWidget {
  return {
    id,
    title: id,
    kind: "markdown",
    layout,
    encoding: { categoryField: "", valueField: "", seriesField: "" },
    options: { text: id, numberFormat: "text", color: "#4f6bed", showLegend: false, refreshSeconds: 0 },
  };
}

function overlaps(left: DashboardWidget["layout"], right: DashboardWidget["layout"]) {
  return left.x < right.x + right.w
    && left.x + left.w > right.x
    && left.y < right.y + right.h
    && left.y + left.h > right.y;
}

describe("dashboard adaptive layout", () => {
  it("moves an occupied widget into the vacated slot like a launcher", () => {
    const result = moveWidgetAndReflow([
      widget("moving", { x: 0, y: 0, w: 2, h: 2 }),
      widget("occupied", { x: 2, y: 0, w: 2, h: 2 }),
      widget("stable", { x: 4, y: 0, w: 2, h: 2 }),
    ], "moving", { x: 2, y: 0, w: 2, h: 2 }, 6);

    expect(result.find((item) => item.id === "moving")?.layout).toEqual({ x: 2, y: 0, w: 2, h: 2 });
    expect(result.find((item) => item.id === "occupied")?.layout).toEqual({ x: 0, y: 0, w: 2, h: 2 });
    expect(result.find((item) => item.id === "stable")?.layout).toEqual({ x: 4, y: 0, w: 2, h: 2 });
  });

  it("keeps every widget in bounds and collision-free after adaptive reflow", () => {
    const result = moveWidgetAndReflow([
      widget("moving", { x: 0, y: 0, w: 3, h: 2 }),
      widget("a", { x: 3, y: 0, w: 3, h: 2 }),
      widget("b", { x: 0, y: 2, w: 4, h: 3 }),
      widget("c", { x: 4, y: 2, w: 2, h: 3 }),
    ], "moving", { x: 3, y: 1, w: 3, h: 2 }, 6);

    for (const item of result) {
      expect(item.layout.x).toBeGreaterThanOrEqual(0);
      expect(item.layout.x + item.layout.w).toBeLessThanOrEqual(6);
      expect(item.layout.y).toBeGreaterThanOrEqual(0);
    }
    for (let left = 0; left < result.length; left += 1) {
      for (let right = left + 1; right < result.length; right += 1) {
        expect(overlaps(result[left].layout, result[right].layout)).toBe(false);
      }
    }
  });

  it("projects a dashboard to fewer columns without overlaps", () => {
    const widgets = [
      widget("a", { x: 0, y: 0, w: 6, h: 3 }),
      widget("b", { x: 6, y: 0, w: 6, h: 3 }),
      widget("c", { x: 0, y: 3, w: 4, h: 2 }),
    ];
    const projected = projectResponsiveLayouts(widgets, 12, 4);
    const layouts = Object.values(projected);

    for (const layout of layouts) expect(layout.x + layout.w).toBeLessThanOrEqual(4);
    for (let left = 0; left < layouts.length; left += 1) {
      for (let right = left + 1; right < layouts.length; right += 1) {
        expect(overlaps(layouts[left], layouts[right])).toBe(false);
      }
    }
  });

  it("closes vertical gaps after a widget is removed", () => {
    const compacted = compactDashboardWidgets([
      widget("top", { x: 0, y: 0, w: 3, h: 2 }),
      widget("lower", { x: 0, y: 5, w: 3, h: 2 }),
    ], 6);

    expect(compacted.find((item) => item.id === "lower")?.layout.y).toBe(2);
  });
});
