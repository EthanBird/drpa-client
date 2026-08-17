import {
  BarChart3,
  Check,
  Copy,
  Database,
  GripVertical,
  Hash,
  LayoutDashboard,
  LineChart,
  Maximize2,
  Monitor,
  PanelRightClose,
  PanelRightOpen,
  Pencil,
  PieChart,
  Plus,
  RefreshCw,
  Search,
  Smartphone,
  Table2,
  Tablet,
  Trash2,
  Type,
  X,
} from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, PointerEvent as ReactPointerEvent, ReactNode } from "react";

import { useAppStore } from "../app/store";
import { useNavigationSurfaceActive } from "../app/NavigationSurface";
import type {
  DashboardDataset,
  DashboardDefinition,
  DashboardDocument,
  DashboardWidget,
  DashboardWidgetKind,
  RemoteDatabaseProfile,
} from "../domain/models";
import { DashboardWidgetView } from "../features/dashboard/DashboardWidgetView";
import {
  compactDashboardWidgets,
  findFirstOpenLayout,
  moveWidgetAndReflow,
  normalizeDashboardWidgetOrder,
  projectDashboardWidgetLayout,
  projectResponsiveLayouts,
  reflowDashboardColumns,
  resizeWidgetAndReflow,
  translateResponsiveDrag,
} from "../features/dashboard/dashboardLayout";
import { dashboardRuntime } from "../features/dashboard/dashboardRuntime";
import { desktopGateway } from "../infra/gateway";

interface WidgetResult {
  loading: boolean;
  dataset?: DashboardDataset;
  error?: string;
}

interface ResizeSession {
  widgetId: string;
  pointerId: number;
  startX: number;
  startY: number;
  initialLayout: DashboardWidget["layout"];
  targetLayout: DashboardWidget["layout"];
  dashboardColumns: number;
  visibleColumns: number;
  columnPitch: number;
  rowPitch: number;
}

interface DragSession {
  widgetId: string;
  pointerId: number;
  grabOffsetX: number;
  grabOffsetY: number;
  dashboardColumns: number;
  visibleColumns: number;
  rowHeight: number;
  startClientX: number;
  startClientY: number;
  initialLayout: DashboardWidget["layout"];
  visibleOriginLayout: DashboardWidget["layout"];
  originalOrderKey: string;
  previewOrderKey: string;
  previewWidgets: DashboardWidget[];
}

interface DragPreview {
  widgetId: string;
  translateX: number;
  translateY: number;
  visibleOriginLayout: DashboardWidget["layout"];
  dropLayout: DashboardWidget["layout"];
  layouts: Record<string, DashboardWidget["layout"]>;
  orderKey: string;
}

interface ResizePreview {
  widgetId: string;
  targetLayout: DashboardWidget["layout"];
}

export function OverviewPage() {
  const snapshot = useAppStore((state) => state.snapshot);
  const pageActive = useNavigationSurfaceActive();
  const [document, setDocument] = useState<DashboardDocument | null>(null);
  const documentRef = useRef<DashboardDocument | null>(null);
  const [editMode, setEditMode] = useState(false);
  const [selectedWidgetId, setSelectedWidgetId] = useState("");
  const [inspectorOpen, setInspectorOpen] = useState(false);
  const [widgetResults, setWidgetResults] = useState<Record<string, WidgetResult>>({});
  const [profiles, setProfiles] = useState<RemoteDatabaseProfile[]>([]);
  const [profilePasswords, setProfilePasswords] = useState<Record<string, string>>({});
  const [saveState, setSaveState] = useState<"saved" | "saving" | "error">("saved");
  const [loadError, setLoadError] = useState("");
  const [refreshTick, setRefreshTick] = useState(0);
  const [visibleColumns, setVisibleColumns] = useState(12);
  const [builderTab, setBuilderTab] = useState<"components" | "data">("components");
  const [componentQuery, setComponentQuery] = useState("");
  const [viewportMode, setViewportMode] = useState<"auto" | "desktop" | "tablet" | "phone">("auto");
  const [draggingId, setDraggingId] = useState("");
  const [dragPreview, setDragPreview] = useState<DragPreview | null>(null);
  const [resizePreview, setResizePreview] = useState<ResizePreview | null>(null);
  const [deleteArmed, setDeleteArmed] = useState(false);
  const canvasRef = useRef<HTMLDivElement | null>(null);
  const saveTimerRef = useRef<number | null>(null);
  const saveQueueRef = useRef<Promise<void>>(Promise.resolve());
  const saveRevisionRef = useRef(0);
  const resizeRef = useRef<ResizeSession | null>(null);
  const dragRef = useRef<DragSession | null>(null);
  const dragFrameRef = useRef<number | null>(null);
  const pendingDragPointRef = useRef<{ x: number; y: number } | null>(null);
  const widgetRectsRef = useRef<Map<string, DOMRect> | null>(null);

  const activeDashboard = document?.dashboards.find((item) => item.id === document.activeDashboardId)
    ?? document?.dashboards[0]
    ?? null;
  const selectedWidget = activeDashboard?.widgets.find((widget) => widget.id === selectedWidgetId) ?? null;

  useEffect(() => {
    let disposed = false;
    void Promise.all([desktopGateway.getBiDashboard(), desktopGateway.listRemoteDatabaseProfiles()])
      .then(([nextDocument, nextProfiles]) => {
        if (disposed) return;
        documentRef.current = nextDocument;
        setDocument(nextDocument);
        setProfiles(nextProfiles);
      })
      .catch((error: unknown) => {
        if (!disposed) {
          setSaveState("error");
          setLoadError(String(error));
        }
        console.error("加载 BI 仪表盘失败", error);
      });
    return () => { disposed = true; };
  }, []);

  useEffect(() => {
    documentRef.current = document;
  }, [document]);

  useEffect(() => () => {
    if (saveTimerRef.current !== null) window.clearTimeout(saveTimerRef.current);
    if (dragFrameRef.current !== null) window.cancelAnimationFrame(dragFrameRef.current);
  }, []);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !activeDashboard) return;
    const update = () => {
      const width = canvas.clientWidth;
      if (viewportMode === "desktop") setVisibleColumns(activeDashboard.columns);
      else if (viewportMode === "tablet") setVisibleColumns(Math.min(8, activeDashboard.columns));
      else if (viewportMode === "phone") setVisibleColumns(Math.min(4, activeDashboard.columns));
      else setVisibleColumns(width >= 1060 ? activeDashboard.columns : width >= 700 ? Math.min(8, activeDashboard.columns) : Math.min(4, activeDashboard.columns));
    };
    update();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(update);
    observer.observe(canvas);
    return () => observer.disconnect();
  }, [activeDashboard?.id, activeDashboard?.columns, inspectorOpen, viewportMode]);

  const querySignature = useMemo(() => JSON.stringify(activeDashboard?.widgets.map((widget) => ({ id: widget.id, source: widget.source })) ?? []), [activeDashboard?.widgets]);
  const refreshSignature = useMemo(() => JSON.stringify(activeDashboard?.widgets.map((widget) => ({ id: widget.id, refreshSeconds: widget.options.refreshSeconds })) ?? []), [activeDashboard?.widgets]);
  const passwordSignature = useMemo(() => JSON.stringify(profilePasswords), [profilePasswords]);
  const projectedLayouts = useMemo(
    () => projectResponsiveLayouts(activeDashboard?.widgets ?? [], activeDashboard?.columns ?? 12, visibleColumns),
    [activeDashboard?.widgets, activeDashboard?.columns, visibleColumns],
  );

  useEffect(() => {
    if (!pageActive || !snapshot || !activeDashboard) return;
    let disposed = false;
    const dataWidgets = activeDashboard.widgets.filter((widget) => widget.kind !== "markdown");
    setWidgetResults((current) => Object.fromEntries(dataWidgets.map((widget) => [widget.id, { ...current[widget.id], loading: true }])));
    const timer = window.setTimeout(() => {
      void Promise.all(dataWidgets.map(async (widget) => {
        try {
          const dataset = await dashboardRuntime.load(widget, { snapshot, gateway: desktopGateway, profilePasswords });
          if (!disposed) setWidgetResults((current) => ({ ...current, [widget.id]: { loading: false, dataset } }));
        } catch (error) {
          if (!disposed) setWidgetResults((current) => ({ ...current, [widget.id]: { loading: false, error: String(error) } }));
        }
      }));
    }, 240);
    return () => { disposed = true; window.clearTimeout(timer); };
  }, [pageActive, snapshot, activeDashboard?.id, querySignature, passwordSignature, refreshTick]);

  useEffect(() => {
    if (!pageActive || !activeDashboard) return;
    const intervals = activeDashboard.widgets.map((widget) => widget.options.refreshSeconds).filter((seconds) => seconds > 0);
    if (!intervals.length) return;
    const timer = window.setInterval(() => setRefreshTick((value) => value + 1), Math.max(5, Math.min(...intervals)) * 1_000);
    return () => window.clearInterval(timer);
  }, [pageActive, activeDashboard?.id, refreshSignature]);

  const commitDocument = useCallback((next: DashboardDocument) => {
    documentRef.current = next;
    setDocument(next);
    setSaveState("saving");
    if (saveTimerRef.current !== null) window.clearTimeout(saveTimerRef.current);
    const revision = ++saveRevisionRef.current;
    saveTimerRef.current = window.setTimeout(() => {
      saveQueueRef.current = saveQueueRef.current.then(async () => {
        try {
          const saved = await desktopGateway.saveBiDashboard(next);
          if (revision !== saveRevisionRef.current) return;
          documentRef.current = saved;
          setDocument(saved);
          setSaveState("saved");
        } catch (error) {
          if (revision === saveRevisionRef.current) setSaveState("error");
          console.error("保存 BI 仪表盘失败", error);
        }
      });
    }, 360);
  }, []);

  const applyActiveDashboard = useCallback((mutator: (dashboard: DashboardDefinition) => DashboardDefinition, persist = true) => {
    const current = documentRef.current;
    if (!current) return;
    const next = {
      ...current,
      dashboards: current.dashboards.map((dashboard) => dashboard.id === current.activeDashboardId ? mutator(dashboard) : dashboard),
    };
    if (persist) commitDocument(next);
    else {
      documentRef.current = next;
      setDocument(next);
    }
  }, [commitDocument]);

  const mutateActiveDashboard = useCallback((mutator: (dashboard: DashboardDefinition) => DashboardDefinition) => {
    applyActiveDashboard(mutator, true);
  }, [applyActiveDashboard]);

  const replaceActiveWidgets = useCallback((widgets: DashboardWidget[], persist = true) => {
    applyActiveDashboard((dashboard) => ({ ...dashboard, widgets }), persist);
  }, [applyActiveDashboard]);

  const updateWidget = useCallback((widgetId: string, mutator: (widget: DashboardWidget) => DashboardWidget, persist = true) => {
    applyActiveDashboard((dashboard) => ({
      ...dashboard,
      widgets: dashboard.widgets.map((widget) => widget.id === widgetId ? mutator(widget) : widget),
    }), persist);
  }, [applyActiveDashboard]);

  const captureWidgetRects = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    widgetRectsRef.current = new Map(
      [...canvas.querySelectorAll<HTMLElement>("[data-bi-widget-id]")]
        .map((element) => [element.dataset.biWidgetId ?? "", element.getBoundingClientRect()] as const)
        .filter(([id]) => Boolean(id)),
    );
  }, []);

  useLayoutEffect(() => {
    const previous = widgetRectsRef.current;
    const canvas = canvasRef.current;
    if (!previous || !canvas) return;
    widgetRectsRef.current = null;
    for (const element of canvas.querySelectorAll<HTMLElement>("[data-bi-widget-id]")) {
      const widgetId = element.dataset.biWidgetId ?? "";
      if (!widgetId || widgetId === draggingId || typeof element.animate !== "function") continue;
      const before = previous.get(widgetId);
      if (!before) continue;
      const after = element.getBoundingClientRect();
      const deltaX = before.left - after.left;
      const deltaY = before.top - after.top;
      if (Math.abs(deltaX) < 1 && Math.abs(deltaY) < 1) continue;
      if (typeof element.getAnimations === "function") {
        element.getAnimations().forEach((animation) => animation.cancel());
      }
      element.animate([
        { transform: `translate(${deltaX}px, ${deltaY}px)` },
        { transform: "translate(0, 0)" },
      ], { duration: 180, easing: "cubic-bezier(.2,.8,.2,1)" });
    }
  }, [activeDashboard?.widgets, draggingId, dragPreview?.orderKey]);

  const applyDragPoint = useCallback((clientX: number, clientY: number) => {
    const session = dragRef.current;
    const canvas = canvasRef.current;
    if (!session || !canvas) return;
    const metrics = canvasGridMetrics(canvas, session.visibleColumns, session.rowHeight);
    const visibleX = clamp(
      Math.round((clientX - session.grabOffsetX - metrics.left) / metrics.columnPitch),
      0,
      session.visibleColumns - session.visibleOriginLayout.w,
    );
    const visibleY = Math.max(0, Math.round((clientY - session.grabOffsetY - metrics.top) / metrics.rowPitch));
    const targetLayout = translateResponsiveDrag(
      session.initialLayout,
      session.visibleOriginLayout,
      { x: visibleX, y: visibleY },
      session.dashboardColumns,
      session.visibleColumns,
    );
    const current = documentRef.current;
    const dashboard = current?.dashboards.find((item) => item.id === current.activeDashboardId);
    if (!dashboard) return;
    const previewWidgets = moveWidgetAndReflow(
      dashboard.widgets,
      session.widgetId,
      { ...session.initialLayout, x: targetLayout.x, y: targetLayout.y },
      session.dashboardColumns,
    );
    const orderKey = previewWidgets.map((widget) => widget.id).join("|");
    if (orderKey !== session.previewOrderKey) captureWidgetRects();
    session.previewOrderKey = orderKey;
    session.previewWidgets = previewWidgets;
    const layouts = projectResponsiveLayouts(previewWidgets, session.dashboardColumns, session.visibleColumns);
    setDragPreview({
      widgetId: session.widgetId,
      translateX: clientX - session.startClientX,
      translateY: clientY - session.startClientY,
      visibleOriginLayout: session.visibleOriginLayout,
      dropLayout: layouts[session.widgetId] ?? { ...session.visibleOriginLayout, x: visibleX, y: visibleY },
      layouts,
      orderKey,
    });
  }, [captureWidgetRects]);

  useEffect(() => {
    const onPointerMove = (event: PointerEvent) => {
      const session = resizeRef.current;
      if (!session || event.pointerId !== session.pointerId) return;
      event.preventDefault();
      const visibleDeltaColumns = Math.round((event.clientX - session.startX) / session.columnPitch);
      const deltaColumns = Math.round(visibleDeltaColumns * session.dashboardColumns / session.visibleColumns);
      const deltaRows = Math.round((event.clientY - session.startY) / session.rowPitch);
      const target = {
        ...session.initialLayout,
        w: clamp(session.initialLayout.w + deltaColumns, 1, session.dashboardColumns - session.initialLayout.x),
        h: clamp(session.initialLayout.h + deltaRows, 1, 24),
      };
      if (target.w === session.targetLayout.w && target.h === session.targetLayout.h) return;
      session.targetLayout = target;
      setResizePreview({ widgetId: session.widgetId, targetLayout: target });
    };
    const finishResize = (commit: boolean) => {
      const session = resizeRef.current;
      if (!session) return;
      resizeRef.current = null;
      setResizePreview(null);
      if (!commit) return;
      const current = documentRef.current;
      const dashboard = current?.dashboards.find((item) => item.id === current.activeDashboardId);
      if (!dashboard) return;
      captureWidgetRects();
      replaceActiveWidgets(
        resizeWidgetAndReflow(dashboard.widgets, session.widgetId, session.targetLayout, session.dashboardColumns),
        true,
      );
    };
    const onPointerUp = (event: PointerEvent) => {
      if (resizeRef.current?.pointerId === event.pointerId) finishResize(true);
    };
    const onPointerCancel = (event: PointerEvent) => {
      if (resizeRef.current?.pointerId === event.pointerId) finishResize(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && resizeRef.current) finishResize(false);
    };
    window.addEventListener("pointermove", onPointerMove, { passive: false });
    window.addEventListener("pointerup", onPointerUp);
    window.addEventListener("pointercancel", onPointerCancel);
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
      window.removeEventListener("pointercancel", onPointerCancel);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [captureWidgetRects, replaceActiveWidgets]);

  useEffect(() => {
    const flushPendingDrag = () => {
      dragFrameRef.current = null;
      const point = pendingDragPointRef.current;
      pendingDragPointRef.current = null;
      if (point) applyDragPoint(point.x, point.y);
    };
    const onPointerMove = (event: PointerEvent) => {
      const session = dragRef.current;
      if (!session || event.pointerId !== session.pointerId) return;
      event.preventDefault();
      pendingDragPointRef.current = { x: event.clientX, y: event.clientY };
      if (dragFrameRef.current === null) dragFrameRef.current = window.requestAnimationFrame(flushPendingDrag);
    };
    const finish = (commit: boolean, point?: { x: number; y: number }) => {
      const session = dragRef.current;
      if (!session) return;
      if (dragFrameRef.current !== null) {
        window.cancelAnimationFrame(dragFrameRef.current);
        dragFrameRef.current = null;
      }
      const finalPoint = point ?? pendingDragPointRef.current;
      pendingDragPointRef.current = null;
      if (commit && finalPoint) applyDragPoint(finalPoint.x, finalPoint.y);
      if (commit) {
        const reordered = session.previewOrderKey !== session.originalOrderKey;
        if (reordered) {
          captureWidgetRects();
          replaceActiveWidgets(session.previewWidgets, true);
        }
      }
      dragRef.current = null;
      setDraggingId("");
      setDragPreview(null);
    };
    const onPointerUp = (event: PointerEvent) => {
      const session = dragRef.current;
      if (!session || event.pointerId !== session.pointerId) return;
      finish(true, { x: event.clientX, y: event.clientY });
    };
    const onPointerCancel = (event: PointerEvent) => {
      if (dragRef.current?.pointerId === event.pointerId) finish(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && dragRef.current) finish(false);
    };
    window.addEventListener("pointermove", onPointerMove, { passive: false });
    window.addEventListener("pointerup", onPointerUp);
    window.addEventListener("pointercancel", onPointerCancel);
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
      window.removeEventListener("pointercancel", onPointerCancel);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [applyDragPoint, captureWidgetRects, replaceActiveWidgets]);

  if (loadError) return <div className="page bi-page"><div className="bi-load-error"><LayoutDashboard size={34} /><h1>BI 主页配置读取失败</h1><p>{loadError}</p><button className="button primary" type="button" onClick={() => {
    setLoadError("");
    void desktopGateway.resetBiDashboard().then((reset) => {
      documentRef.current = reset;
      setDocument(reset);
      setSaveState("saved");
    }).catch((error: unknown) => setLoadError(String(error)));
  }}>恢复默认主页</button></div></div>;
  if (!snapshot || !document || !activeDashboard) return <div className="page loading-page"><div className="skeleton skeleton-title" /></div>;

  const addWidget = (kind: DashboardWidgetKind) => {
    const widget = createWidget(kind, activeDashboard);
    mutateActiveDashboard((dashboard) => ({
      ...dashboard,
      widgets: compactDashboardWidgets([
        ...normalizeDashboardWidgetOrder(dashboard.widgets, dashboard.columns),
        widget,
      ], dashboard.columns),
    }));
    setSelectedWidgetId(widget.id);
    setInspectorOpen(true);
  };

  const addBuiltinWidget = (dataset: "workspaceSummary" | "runHistory" | "runStatus" | "packages", title: string, kind: DashboardWidgetKind) => {
    const widget = createWidget(kind, activeDashboard);
    widget.title = title;
    if (kind !== "markdown") widget.source = { kind: "builtin", dataset };
    mutateActiveDashboard((dashboard) => ({ ...dashboard, widgets: compactDashboardWidgets([...normalizeDashboardWidgetOrder(dashboard.widgets, dashboard.columns), widget], dashboard.columns) }));
    setSelectedWidgetId(widget.id);
    setInspectorOpen(true);
  };

  const addDatabaseWidget = (profileId: string, title: string) => {
    const widget = createWidget("table", activeDashboard);
    widget.title = title;
    widget.source = profileId === "workspace"
      ? { kind: "database", profileId, sql: "SELECT name, type FROM sqlite_master WHERE type IN ('table', 'view') ORDER BY name LIMIT 100" }
      : { kind: "database", profileId, sql: "SELECT 1 AS value" };
    mutateActiveDashboard((dashboard) => ({ ...dashboard, widgets: compactDashboardWidgets([...normalizeDashboardWidgetOrder(dashboard.widgets, dashboard.columns), widget], dashboard.columns) }));
    setSelectedWidgetId(widget.id);
    setInspectorOpen(true);
  };

  const createDashboard = () => {
    const id = uniqueId("dashboard");
    const dashboard: DashboardDefinition = {
      id,
      title: `新仪表盘 ${document.dashboards.length + 1}`,
      description: "从空白栅格开始组织业务数据。",
      columns: 12,
      rowHeight: 58,
      widgets: [],
    };
    commitDocument({ ...document, activeDashboardId: id, dashboards: [...document.dashboards, dashboard] });
    setSelectedWidgetId("");
    setEditMode(true);
    setInspectorOpen(true);
  };

  const deleteDashboard = () => {
    if (document.dashboards.length <= 1) return;
    if (!deleteArmed) {
      setDeleteArmed(true);
      window.setTimeout(() => setDeleteArmed(false), 3_000);
      return;
    }
    const dashboards = document.dashboards.filter((dashboard) => dashboard.id !== activeDashboard.id);
    commitDocument({ ...document, activeDashboardId: dashboards[0].id, dashboards });
    setDeleteArmed(false);
    setSelectedWidgetId("");
  };

  const startWidgetDrag = (event: ReactPointerEvent<HTMLElement>, widget: DashboardWidget, visibleLayout: DashboardWidget["layout"]) => {
    if (!editMode || !canvasRef.current || !documentRef.current) return;
    event.preventDefault();
    const metrics = canvasGridMetrics(canvasRef.current, visibleColumns, activeDashboard.rowHeight);
    const orderedWidgets = normalizeDashboardWidgetOrder(activeDashboard.widgets, activeDashboard.columns);
    const orderKey = orderedWidgets.map((item) => item.id).join("|");
    dragRef.current = {
      widgetId: widget.id,
      pointerId: event.pointerId,
      grabOffsetX: event.clientX - (metrics.left + visibleLayout.x * metrics.columnPitch),
      grabOffsetY: event.clientY - (metrics.top + visibleLayout.y * metrics.rowPitch),
      dashboardColumns: activeDashboard.columns,
      visibleColumns,
      rowHeight: activeDashboard.rowHeight,
      startClientX: event.clientX,
      startClientY: event.clientY,
      initialLayout: { ...widget.layout },
      visibleOriginLayout: { ...visibleLayout },
      originalOrderKey: orderKey,
      previewOrderKey: orderKey,
      previewWidgets: compactDashboardWidgets(orderedWidgets, activeDashboard.columns),
    };
    try {
      event.currentTarget.setPointerCapture?.(event.pointerId);
    } catch {
      // Window-level pointer listeners still keep the drag session active.
    }
    setDraggingId(widget.id);
    setDragPreview({
      widgetId: widget.id,
      translateX: 0,
      translateY: 0,
      visibleOriginLayout: { ...visibleLayout },
      dropLayout: { ...visibleLayout },
      layouts: projectedLayouts,
      orderKey,
    });
    setSelectedWidgetId(widget.id);
    setInspectorOpen(true);
  };

  return (
    <div className="page bi-page">
      <header className="bi-toolbar">
        <div className="bi-brand"><span><LayoutDashboard size={18} /></span><div><h1>{activeDashboard.title}</h1><p>{activeDashboard.description}</p></div></div>
        <div className="bi-dashboard-switcher">
          <select aria-label="选择 BI 仪表盘" value={activeDashboard.id} onChange={(event) => { commitDocument({ ...document, activeDashboardId: event.target.value }); setSelectedWidgetId(""); }}>
            {document.dashboards.map((dashboard) => <option key={dashboard.id} value={dashboard.id}>{dashboard.title}</option>)}
          </select>
          <button className="icon-button subtle" type="button" title="新建仪表盘" aria-label="新建仪表盘" onClick={createDashboard}><Plus size={15} /></button>
        </div>
        <div className="bi-toolbar-spacer" />
        <span className={`bi-save-state ${saveState}`}>{saveState === "saving" ? <RefreshCw size={12} /> : saveState === "error" ? <X size={12} /> : <Check size={12} />}{saveState === "saving" ? "保存中" : saveState === "error" ? "保存失败" : "已保存"}</span>
        <button className="button secondary small" type="button" onClick={() => setRefreshTick((value) => value + 1)}><RefreshCw size={14} /> 刷新数据</button>
        <button className={`button small ${editMode ? "primary" : "secondary"}`} type="button" onClick={() => { setEditMode((value) => !value); if (!editMode) setInspectorOpen(true); }}><Pencil size={14} /> {editMode ? "完成编辑" : "编辑主页"}</button>
        {editMode && <button className="icon-button subtle" type="button" title={inspectorOpen ? "收起属性面板" : "打开属性面板"} onClick={() => setInspectorOpen((value) => !value)}>{inspectorOpen ? <PanelRightClose size={16} /> : <PanelRightOpen size={16} />}</button>}
      </header>

      <div className={`bi-workspace ${editMode ? "editing" : ""} ${editMode && inspectorOpen ? "with-inspector" : ""}`}>
        {editMode && <aside className="bi-builder-sidebar" aria-label="添加 BI 组件">
          <header><div><strong>构建器</strong><span>组件与数据</span></div></header>
          <nav className="bi-builder-tabs" role="tablist" aria-label="BI 构建器">
            <button type="button" className={builderTab === "components" ? "active" : ""} onClick={() => setBuilderTab("components")}>组件</button>
            <button type="button" className={builderTab === "data" ? "active" : ""} onClick={() => setBuilderTab("data")}>数据源</button>
          </nav>
          {builderTab === "components" ? <>
            <label className="bi-builder-search"><Search size={13} /><input aria-label="搜索 BI 组件" value={componentQuery} onChange={(event) => setComponentQuery(event.target.value)} placeholder="搜索组件" /></label>
            <div className="bi-builder-list"><span className="bi-builder-group-title">可视化组件</span>{BUILDER_COMPONENTS.filter((component) => `${component.label} ${component.description}`.toLowerCase().includes(componentQuery.toLowerCase())).map((component) => { const Icon = component.icon; return <button type="button" key={component.kind} aria-label={component.label} onClick={() => addWidget(component.kind)}><span><Icon size={16} /></span><div><strong>{component.label}</strong><small>{component.description}</small></div><Plus size={13} /></button>; })}</div>
          </> : <div className="bi-builder-data">
            <span className="bi-builder-group-title">DRPA 内置数据</span>
            <button type="button" onClick={() => addBuiltinWidget("workspaceSummary", "工作区概览", "metric")}><span><Hash size={15} /></span><div><strong>工作区指标</strong><small>运行、包与工作区汇总</small></div></button>
            <button type="button" onClick={() => addBuiltinWidget("runHistory", "运行趋势", "line")}><span><LineChart size={15} /></span><div><strong>运行记录</strong><small>时间、耗时与运行结果</small></div></button>
            <button type="button" onClick={() => addBuiltinWidget("runStatus", "运行状态", "pie")}><span><PieChart size={15} /></span><div><strong>状态汇总</strong><small>成功、失败与取消占比</small></div></button>
            <button type="button" onClick={() => addBuiltinWidget("packages", "RPAZ 包清单", "table")}><span><Table2 size={15} /></span><div><strong>RPAZ 包</strong><small>已安装包与版本信息</small></div></button>
            <span className="bi-builder-group-title">数据工作台连接</span>
            <button type="button" onClick={() => addDatabaseWidget("workspace", "工作区 SQLite")}><span><Database size={15} /></span><div><strong>工作区 SQLite</strong><small>内置只读查询</small></div></button>
            {profiles.map((profile) => <button type="button" key={profile.id} onClick={() => addDatabaseWidget(profile.id, profile.name)}><span><Database size={15} /></span><div><strong>{profile.name}</strong><small>{profile.engine.toUpperCase()} · 只读查询</small></div></button>)}
          </div>}
          <footer>{activeDashboard.columns} 列栅格 · 自动保存</footer>
        </aside>}

        <section className={`bi-stage ${editMode ? "editing" : ""}`}>
          {editMode && <header className="bi-stage-toolbar"><div><strong>画布</strong><span>{visibleColumns} 列响应式预览</span></div><div className="bi-viewport-switch" role="group" aria-label="画布尺寸"><button type="button" className={viewportMode === "auto" ? "active" : ""} title="自动" onClick={() => setViewportMode("auto")}><Maximize2 size={14} /></button><button type="button" className={viewportMode === "desktop" ? "active" : ""} title="桌面" onClick={() => setViewportMode("desktop")}><Monitor size={14} /></button><button type="button" className={viewportMode === "tablet" ? "active" : ""} title="平板" onClick={() => setViewportMode("tablet")}><Tablet size={14} /></button><button type="button" className={viewportMode === "phone" ? "active" : ""} title="手机" onClick={() => setViewportMode("phone")}><Smartphone size={14} /></button></div></header>}
          <div className="bi-stage-viewport">
          <div
          className={`bi-canvas ${editMode ? "editing" : ""} ${draggingId ? "drag-active" : ""}`}
          ref={canvasRef}
          data-viewport={viewportMode}
          style={{ "--bi-columns": visibleColumns, "--bi-row-height": `${activeDashboard.rowHeight}px` } as CSSProperties}
        >
          {!activeDashboard.widgets.length && <div className="bi-empty-canvas"><LayoutDashboard size={34} /><h2>这是一个空白仪表盘</h2><p>进入编辑模式，然后添加指标、图表、表格或 Markdown 描述。</p>{!editMode && <button className="button primary" type="button" onClick={() => { setEditMode(true); setInspectorOpen(true); }}>开始设计</button>}</div>}
          {dragPreview && (() => {
            const layout = dragPreview.dropLayout;
            return <div className="bi-drop-placeholder" aria-hidden="true" style={{ gridColumn: `${layout.x + 1} / span ${layout.w}`, gridRow: `${layout.y + 1} / span ${layout.h}` }} />;
          })()}
          {activeDashboard.widgets.map((widget) => {
            const previewLayout = resizePreview?.widgetId === widget.id ? resizePreview.targetLayout : null;
            const movingPreview = dragPreview?.widgetId === widget.id ? dragPreview : null;
            const layout = movingPreview
              ? movingPreview.visibleOriginLayout
              : previewLayout
              ? projectDashboardWidgetLayout({ ...widget, layout: previewLayout }, activeDashboard.columns, visibleColumns)
              : dragPreview?.layouts[widget.id] ?? projectedLayouts[widget.id] ?? widget.layout;
            const result = widgetResults[widget.id];
            return (
              <section
                className={`bi-widget kind-${widget.kind} ${editMode ? "editable" : ""} ${selectedWidgetId === widget.id ? "selected" : ""} ${draggingId === widget.id ? "dragging" : ""} ${previewLayout ? "resizing" : ""}`}
                data-bi-widget-id={widget.id}
                key={widget.id}
                style={{
                  gridColumn: `${layout.x + 1} / span ${layout.w}`,
                  gridRow: `${layout.y + 1} / span ${layout.h}`,
                  ...(movingPreview ? { transform: `translate3d(${movingPreview.translateX}px, ${movingPreview.translateY}px, 0) scale(1.012)`, transition: "none" } : {}),
                }}
                onClick={() => { if (editMode) { setSelectedWidgetId(widget.id); setInspectorOpen(true); } }}
              >
                <header
                  onPointerDown={(event) => startWidgetDrag(event, widget, layout)}
                >
                  {editMode && <GripVertical size={14} className="bi-drag-grip" />}
                  <div><strong>{widget.title}</strong><small>{sourceLabel(widget, profiles)}</small></div>
                  {result?.dataset && widget.kind !== "markdown" && <span>{result.dataset.rows.length} 行 · {result.dataset.durationMs} ms</span>}
                </header>
                <div className="bi-widget-content"><DashboardWidgetView widget={widget} dataset={result?.dataset} loading={Boolean(result?.loading)} error={result?.error} /></div>
                {editMode && <button
                  className="bi-resize-handle"
                  type="button"
                  aria-label={`调整 ${widget.title} 大小`}
                  onPointerDown={(event) => {
                    event.preventDefault();
                    event.stopPropagation();
                    if (!canvasRef.current) return;
                    const metrics = canvasGridMetrics(canvasRef.current, visibleColumns, activeDashboard.rowHeight);
                    resizeRef.current = {
                      widgetId: widget.id,
                      pointerId: event.pointerId,
                      startX: event.clientX,
                      startY: event.clientY,
                      initialLayout: { ...widget.layout },
                      targetLayout: { ...widget.layout },
                      dashboardColumns: activeDashboard.columns,
                      visibleColumns,
                      columnPitch: metrics.columnPitch,
                      rowPitch: metrics.rowPitch,
                    };
                    setResizePreview({ widgetId: widget.id, targetLayout: { ...widget.layout } });
                    setSelectedWidgetId(widget.id);
                    try {
                      event.currentTarget.setPointerCapture?.(event.pointerId);
                    } catch {
                      // Window-level listeners retain the resize session.
                    }
                  }}
                ><span /></button>}
              </section>
            );
          })}
          </div>
          </div>
        </section>

        {editMode && inspectorOpen && <aside className="bi-inspector">
          <header><div><strong>{selectedWidget ? "组件属性" : "仪表盘属性"}</strong><span>{selectedWidget ? widgetKindLabel(selectedWidget.kind) : "响应式栅格"}</span></div><button className="icon-button subtle" type="button" aria-label="关闭属性面板" onClick={() => setInspectorOpen(false)}><X size={15} /></button></header>
          {selectedWidget ? <WidgetInspector
            widget={selectedWidget}
            dataset={widgetResults[selectedWidget.id]?.dataset}
            profiles={profiles}
            password={selectedWidget.source?.kind === "database" ? profilePasswords[selectedWidget.source.profileId] ?? "" : ""}
            onPassword={(profileId, password) => setProfilePasswords((current) => ({ ...current, [profileId]: password }))}
            onChange={(mutator) => updateWidget(selectedWidget.id, mutator)}
            onDuplicate={() => {
              const copy = {
                ...structuredClone(selectedWidget),
                id: uniqueId("widget"),
                title: `${selectedWidget.title} 副本`,
                layout: findFirstOpenLayout(selectedWidget.layout, activeDashboard.widgets, activeDashboard.columns),
              };
              mutateActiveDashboard((dashboard) => ({ ...dashboard, widgets: [...dashboard.widgets, copy] }));
              setSelectedWidgetId(copy.id);
            }}
            columns={activeDashboard.columns}
            onDelete={() => {
              mutateActiveDashboard((dashboard) => ({
                ...dashboard,
                widgets: compactDashboardWidgets(
                  normalizeDashboardWidgetOrder(dashboard.widgets, dashboard.columns)
                    .filter((widget) => widget.id !== selectedWidget.id),
                  dashboard.columns,
                ),
              }));
              setSelectedWidgetId("");
            }}
          /> : <DashboardInspector dashboard={activeDashboard} dashboardCount={document.dashboards.length} deleteArmed={deleteArmed} onChange={mutateActiveDashboard} onDelete={deleteDashboard} />}
        </aside>}
      </div>
    </div>
  );
}

function WidgetInspector({ widget, dataset, profiles, password, onPassword, onChange, onDuplicate, onDelete, columns }: {
  widget: DashboardWidget;
  dataset?: DashboardDataset;
  profiles: RemoteDatabaseProfile[];
  password: string;
  onPassword: (profileId: string, password: string) => void;
  onChange: (mutator: (widget: DashboardWidget) => DashboardWidget) => void;
  onDuplicate: () => void;
  onDelete: () => void;
  columns: number;
}) {
  const source = widget.source;
  const selectedProfile = source?.kind === "database" ? profiles.find((profile) => profile.id === source.profileId) : undefined;
  const needsPassword = selectedProfile && !["sqlite", "excel"].includes(selectedProfile.engine);
  const update = <K extends keyof DashboardWidget>(key: K, value: DashboardWidget[K]) => onChange((current) => ({ ...current, [key]: value }));
  const updateEncoding = (key: keyof DashboardWidget["encoding"], value: string) => onChange((current) => ({ ...current, encoding: { ...current.encoding, [key]: value } }));
  const updateOptions = <K extends keyof DashboardWidget["options"]>(key: K, value: DashboardWidget["options"][K]) => onChange((current) => ({ ...current, options: { ...current.options, [key]: value } }));
  const updateLayout = (key: keyof DashboardWidget["layout"], value: number) => onChange((current) => {
    const next = { ...current.layout, [key]: value };
    next.w = clamp(next.w, 1, columns);
    next.h = clamp(next.h, 1, 24);
    next.x = clamp(next.x, 0, columns - next.w);
    next.y = clamp(next.y, 0, 4_000);
    return { ...current, layout: next };
  });
  return <div className="bi-inspector-body">
    <InspectorSection title="基本信息">
      <label><span>标题</span><input value={widget.title} onChange={(event) => update("title", event.target.value)} /></label>
      <label><span>组件类型</span><select value={widget.kind} onChange={(event) => onChange((current) => changeWidgetKind(current, event.target.value as DashboardWidgetKind))}>{WIDGET_KINDS.map((kind) => <option key={kind} value={kind}>{widgetKindLabel(kind)}</option>)}</select></label>
    </InspectorSection>
    {widget.kind === "markdown" ? <InspectorSection title="Markdown 内容"><label><textarea className="bi-markdown-editor" value={widget.options.text} onChange={(event) => updateOptions("text", event.target.value)} /></label></InspectorSection> : <>
      <InspectorSection title="数据来源">
        <label><span>来源</span><select value={source?.kind ?? "builtin"} onChange={(event) => {
          const next = event.target.value;
          onChange((current) => ({ ...current, source: next === "database" ? { kind: "database", profileId: "workspace", sql: "SELECT name, type FROM sqlite_master WHERE type IN ('table', 'view') ORDER BY name LIMIT 100" } : { kind: "builtin", dataset: defaultDataset(current.kind) } }));
        }}><option value="builtin">DRPA 内置数据</option><option value="database">数据工作台</option></select></label>
        {source?.kind === "builtin" ? <label><span>数据集</span><select value={source.dataset} onChange={(event) => onChange((current) => ({ ...current, source: { kind: "builtin", dataset: event.target.value as typeof source.dataset } }))}><option value="workspaceSummary">工作区指标</option><option value="runHistory">运行记录</option><option value="runStatus">运行状态汇总</option><option value="packages">RPAZ 包</option></select></label> : source?.kind === "database" ? <>
          <label><span>数据库连接</span><select value={source.profileId} onChange={(event) => onChange((current) => ({ ...current, source: { kind: "database", profileId: event.target.value, sql: source.sql } }))}><option value="workspace">内置工作区 SQLite</option>{profiles.map((profile) => <option key={profile.id} value={profile.id}>{profile.name} · {profile.engine}</option>)}</select></label>
          {needsPassword && <label><span>本次会话密码</span><input type="password" value={password} placeholder="仅保留到应用退出" onChange={(event) => onPassword(source.profileId, event.target.value)} /></label>}
          <label><span>只读 SQL</span><textarea className="bi-sql-editor" spellCheck={false} value={source.sql} onChange={(event) => onChange((current) => ({ ...current, source: { kind: "database", profileId: source.profileId, sql: event.target.value } }))} /></label>
        </> : null}
      </InspectorSection>
      <InspectorSection title="字段映射">
        {(widget.kind === "line" || widget.kind === "bar" || widget.kind === "pie") && <FieldSelect label="分类字段" value={widget.encoding.categoryField} columns={dataset?.columns ?? []} onChange={(value) => updateEncoding("categoryField", value)} />}
        <FieldSelect label={widget.kind === "metric" ? "指标字段" : "数值字段"} value={widget.encoding.valueField} columns={dataset?.columns ?? []} onChange={(value) => updateEncoding("valueField", value)} />
        {(widget.kind === "line" || widget.kind === "bar") && <FieldSelect label="系列字段（预留）" value={widget.encoding.seriesField} columns={dataset?.columns ?? []} onChange={(value) => updateEncoding("seriesField", value)} />}
      </InspectorSection>
      <InspectorSection title="显示">
        <label><span>数值格式</span><select value={widget.options.numberFormat} onChange={(event) => updateOptions("numberFormat", event.target.value as DashboardWidget["options"]["numberFormat"])}><option value="number">数字</option><option value="compact">紧凑数字</option><option value="percent">百分比</option><option value="currency">人民币</option><option value="hours">小时</option><option value="text">文本</option></select></label>
        <label className="bi-color-field"><span>主色</span><input type="color" value={widget.options.color} onChange={(event) => updateOptions("color", event.target.value)} /><code>{widget.options.color}</code></label>
        <label><span>自动刷新</span><select value={widget.options.refreshSeconds} onChange={(event) => updateOptions("refreshSeconds", Number(event.target.value))}><option value={0}>手动</option><option value={10}>10 秒</option><option value={30}>30 秒</option><option value={60}>1 分钟</option><option value={300}>5 分钟</option></select></label>
      </InspectorSection>
    </>}
    <InspectorSection title="栅格位置"><div className="bi-layout-fields"><label><span>X</span><input type="number" min={0} max={columns - 1} value={widget.layout.x} onChange={(event) => updateLayout("x", Number(event.target.value))} /></label><label><span>Y</span><input type="number" min={0} max={4_000} value={widget.layout.y} onChange={(event) => updateLayout("y", Number(event.target.value))} /></label><label><span>宽</span><input type="number" min={1} max={columns} value={widget.layout.w} onChange={(event) => updateLayout("w", Number(event.target.value))} /></label><label><span>高</span><input type="number" min={1} max={24} value={widget.layout.h} onChange={(event) => updateLayout("h", Number(event.target.value))} /></label></div></InspectorSection>
    <div className="bi-inspector-actions"><button className="button secondary" type="button" onClick={onDuplicate}><Copy size={14} /> 复制</button><button className="button danger" type="button" onClick={onDelete}><Trash2 size={14} /> 删除</button></div>
  </div>;
}

function DashboardInspector({ dashboard, dashboardCount, deleteArmed, onChange, onDelete }: { dashboard: DashboardDefinition; dashboardCount: number; deleteArmed: boolean; onChange: (mutator: (dashboard: DashboardDefinition) => DashboardDefinition) => void; onDelete: () => void }) {
  return <div className="bi-inspector-body"><InspectorSection title="主页设置">
    <label><span>名称</span><input value={dashboard.title} onChange={(event) => onChange((current) => ({ ...current, title: event.target.value }))} /></label>
    <label><span>描述</span><textarea value={dashboard.description} onChange={(event) => onChange((current) => ({ ...current, description: event.target.value }))} /></label>
  </InspectorSection><InspectorSection title="栅格系统">
    <label><span>基础列数</span><select value={dashboard.columns} onChange={(event) => onChange((current) => reflowDashboardColumns(current, Number(event.target.value)))}><option value={8}>8 列</option><option value={12}>12 列</option><option value={16}>16 列</option><option value={24}>24 列</option></select></label>
    <label><span>基础行高</span><input type="range" min={40} max={100} step={2} value={dashboard.rowHeight} onChange={(event) => onChange((current) => ({ ...current, rowHeight: Number(event.target.value) }))} /><em>{dashboard.rowHeight}px</em></label>
  </InspectorSection><div className="bi-dashboard-summary"><span><strong>{dashboard.widgets.length}</strong> 个组件</span><span><strong>{dashboard.columns}</strong> 列栅格</span></div>
    {dashboardCount > 1 && <button className="button danger wide" type="button" onClick={onDelete}><Trash2 size={14} /> {deleteArmed ? "再次点击删除仪表盘" : "删除当前仪表盘"}</button>}
  </div>;
}

function InspectorSection({ title, children }: { title: string; children: ReactNode }) {
  return <section className="bi-inspector-section"><h3>{title}</h3>{children}</section>;
}

function FieldSelect({ label, value, columns, onChange }: { label: string; value: string; columns: string[]; onChange: (value: string) => void }) {
  return <label><span>{label}</span><input list={`fields-${label}`} value={value} placeholder="选择或输入字段名" onChange={(event) => onChange(event.target.value)} /><datalist id={`fields-${label}`}>{columns.map((column) => <option key={column} value={column} />)}</datalist></label>;
}

const WIDGET_KINDS: DashboardWidgetKind[] = ["metric", "line", "bar", "pie", "table", "markdown"];

const BUILDER_COMPONENTS: Array<{ kind: DashboardWidgetKind; label: string; description: string; icon: typeof Hash }> = [
  { kind: "metric", label: "指标", description: "突出显示单个关键数值", icon: Hash },
  { kind: "line", label: "折线图", description: "观察时间序列与变化趋势", icon: LineChart },
  { kind: "bar", label: "柱状图", description: "比较不同分类的数据", icon: BarChart3 },
  { kind: "pie", label: "饼图", description: "展示构成与占比", icon: PieChart },
  { kind: "table", label: "数据表", description: "浏览明细记录与字段", icon: Table2 },
  { kind: "markdown", label: "Markdown", description: "添加标题、说明与结论", icon: Type },
];

function createWidget(kind: DashboardWidgetKind, dashboard: DashboardDefinition): DashboardWidget {
  const sizes: Record<DashboardWidgetKind, { w: number; h: number }> = {
    metric: { w: 3, h: 2 }, line: { w: 6, h: 5 }, bar: { w: 6, h: 5 }, pie: { w: 4, h: 5 }, table: { w: 8, h: 5 }, markdown: { w: 4, h: 4 },
  };
  const source = kind === "markdown" ? undefined : { kind: "builtin" as const, dataset: defaultDataset(kind) };
  const layout = findFirstOpenLayout({ x: 0, y: 0, w: Math.min(sizes[kind].w, dashboard.columns), h: sizes[kind].h }, dashboard.widgets, dashboard.columns);
  return {
    id: uniqueId("widget"),
    title: `新建${widgetKindLabel(kind)}`,
    kind,
    layout,
    source,
    encoding: kind === "metric"
      ? { categoryField: "", valueField: "activeRuns", seriesField: "" }
      : kind === "pie"
        ? { categoryField: "status", valueField: "count", seriesField: "" }
        : { categoryField: "startedAt", valueField: "durationSeconds", seriesField: "" },
    options: { text: kind === "markdown" ? "## 业务说明\n\n在这里输入 **Markdown** 描述、结论或数据口径。" : "", numberFormat: "number", color: "#4f6bed", showLegend: true, refreshSeconds: 0 },
  };
}

function changeWidgetKind(widget: DashboardWidget, kind: DashboardWidgetKind): DashboardWidget {
  const next = { ...widget, kind };
  if (kind === "markdown") return { ...next, source: undefined, options: { ...next.options, text: next.options.text || "## 业务说明\n\n在这里输入 Markdown。" } };
  if (!next.source) return { ...next, source: { kind: "builtin", dataset: defaultDataset(kind) } };
  return next;
}

function defaultDataset(kind: DashboardWidgetKind) {
  if (kind === "metric") return "workspaceSummary" as const;
  if (kind === "pie") return "runStatus" as const;
  return "runHistory" as const;
}

function widgetKindLabel(kind: DashboardWidgetKind): string {
  return ({ metric: "指标", line: "折线图", bar: "柱状图", pie: "饼图", table: "数据表", markdown: "Markdown" })[kind];
}

function sourceLabel(widget: DashboardWidget, profiles: RemoteDatabaseProfile[]): string {
  if (widget.kind === "markdown") return "文字描述";
  if (widget.source?.kind === "builtin") return `DRPA · ${({ workspaceSummary: "工作区指标", runHistory: "运行记录", runStatus: "状态汇总", packages: "RPAZ 包" })[widget.source.dataset]}`;
  if (widget.source?.kind === "database") {
    const profileId = widget.source.profileId;
    if (profileId === "workspace") return "数据工作台 · SQLite";
    return `数据工作台 · ${profiles.find((profile) => profile.id === profileId)?.name ?? profileId}`;
  }
  return "未配置数据源";
}

function canvasGridMetrics(canvas: HTMLDivElement, columns: number, rowHeight: number) {
  const rect = canvas.getBoundingClientRect();
  const style = window.getComputedStyle(canvas);
  const paddingLeft = parsePixels(style.paddingLeft);
  const paddingRight = parsePixels(style.paddingRight);
  const paddingTop = parsePixels(style.paddingTop);
  const columnGap = parsePixels(style.columnGap || style.gap);
  const rowGap = parsePixels(style.rowGap || style.gap);
  const availableWidth = Math.max(1, (canvas.clientWidth || rect.width) - paddingLeft - paddingRight - columnGap * Math.max(0, columns - 1));
  const columnWidth = availableWidth / Math.max(1, columns);
  return {
    left: rect.left + paddingLeft - canvas.scrollLeft,
    top: rect.top + paddingTop - canvas.scrollTop,
    columnPitch: Math.max(1, columnWidth + columnGap),
    rowPitch: Math.max(1, rowHeight + rowGap),
  };
}

function parsePixels(value: string) {
  const parsed = Number.parseFloat(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function uniqueId(prefix: string) {
  return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 9)}`;
}

function clamp(value: number, minimum: number, maximum: number) {
  return Math.min(maximum, Math.max(minimum, value));
}
