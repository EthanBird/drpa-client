import { describe, expect, it } from "vitest";

import type { DashboardWidget, DashboardWidgetKind } from "../../domain/models";
import {
  compactDashboardWidgets,
  moveWidgetAndReflow,
  projectResponsiveLayouts,
  resizeWidgetAndReflow,
  translateResponsiveDrag,
} from "./dashboardLayout";

function widget(id: string, layout: DashboardWidget["layout"], kind: DashboardWidgetKind = "markdown"): DashboardWidget {
  return {
    id,
    title: id,
    kind,
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
  it("changes the linear order and repacks every following card after a drag", () => {
    const result = moveWidgetAndReflow([
      widget("moving", { x: 0, y: 0, w: 2, h: 2 }),
      widget("second", { x: 2, y: 0, w: 2, h: 2 }),
      widget("third", { x: 4, y: 0, w: 2, h: 2 }),
      widget("fourth", { x: 0, y: 2, w: 2, h: 2 }),
    ], "moving", { x: 4, y: 0, w: 2, h: 2 }, 6);

    expect(result.map((item) => item.id)).toEqual(["second", "third", "moving", "fourth"]);
    expect(result.map((item) => item.layout)).toEqual([
      { x: 0, y: 0, w: 2, h: 2 },
      { x: 2, y: 0, w: 2, h: 2 },
      { x: 4, y: 0, w: 2, h: 2 },
      { x: 0, y: 2, w: 2, h: 2 },
    ]);
  });

  it("inserts a later card into the requested order instead of swapping two cards", () => {
    const result = moveWidgetAndReflow([
      widget("first", { x: 0, y: 0, w: 2, h: 2 }),
      widget("second", { x: 2, y: 0, w: 2, h: 2 }),
      widget("third", { x: 4, y: 0, w: 2, h: 2 }),
      widget("moving", { x: 0, y: 2, w: 2, h: 2 }),
    ], "moving", { x: 2, y: 0, w: 2, h: 2 }, 6);

    expect(result.map((item) => item.id)).toEqual(["first", "moving", "second", "third"]);
    expect(result.map((item) => item.layout)).toEqual([
      { x: 0, y: 0, w: 2, h: 2 },
      { x: 2, y: 0, w: 2, h: 2 },
      { x: 4, y: 0, w: 2, h: 2 },
      { x: 0, y: 2, w: 2, h: 2 },
    ]);
  });

  it("packs mixed card sizes from the array order instead of their stale coordinates", () => {
    const result = compactDashboardWidgets([
      widget("wide", { x: 2, y: 8, w: 4, h: 2 }),
      widget("small", { x: 0, y: 0, w: 2, h: 2 }),
      widget("lower-left", { x: 3, y: 3, w: 3, h: 2 }),
      widget("lower-right", { x: 0, y: 3, w: 3, h: 2 }),
    ], 6);

    expect(result.map((item) => item.id)).toEqual(["wide", "small", "lower-left", "lower-right"]);
    expect(result.map((item) => item.layout)).toEqual([
      { x: 0, y: 0, w: 4, h: 2 },
      { x: 4, y: 0, w: 2, h: 2 },
      { x: 0, y: 2, w: 3, h: 2 },
      { x: 3, y: 2, w: 3, h: 2 },
    ]);
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

  it("keeps the established order while a resized card triggers a fresh packing pass", () => {
    const result = resizeWidgetAndReflow([
      widget("first", { x: 0, y: 0, w: 2, h: 2 }),
      widget("resized", { x: 2, y: 0, w: 2, h: 2 }),
      widget("third", { x: 4, y: 0, w: 2, h: 2 }),
    ], "resized", { x: 2, y: 0, w: 4, h: 2 }, 6);

    expect(result.map((item) => item.id)).toEqual(["first", "resized", "third"]);
    expect(result.map((item) => item.layout)).toEqual([
      { x: 0, y: 0, w: 2, h: 2 },
      { x: 2, y: 0, w: 4, h: 2 },
      { x: 0, y: 2, w: 2, h: 2 },
    ]);
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

  it("projects both horizontal edges so responsive position and size stay synchronized", () => {
    const projected = projectResponsiveLayouts([
      widget("left", { x: 0, y: 0, w: 3, h: 2 }),
      widget("middle", { x: 3, y: 0, w: 3, h: 2 }),
      widget("right", { x: 6, y: 0, w: 6, h: 2 }),
    ], 12, 5);

    expect(projected.left).toEqual({ x: 0, y: 0, w: 1, h: 2 });
    expect(projected.middle).toEqual({ x: 1, y: 0, w: 2, h: 2 });
    expect(projected.right).toEqual({ x: 3, y: 0, w: 2, h: 2 });
  });

  it("gives compact metric cards a readable width and repacks them row-major", () => {
    const projected = projectResponsiveLayouts([
      widget("one", { x: 0, y: 0, w: 3, h: 2 }, "metric"),
      widget("two", { x: 3, y: 0, w: 3, h: 2 }, "metric"),
      widget("three", { x: 6, y: 0, w: 3, h: 2 }, "metric"),
      widget("four", { x: 9, y: 0, w: 3, h: 2 }, "metric"),
    ], 12, 4);

    expect(projected.one).toEqual({ x: 0, y: 0, w: 2, h: 2 });
    expect(projected.two).toEqual({ x: 2, y: 0, w: 2, h: 2 });
    expect(projected.three).toEqual({ x: 0, y: 2, w: 2, h: 2 });
    expect(projected.four).toEqual({ x: 2, y: 2, w: 2, h: 2 });
  });

  it("translates drag deltas from a responsive reflow without rewriting the saved origin", () => {
    const saved = { x: 6, y: 0, w: 3, h: 2 };

    expect(translateResponsiveDrag(saved, { x: 0, y: 2 }, { x: 0, y: 2 }, 12, 4)).toEqual(saved);
    expect(translateResponsiveDrag(saved, { x: 0, y: 2 }, { x: 1, y: 3 }, 12, 4)).toEqual({ x: 9, y: 1, w: 3, h: 2 });
  });

  it("closes vertical gaps after a widget is removed", () => {
    const compacted = compactDashboardWidgets([
      widget("top", { x: 0, y: 0, w: 3, h: 2 }),
      widget("lower", { x: 0, y: 5, w: 3, h: 2 }),
    ], 6);

    expect(compacted.find((item) => item.id === "lower")?.layout).toEqual({ x: 3, y: 0, w: 3, h: 2 });
  });
});
