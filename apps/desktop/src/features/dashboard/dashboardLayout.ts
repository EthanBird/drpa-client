import type { DashboardDefinition, DashboardWidget } from "../../domain/models";

export type DashboardLayout = DashboardWidget["layout"];

export function moveWidgetAndReflow(
  widgets: DashboardWidget[],
  movingId: string,
  target: DashboardLayout,
  columns: number,
): DashboardWidget[] {
  const moving = widgets.find((widget) => widget.id === movingId);
  if (!moving) return widgets;

  const normalizedTarget = normalizeLayout(target, columns);
  const movingOriginal = normalizeLayout(moving.layout, columns);
  const collidedIds = new Set(
    widgets
      .filter((widget) => widget.id !== movingId && overlaps(normalizedTarget, normalizeLayout(widget.layout, columns)))
      .map((widget) => widget.id),
  );
  const placed: DashboardWidget[] = [{ ...moving, layout: normalizedTarget }];
  const others = widgets
    .filter((widget) => widget.id !== movingId)
    .sort((left, right) => {
      const collisionOrder = Number(collidedIds.has(right.id)) - Number(collidedIds.has(left.id));
      return collisionOrder || left.layout.y - right.layout.y || left.layout.x - right.layout.x;
    });

  for (const widget of others) {
    const desired = normalizeLayout(widget.layout, columns);
    const vacatedSlot = collidedIds.has(widget.id)
      ? normalizeLayout({ ...desired, x: movingOriginal.x, y: movingOriginal.y }, columns)
      : null;
    const layout = vacatedSlot && isOpen(vacatedSlot, placed)
      ? vacatedSlot
      : isOpen(desired, placed)
        ? desired
        : findNearestOpenLayout(desired, placed, columns);
    placed.push({ ...widget, layout });
  }

  const compacted = compactVertically(placed, columns, movingId);
  const byId = new Map(compacted.map((widget) => [widget.id, widget]));
  return widgets.map((widget) => byId.get(widget.id) ?? widget);
}

export function projectResponsiveLayouts(
  widgets: DashboardWidget[],
  baseColumns: number,
  visibleColumns: number,
): Record<string, DashboardLayout> {
  const scale = visibleColumns / Math.max(1, baseColumns);
  const placed: DashboardWidget[] = [];
  const result: Record<string, DashboardLayout> = {};
  const ordered = [...widgets].sort((left, right) => left.layout.y - right.layout.y || left.layout.x - right.layout.x);
  for (const widget of ordered) {
    const width = clamp(Math.round(widget.layout.w * scale), 1, visibleColumns);
    const desired = normalizeLayout({
      ...widget.layout,
      x: Math.round(widget.layout.x * scale),
      w: width,
    }, visibleColumns);
    const layout = isOpen(desired, placed) ? desired : findNearestOpenLayout(desired, placed, visibleColumns);
    result[widget.id] = layout;
    placed.push({ ...widget, layout });
  }
  return Object.fromEntries(compactVertically(placed, visibleColumns).map((widget) => [widget.id, widget.layout]));
}

export function reflowDashboardColumns(dashboard: DashboardDefinition, columns: number): DashboardDefinition {
  const ratio = columns / Math.max(1, dashboard.columns);
  const placed: DashboardWidget[] = [];
  for (const widget of [...dashboard.widgets].sort((left, right) => left.layout.y - right.layout.y || left.layout.x - right.layout.x)) {
    const width = clamp(Math.round(widget.layout.w * ratio), 1, columns);
    const desired = normalizeLayout({ ...widget.layout, x: Math.round(widget.layout.x * ratio), w: width }, columns);
    const layout = isOpen(desired, placed) ? desired : findNearestOpenLayout(desired, placed, columns);
    placed.push({ ...widget, layout });
  }
  const compacted = compactVertically(placed, columns);
  const byId = new Map(compacted.map((widget) => [widget.id, widget]));
  return { ...dashboard, columns, widgets: dashboard.widgets.map((widget) => byId.get(widget.id) ?? widget) };
}

export function compactDashboardWidgets(widgets: DashboardWidget[], columns: number): DashboardWidget[] {
  return compactVertically(widgets, columns);
}

export function findFirstOpenLayout(layout: DashboardLayout, others: DashboardWidget[], columns: number): DashboardLayout {
  const normalized = normalizeLayout(layout, columns);
  const maxY = Math.max(0, ...others.map((widget) => widget.layout.y + widget.layout.h));
  for (let y = 0; y <= maxY + 64; y += 1) {
    for (let x = 0; x <= columns - normalized.w; x += 1) {
      const candidate = { ...normalized, x, y };
      if (isOpen(candidate, others)) return candidate;
    }
  }
  return { ...normalized, x: 0, y: maxY };
}

export function findNearestOpenLayout(layout: DashboardLayout, others: DashboardWidget[], columns: number): DashboardLayout {
  const normalized = normalizeLayout(layout, columns);
  if (isOpen(normalized, others)) return normalized;
  const maxY = Math.max(normalized.y + normalized.h, ...others.map((widget) => widget.layout.y + widget.layout.h)) + 64;
  let best: DashboardLayout | null = null;
  let bestScore = Number.POSITIVE_INFINITY;
  for (let y = 0; y <= maxY; y += 1) {
    for (let x = 0; x <= columns - normalized.w; x += 1) {
      const candidate = { ...normalized, x, y };
      if (!isOpen(candidate, others)) continue;
      const score = Math.abs(y - normalized.y) * columns * 3 + Math.abs(x - normalized.x);
      if (score < bestScore) {
        best = candidate;
        bestScore = score;
      }
    }
  }
  return best ?? { ...normalized, x: 0, y: maxY };
}

export function overlaps(left: DashboardLayout, right: DashboardLayout): boolean {
  return left.x < right.x + right.w
    && left.x + left.w > right.x
    && left.y < right.y + right.h
    && left.y + left.h > right.y;
}

function compactVertically(widgets: DashboardWidget[], columns: number, pinnedId = ""): DashboardWidget[] {
  const layouts = new Map(widgets.map((widget) => [widget.id, normalizeLayout(widget.layout, columns)]));
  const ordered = [...widgets].sort((left, right) => {
    if (left.id === pinnedId) return -1;
    if (right.id === pinnedId) return 1;
    const leftLayout = layouts.get(left.id)!;
    const rightLayout = layouts.get(right.id)!;
    return leftLayout.y - rightLayout.y || leftLayout.x - rightLayout.x;
  });

  for (const widget of ordered) {
    if (widget.id === pinnedId) continue;
    const current = layouts.get(widget.id)!;
    let candidate = current;
    while (candidate.y > 0) {
      const above = { ...candidate, y: candidate.y - 1 };
      const blocked = widgets.some((other) => other.id !== widget.id && overlaps(above, layouts.get(other.id)!));
      if (blocked) break;
      candidate = above;
    }
    layouts.set(widget.id, candidate);
  }

  return widgets.map((widget) => ({ ...widget, layout: layouts.get(widget.id)! }));
}

function isOpen(layout: DashboardLayout, others: DashboardWidget[]): boolean {
  return !others.some((widget) => overlaps(layout, widget.layout));
}

function normalizeLayout(layout: DashboardLayout, columns: number): DashboardLayout {
  const width = clamp(layout.w, 1, Math.max(1, columns));
  return {
    x: clamp(layout.x, 0, Math.max(0, columns - width)),
    y: Math.max(0, layout.y),
    w: width,
    h: clamp(layout.h, 1, 24),
  };
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value));
}
