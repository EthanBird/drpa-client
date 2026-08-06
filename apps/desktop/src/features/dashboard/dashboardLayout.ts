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
  const normalized = widgets.map((widget) => ({ ...widget, layout: normalizeLayout(widget.layout, columns) }));
  const collided = normalized
    .filter((widget) => widget.id !== movingId && overlaps(normalizedTarget, widget.layout))
    .sort(compareRowMajor);
  const collidedIds = new Set(collided.map((widget) => widget.id));

  // A drag is a local launcher-style exchange. Widgets outside the actual
  // collision remain exact anchors; only the cards covered by the drop are
  // relocated. This deliberately avoids the old global vertical compaction.
  const placed = normalized.filter((widget) => widget.id !== movingId && !collidedIds.has(widget.id));
  placed.push({ ...moving, layout: normalizedTarget });

  for (const widget of collided) {
    const vacatedSlot = normalizeLayout({ ...widget.layout, x: movingOriginal.x, y: movingOriginal.y }, columns);
    const layout = isOpen(vacatedSlot, placed)
      ? vacatedSlot
      : findFirstOpenLayout(widget.layout, placed, columns);
    placed.push({ ...widget, layout });
  }

  const byId = new Map(placed.map((widget) => [widget.id, widget]));
  return widgets.map((widget) => byId.get(widget.id) ?? widget);
}

export function projectDashboardLayout(
  layout: DashboardLayout,
  baseColumns: number,
  visibleColumns: number,
): DashboardLayout {
  const source = normalizeLayout(layout, Math.max(1, baseColumns));
  const scale = visibleColumns / Math.max(1, baseColumns);
  // Project shared edges instead of independently rounding x and width. Two
  // adjacent cards therefore keep the exact same boundary at every breakpoint.
  const x = clamp(Math.round(source.x * scale), 0, Math.max(0, visibleColumns - 1));
  const right = clamp(Math.round((source.x + source.w) * scale), x + 1, visibleColumns);
  return normalizeLayout({ ...source, x, w: right - x }, visibleColumns);
}

export function projectDashboardWidgetLayout(
  widget: DashboardWidget,
  baseColumns: number,
  visibleColumns: number,
): DashboardLayout {
  const projected = projectDashboardLayout(widget.layout, baseColumns, visibleColumns);
  const minimumWidth = minimumReadableWidth(widget.kind, visibleColumns);
  if (projected.w >= minimumWidth) return projected;
  const width = Math.min(visibleColumns, minimumWidth);
  const center = projected.x + projected.w / 2;
  const x = clamp(Math.round(center - width / 2), 0, visibleColumns - width);
  return { ...projected, x, w: width };
}

export function translateResponsiveDrag(
  savedLayout: DashboardLayout,
  visibleOrigin: Pick<DashboardLayout, "x" | "y">,
  visibleTarget: Pick<DashboardLayout, "x" | "y">,
  baseColumns: number,
  visibleColumns: number,
): DashboardLayout {
  const saved = normalizeLayout(savedLayout, baseColumns);
  const deltaX = Math.round((visibleTarget.x - visibleOrigin.x) * baseColumns / Math.max(1, visibleColumns));
  const deltaY = visibleTarget.y - visibleOrigin.y;
  return normalizeLayout({ ...saved, x: saved.x + deltaX, y: saved.y + deltaY }, baseColumns);
}

export function projectResponsiveLayouts(
  widgets: DashboardWidget[],
  baseColumns: number,
  visibleColumns: number,
): Record<string, DashboardLayout> {
  const placed: DashboardWidget[] = [];
  const result: Record<string, DashboardLayout> = {};
  const ordered = [...widgets].sort(compareRowMajor);
  for (const widget of ordered) {
    const desired = projectDashboardWidgetLayout(widget, baseColumns, visibleColumns);
    const layout = isOpen(desired, placed) ? desired : findFirstOpenLayout(desired, placed, visibleColumns);
    result[widget.id] = layout;
    placed.push({ ...widget, layout });
  }
  return result;
}

export function reflowDashboardColumns(dashboard: DashboardDefinition, columns: number): DashboardDefinition {
  const placed: DashboardWidget[] = [];
  for (const widget of [...dashboard.widgets].sort(compareRowMajor)) {
    const desired = projectDashboardLayout(widget.layout, dashboard.columns, columns);
    const layout = isOpen(desired, placed) ? desired : findFirstOpenLayout(desired, placed, columns);
    placed.push({ ...widget, layout });
  }
  const byId = new Map(placed.map((widget) => [widget.id, widget]));
  return { ...dashboard, columns, widgets: dashboard.widgets.map((widget) => byId.get(widget.id) ?? widget) };
}

export function compactDashboardWidgets(widgets: DashboardWidget[], columns: number): DashboardWidget[] {
  const placed: DashboardWidget[] = [];
  for (const widget of [...widgets].sort(compareRowMajor)) {
    placed.push({ ...widget, layout: findFirstOpenLayout(widget.layout, placed, columns) });
  }
  const byId = new Map(placed.map((widget) => [widget.id, widget]));
  return widgets.map((widget) => byId.get(widget.id) ?? widget);
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

function isOpen(layout: DashboardLayout, others: DashboardWidget[]): boolean {
  return !others.some((widget) => overlaps(layout, widget.layout));
}

function compareRowMajor(left: DashboardWidget, right: DashboardWidget): number {
  return left.layout.y - right.layout.y || left.layout.x - right.layout.x;
}

function minimumReadableWidth(kind: DashboardWidget["kind"], visibleColumns: number): number {
  if (visibleColumns > 4) return 1;
  if (kind === "line" || kind === "bar" || kind === "table") return visibleColumns;
  return Math.min(2, visibleColumns);
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
