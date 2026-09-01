import type { DashboardDefinition, DashboardWidget } from "../../domain/models";

export type DashboardLayout = DashboardWidget["layout"];

export function moveWidgetAndReflow(
  widgets: DashboardWidget[],
  movingId: string,
  target: DashboardLayout,
  columns: number,
): DashboardWidget[] {
  const ordered = normalizeDashboardWidgetOrder(widgets, columns);
  const movingIndex = ordered.findIndex((widget) => widget.id === movingId);
  if (movingIndex < 0) return widgets;

  const moving = ordered[movingIndex];
  const remaining = ordered.filter((widget) => widget.id !== movingId);
  const insertionIndex = findDropInsertionIndex(remaining, moving, normalizeLayout(target, columns), columns, movingIndex);
  const reordered = [
    ...remaining.slice(0, insertionIndex),
    moving,
    ...remaining.slice(insertionIndex),
  ];
  return packDashboardWidgets(reordered, columns);
}

export function resizeWidgetAndReflow(
  widgets: DashboardWidget[],
  widgetId: string,
  target: DashboardLayout,
  columns: number,
): DashboardWidget[] {
  const ordered = normalizeDashboardWidgetOrder(widgets, columns);
  if (!ordered.some((widget) => widget.id === widgetId)) return widgets;
  return packDashboardWidgets(ordered.map((widget) => widget.id === widgetId
    ? { ...widget, layout: normalizeLayout(target, columns) }
    : widget), columns);
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
  const ordered = normalizeDashboardWidgetOrder(widgets, baseColumns);
  let cursor = 0;
  for (const widget of ordered) {
    const desired = projectDashboardWidgetLayout(widget, baseColumns, visibleColumns);
    const layout = findOrderedOpenLayout(desired, placed, visibleColumns, cursor);
    result[widget.id] = layout;
    placed.push({ ...widget, layout });
    cursor = rowMajorOrdinal(layout, visibleColumns);
  }
  return result;
}

export function reflowDashboardColumns(dashboard: DashboardDefinition, columns: number): DashboardDefinition {
  const ordered = normalizeDashboardWidgetOrder(dashboard.widgets, dashboard.columns)
    .map((widget) => ({ ...widget, layout: projectDashboardLayout(widget.layout, dashboard.columns, columns) }));
  return { ...dashboard, columns, widgets: packDashboardWidgets(ordered, columns) };
}

export function compactDashboardWidgets(widgets: DashboardWidget[], columns: number): DashboardWidget[] {
  return packDashboardWidgets(widgets, columns);
}

export function packDashboardWidgets(widgets: DashboardWidget[], columns: number): DashboardWidget[] {
  const placed: DashboardWidget[] = [];
  let cursor = 0;
  for (const widget of widgets) {
    const layout = findOrderedOpenLayout(widget.layout, placed, columns, cursor);
    placed.push({ ...widget, layout });
    cursor = rowMajorOrdinal(layout, columns);
  }
  return placed;
}

export function normalizeDashboardWidgetOrder(widgets: DashboardWidget[], columns: number): DashboardWidget[] {
  const normalized = widgets.map((widget) => ({ ...widget, layout: normalizeLayout(widget.layout, columns) }));
  const alreadyOrdered = normalized.every((widget, index) => index === 0
    || compareRowMajor(normalized[index - 1], widget) <= 0);
  return alreadyOrdered ? normalized : [...normalized].sort(compareRowMajor);
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

function findDropInsertionIndex(
  remaining: DashboardWidget[],
  moving: DashboardWidget,
  target: DashboardLayout,
  columns: number,
  currentIndex: number,
): number {
  const prefix: DashboardWidget[] = [];
  let cursor = 0;
  let bestIndex = 0;
  let bestDistance = Number.POSITIVE_INFINITY;
  let bestDisruption = Number.POSITIVE_INFINITY;

  for (let index = 0; index <= remaining.length; index += 1) {
    const candidate = findOrderedOpenLayout(moving.layout, prefix, columns, cursor);
    const deltaX = candidate.x - target.x;
    const deltaY = candidate.y - target.y;
    const distance = deltaY * deltaY * columns * columns + deltaX * deltaX;
    const disruption = Math.abs(index - currentIndex);
    if (distance < bestDistance || (distance === bestDistance && disruption < bestDisruption)) {
      bestIndex = index;
      bestDistance = distance;
      bestDisruption = disruption;
    }

    const next = remaining[index];
    if (!next) continue;
    const layout = findOrderedOpenLayout(next.layout, prefix, columns, cursor);
    prefix.push({ ...next, layout });
    cursor = rowMajorOrdinal(layout, columns);
  }
  return bestIndex;
}

function findOrderedOpenLayout(
  layout: DashboardLayout,
  others: DashboardWidget[],
  columns: number,
  startOrdinal: number,
): DashboardLayout {
  const normalized = normalizeLayout(layout, columns);
  const startY = Math.floor(startOrdinal / columns);
  const startX = startOrdinal % columns;
  const maxY = Math.max(startY, ...others.map((widget) => widget.layout.y + widget.layout.h));
  for (let y = startY; y <= maxY + 64; y += 1) {
    const firstX = y === startY ? startX : 0;
    for (let x = firstX; x <= columns - normalized.w; x += 1) {
      const candidate = { ...normalized, x, y };
      if (isOpen(candidate, others)) return candidate;
    }
  }
  return { ...normalized, x: 0, y: maxY };
}

function rowMajorOrdinal(layout: DashboardLayout, columns: number): number {
  return layout.y * columns + layout.x;
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
