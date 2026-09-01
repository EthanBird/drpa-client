import {
  Box,
  Braces,
  CheckCircle2,
  CircleAlert,
  Code2,
  GitBranch,
  Link2,
  Maximize2,
  MousePointer2,
  PanelBottom,
  Play,
  Plus,
  Redo2,
  Repeat2,
  Route,
  Search,
  Trash2,
  Undo2,
  X,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type DragEvent,
  type PointerEvent as ReactPointerEvent,
} from "react";

import "../styles/python-flow.css";

export type PythonFlowNodeKind =
  | "start"
  | "python-call"
  | "condition"
  | "loop"
  | "rpa-action"
  | "data"
  | "return"
  | "custom";

export interface PythonFlowSourceBinding {
  filePath?: string;
  symbol?: string;
  startLine?: number;
  endLine?: number;
  sourceHash?: string;
}

export interface PythonFlowPort {
  id: string;
  label: string;
  dataType?: string;
}

export interface PythonFlowNode {
  id: string;
  kind: PythonFlowNodeKind | (string & {});
  title: string;
  description?: string;
  x: number;
  y: number;
  width?: number;
  height?: number;
  inputs?: PythonFlowPort[];
  outputs?: PythonFlowPort[];
  config: Record<string, unknown>;
  source?: PythonFlowSourceBinding;
}

export interface PythonFlowEdge {
  id: string;
  source: string;
  target: string;
  sourceHandle?: string;
  targetHandle?: string;
  label?: string;
  condition?: string;
}

export interface PythonFlowViewport {
  x: number;
  y: number;
  zoom: number;
}

export interface PythonFlowGraph {
  version: 1;
  nodes: PythonFlowNode[];
  edges: PythonFlowEdge[];
  viewport?: PythonFlowViewport;
  entryNodeId?: string;
  sourceFile?: string;
  metadata?: Record<string, unknown>;
}

export type PythonFlowValidationSeverity = "error" | "warning" | "info";

export interface PythonFlowValidationIssue {
  id: string;
  severity: PythonFlowValidationSeverity;
  message: string;
  nodeId?: string;
  edgeId?: string;
  source?: PythonFlowSourceBinding;
}

export interface PythonFlowCatalogItem {
  id: string;
  kind: PythonFlowNode["kind"];
  title: string;
  description: string;
  category: string;
  defaultConfig?: Record<string, unknown>;
  inputs?: PythonFlowPort[];
  outputs?: PythonFlowPort[];
  defaultWidth?: number;
  defaultHeight?: number;
}

export type PythonFlowChangeReason =
  | "add-node"
  | "move-node"
  | "update-node"
  | "delete-selection"
  | "connect"
  | "auto-layout"
  | "code-to-graph"
  | "undo"
  | "redo";

export interface PythonFlowChangeMeta {
  reason: PythonFlowChangeReason;
  selectedNodeId?: string;
}

export type PythonFlowSyncStatus = "synced" | "graph-dirty" | "source-stale" | "conflict";

export interface PythonFlowDesignerProps {
  graph: PythonFlowGraph;
  /** Current Monaco buffer. It remains the canonical source and is previewed read-only here. */
  source: string;
  /** True when source no longer matches graph.sourceHash. */
  sourceStale: boolean;
  /** Parent-owned IPC/save/parse activity flag. */
  busy: boolean;
  catalog?: PythonFlowCatalogItem[];
  validationIssues?: PythonFlowValidationIssue[];
  readOnly?: boolean;
  className?: string;
  onGraphChange: (graph: PythonFlowGraph, meta: PythonFlowChangeMeta) => void;
  /** Validate/render graph in the parent, then replace the active Monaco buffer. */
  onApplyToCode: (graph: PythonFlowGraph) => Promise<void> | void;
  /** Parse the current Monaco buffer in the parent and replace the controlled graph. */
  onRefreshFromCode: (source: string) => Promise<void> | void;
  onNotice: (notice: string) => void;
  onValidate?: (graph: PythonFlowGraph) => Promise<PythonFlowValidationIssue[]> | PythonFlowValidationIssue[];
  onOpenSource?: (binding: PythonFlowSourceBinding) => void;
}

interface NodeDragState {
  nodeId: string;
  pointerId: number;
  clientX: number;
  clientY: number;
  nodeX: number;
  nodeY: number;
  before: PythonFlowGraph;
}

interface Selection {
  nodeId?: string;
  edgeId?: string;
}

const NODE_WIDTH = 216;
const NODE_HEIGHT = 96;
const CANVAS_WIDTH = 3200;
const CANVAS_HEIGHT = 2200;
const MIME_NODE = "application/x-drpa-python-flow-node";

export const DEFAULT_PYTHON_FLOW_CATALOG: PythonFlowCatalogItem[] = [
  {
    id: "start",
    kind: "start",
    title: "流程入口",
    description: "声明 ctx、任务参数与入口变量",
    category: "控制",
    outputs: [{ id: "next", label: "下一步" }],
  },
  {
    id: "python-call",
    kind: "python-call",
    title: "Python 函数",
    description: "调用项目内可解析的 Python 函数",
    category: "Python",
    defaultConfig: { callable: "module.function", arguments: {} },
    inputs: [{ id: "in", label: "输入" }],
    outputs: [{ id: "result", label: "结果" }],
  },
  {
    id: "rpa-for-python",
    kind: "rpa-action",
    title: "RPA for Python",
    description: "调用 click、type、read、download 等动作",
    category: "自动化",
    defaultConfig: { action: "click", target: "", options: {} },
    inputs: [{ id: "in", label: "输入" }],
    outputs: [{ id: "result", label: "结果" }],
  },
  {
    id: "condition",
    kind: "condition",
    title: "条件分支",
    description: "将 Python if/elif 映射为分支",
    category: "控制",
    defaultConfig: { expression: "result is not None" },
    inputs: [{ id: "in", label: "输入" }],
    outputs: [{ id: "true", label: "True" }, { id: "false", label: "False" }],
    defaultHeight: 112,
  },
  {
    id: "loop",
    kind: "loop",
    title: "循环",
    description: "遍历列表或映射 Python 循环",
    category: "控制",
    defaultConfig: { iterable: "items", itemName: "item" },
    inputs: [{ id: "in", label: "输入" }],
    outputs: [{ id: "body", label: "循环体" }, { id: "done", label: "完成" }],
    defaultHeight: 112,
  },
  {
    id: "data",
    kind: "data",
    title: "数据处理",
    description: "变量赋值、映射、SQL 与 ctx 数据操作",
    category: "数据",
    defaultConfig: { expression: "ctx.data" },
    inputs: [{ id: "in", label: "输入" }],
    outputs: [{ id: "result", label: "结果" }],
  },
  {
    id: "return",
    kind: "return",
    title: "流程输出",
    description: "映射 return 并声明 RPAZ 输出",
    category: "控制",
    defaultConfig: { expression: "result" },
    inputs: [{ id: "value", label: "返回值" }],
  },
];

function cloneGraph(graph: PythonFlowGraph): PythonFlowGraph {
  return structuredClone(graph);
}

function graphSignature(graph: PythonFlowGraph): string {
  return JSON.stringify(graph);
}

function createId(prefix: string): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return `${prefix}-${crypto.randomUUID().slice(0, 8)}`;
  }
  return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`;
}

function nodeWidth(node: PythonFlowNode): number {
  return node.width ?? NODE_WIDTH;
}

function nodeHeight(node: PythonFlowNode): number {
  return node.height ?? NODE_HEIGHT;
}

function nodeIcon(kind: PythonFlowNode["kind"]) {
  switch (kind) {
    case "start": return Play;
    case "condition": return GitBranch;
    case "loop": return Repeat2;
    case "rpa-action": return MousePointer2;
    case "data": return Braces;
    case "return": return Route;
    case "python-call": return Code2;
    default: return Box;
  }
}

function nodeSummary(node: PythonFlowNode): string {
  const config = node.config;
  if (node.kind === "python-call") return String(config.callable ?? "选择 Python 函数");
  if (node.kind === "rpa-action") return `${String(config.action ?? "action")} · ${String(config.target ?? "等待目标")}`;
  if (node.kind === "condition") return String(config.expression ?? "等待条件表达式");
  if (node.kind === "loop") return `for ${String(config.itemName ?? "item")} in ${String(config.iterable ?? "items")}`;
  if (node.kind === "data" || node.kind === "return") return String(config.expression ?? "等待表达式");
  return node.description || "Python Flow 节点";
}

function edgePath(source: PythonFlowNode, target: PythonFlowNode): string {
  const x1 = source.x + nodeWidth(source);
  const y1 = source.y + nodeHeight(source) / 2;
  const x2 = target.x;
  const y2 = target.y + nodeHeight(target) / 2;
  const bend = Math.max(72, Math.abs(x2 - x1) * 0.46);
  return `M ${x1} ${y1} C ${x1 + bend} ${y1}, ${x2 - bend} ${y2}, ${x2} ${y2}`;
}

export function validatePythonFlowGraph(graph: PythonFlowGraph): PythonFlowValidationIssue[] {
  const issues: PythonFlowValidationIssue[] = [];
  const nodeIds = new Set<string>();
  for (const node of graph.nodes) {
    if (nodeIds.has(node.id)) {
      issues.push({ id: `duplicate-${node.id}`, severity: "error", nodeId: node.id, message: `节点 ID 重复：${node.id}` });
    }
    nodeIds.add(node.id);
  }
  if (!graph.nodes.some((node) => node.kind === "start")) {
    issues.push({ id: "missing-start", severity: "error", message: "流程缺少入口节点" });
  }
  if (!graph.nodes.some((node) => node.kind === "return")) {
    issues.push({ id: "missing-return", severity: "warning", message: "流程尚未声明输出节点" });
  }
  for (const edge of graph.edges) {
    if (!nodeIds.has(edge.source) || !nodeIds.has(edge.target)) {
      issues.push({ id: `dangling-${edge.id}`, severity: "error", edgeId: edge.id, message: `连线 ${edge.id} 引用了不存在的节点` });
    }
    if (edge.source === edge.target) {
      issues.push({ id: `self-${edge.id}`, severity: "warning", edgeId: edge.id, nodeId: edge.source, message: "节点连接到了自身" });
    }
  }
  const connected = new Set(graph.edges.flatMap((edge) => [edge.source, edge.target]));
  for (const node of graph.nodes) {
    if (graph.nodes.length > 1 && !connected.has(node.id)) {
      issues.push({ id: `isolated-${node.id}`, severity: "warning", nodeId: node.id, message: `${node.title} 尚未连接到流程` });
    }
    if (node.kind === "python-call" && !String(node.config.callable ?? "").trim()) {
      issues.push({ id: `callable-${node.id}`, severity: "error", nodeId: node.id, message: `${node.title} 尚未配置 callable` });
    }
  }
  return issues;
}

export function autoLayoutPythonFlow(graph: PythonFlowGraph): PythonFlowGraph {
  if (!graph.nodes.length) return cloneGraph(graph);
  const ids = new Set(graph.nodes.map((node) => node.id));
  const incoming = new Map(graph.nodes.map((node) => [node.id, 0]));
  const outgoing = new Map(graph.nodes.map((node) => [node.id, [] as string[]]));
  for (const edge of graph.edges) {
    if (!ids.has(edge.source) || !ids.has(edge.target) || edge.source === edge.target) continue;
    incoming.set(edge.target, (incoming.get(edge.target) ?? 0) + 1);
    outgoing.get(edge.source)?.push(edge.target);
  }

  const rank = new Map<string, number>();
  const queue = graph.nodes
    .filter((node) => (incoming.get(node.id) ?? 0) === 0)
    .sort((a, b) => Number(b.kind === "start") - Number(a.kind === "start") || a.y - b.y)
    .map((node) => node.id);
  for (const id of queue) rank.set(id, 0);
  for (let index = 0; index < queue.length; index += 1) {
    const id = queue[index];
    const nextRank = (rank.get(id) ?? 0) + 1;
    for (const target of outgoing.get(id) ?? []) {
      rank.set(target, Math.max(rank.get(target) ?? 0, nextRank));
      incoming.set(target, (incoming.get(target) ?? 1) - 1);
      if (incoming.get(target) === 0) queue.push(target);
    }
  }
  const highestRank = Math.max(0, ...rank.values());
  for (const node of graph.nodes) {
    if (!rank.has(node.id)) rank.set(node.id, highestRank + 1);
  }

  const columns = new Map<number, PythonFlowNode[]>();
  for (const node of graph.nodes) {
    const column = rank.get(node.id) ?? 0;
    columns.set(column, [...(columns.get(column) ?? []), node]);
  }
  for (const nodes of columns.values()) nodes.sort((a, b) => a.y - b.y || a.x - b.x);

  const positions = new Map<string, { x: number; y: number }>();
  let x = 72;
  for (const column of [...columns.keys()].sort((a, b) => a - b)) {
    const nodes = columns.get(column) ?? [];
    let y = 64;
    let widest = NODE_WIDTH;
    for (const node of nodes) {
      positions.set(node.id, { x, y });
      y += nodeHeight(node) + 42;
      widest = Math.max(widest, nodeWidth(node));
    }
    x += widest + 104;
  }

  return {
    ...cloneGraph(graph),
    nodes: graph.nodes.map((node) => ({ ...node, ...(positions.get(node.id) ?? {}) })),
  };
}

function fitViewport(stage: HTMLDivElement, nodes: PythonFlowNode[]): PythonFlowViewport {
  if (!nodes.length) return { x: 48, y: 48, zoom: 1 };
  const minX = Math.min(...nodes.map((node) => node.x));
  const minY = Math.min(...nodes.map((node) => node.y));
  const maxX = Math.max(...nodes.map((node) => node.x + nodeWidth(node)));
  const maxY = Math.max(...nodes.map((node) => node.y + nodeHeight(node)));
  const width = Math.max(1, maxX - minX);
  const height = Math.max(1, maxY - minY);
  const zoom = Math.min(1.3, Math.max(0.3, Math.min((stage.clientWidth - 96) / width, (stage.clientHeight - 116) / height)));
  return {
    zoom,
    x: (stage.clientWidth - width * zoom) / 2 - minX * zoom,
    y: (stage.clientHeight - height * zoom) / 2 - minY * zoom,
  };
}

export function PythonFlowDesigner({
  graph,
  source,
  sourceStale,
  busy,
  catalog = DEFAULT_PYTHON_FLOW_CATALOG,
  validationIssues = [],
  readOnly = false,
  className = "",
  onGraphChange,
  onApplyToCode,
  onRefreshFromCode,
  onNotice,
  onValidate,
  onOpenSource,
}: PythonFlowDesignerProps) {
  const rootRef = useRef<HTMLDivElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const graphRef = useRef<PythonFlowGraph>(cloneGraph(graph));
  const previousIncomingGraph = useRef(graphSignature(graph));
  const undoRef = useRef<PythonFlowGraph[]>([]);
  const redoRef = useRef<PythonFlowGraph[]>([]);
  const [draftGraph, setDraftGraphState] = useState<PythonFlowGraph>(() => cloneGraph(graph));
  const [viewport, setViewport] = useState<PythonFlowViewport>(() => graph.viewport ?? { x: 48, y: 48, zoom: 1 });
  const [selection, setSelection] = useState<Selection>(() => ({ nodeId: graph.entryNodeId ?? graph.nodes[0]?.id }));
  const [nodeDrag, setNodeDrag] = useState<NodeDragState | null>(null);
  const [connectingFrom, setConnectingFrom] = useState<{ nodeId: string; handle?: string } | null>(null);
  const [historyVersion, setHistoryVersion] = useState(0);
  const [catalogQuery, setCatalogQuery] = useState("");
  const [rawConfig, setRawConfig] = useState("");
  const [configError, setConfigError] = useState("");
  const [codeOpen, setCodeOpen] = useState(false);
  const [graphDirty, setGraphDirty] = useState(false);
  const [syncBusy, setSyncBusy] = useState(false);
  const [syncMessage, setSyncMessage] = useState("");
  const [localIssues, setLocalIssues] = useState<PythonFlowValidationIssue[]>([]);
  const [issuesOpen, setIssuesOpen] = useState(false);
  const [validating, setValidating] = useState(false);
  const locked = readOnly || busy;

  const setDraftGraph = useCallback((next: PythonFlowGraph) => {
    graphRef.current = next;
    setDraftGraphState(next);
  }, []);

  useEffect(() => {
    const signature = graphSignature(graph);
    if (signature === previousIncomingGraph.current) return;
    previousIncomingGraph.current = signature;
    setDraftGraph(cloneGraph(graph));
    setGraphDirty(false);
    setSelection((current) => {
      if (current.nodeId && graph.nodes.some((node) => node.id === current.nodeId)) return current;
      if (current.edgeId && graph.edges.some((edge) => edge.id === current.edgeId)) return current;
      return { nodeId: graph.entryNodeId ?? graph.nodes[0]?.id };
    });
  }, [graph, setDraftGraph]);

  const selectedNode = useMemo(
    () => draftGraph.nodes.find((node) => node.id === selection.nodeId) ?? null,
    [draftGraph.nodes, selection.nodeId],
  );
  const selectedEdge = useMemo(
    () => draftGraph.edges.find((edge) => edge.id === selection.edgeId) ?? null,
    [draftGraph.edges, selection.edgeId],
  );

  useEffect(() => {
    if (!selectedNode) {
      setRawConfig("");
      setConfigError("");
      return;
    }
    setRawConfig(JSON.stringify(selectedNode.config, null, 2));
    setConfigError("");
  }, [selectedNode?.id]);

  const builtinIssues = useMemo(() => validatePythonFlowGraph(draftGraph), [draftGraph]);
  const allIssues = useMemo(() => {
    const byId = new Map<string, PythonFlowValidationIssue>();
    for (const issue of [...builtinIssues, ...validationIssues, ...localIssues]) byId.set(issue.id, issue);
    return [...byId.values()];
  }, [builtinIssues, validationIssues, localIssues]);

  const effectiveSyncStatus: PythonFlowSyncStatus = graphDirty && sourceStale
    ? "conflict"
    : graphDirty
      ? "graph-dirty"
      : sourceStale
        ? "source-stale"
        : "synced";

  const emitGraph = useCallback((next: PythonFlowGraph, reason: PythonFlowChangeReason, selectedNodeId?: string) => {
    setDraftGraph(next);
    previousIncomingGraph.current = graphSignature(next);
    setGraphDirty(reason !== "code-to-graph");
    onGraphChange(cloneGraph(next), { reason, selectedNodeId });
  }, [onGraphChange, setDraftGraph]);

  const commitGraph = useCallback((next: PythonFlowGraph, reason: PythonFlowChangeReason, selectedNodeId?: string, before = graphRef.current) => {
    if (locked || graphSignature(next) === graphSignature(before)) return;
    undoRef.current = [...undoRef.current.slice(-79), cloneGraph(before)];
    redoRef.current = [];
    setHistoryVersion((value) => value + 1);
    emitGraph(next, reason, selectedNodeId);
  }, [emitGraph, locked]);

  const updateNode = useCallback((nodeId: string, updater: (node: PythonFlowNode) => PythonFlowNode) => {
    const current = graphRef.current;
    const next = { ...current, nodes: current.nodes.map((node) => node.id === nodeId ? updater(node) : node) };
    commitGraph(next, "update-node", nodeId, current);
  }, [commitGraph]);

  const addNode = useCallback((item: PythonFlowCatalogItem, x: number, y: number) => {
    if (locked) return;
    const id = createId(item.kind.replace(/[^a-z0-9]+/gi, "-").toLowerCase() || "node");
    const node: PythonFlowNode = {
      id,
      kind: item.kind,
      title: item.title,
      description: item.description,
      x: Math.max(0, Math.min(CANVAS_WIDTH - NODE_WIDTH, x)),
      y: Math.max(0, Math.min(CANVAS_HEIGHT - NODE_HEIGHT, y)),
      width: item.defaultWidth ?? NODE_WIDTH,
      height: item.defaultHeight ?? NODE_HEIGHT,
      inputs: item.inputs ? structuredClone(item.inputs) : [{ id: "in", label: "输入" }],
      outputs: item.outputs ? structuredClone(item.outputs) : [{ id: "out", label: "输出" }],
      config: structuredClone(item.defaultConfig ?? {}),
    };
    const current = graphRef.current;
    const next: PythonFlowGraph = {
      ...current,
      nodes: [...current.nodes, node],
      entryNodeId: current.entryNodeId ?? (item.kind === "start" ? id : undefined),
    };
    setSelection({ nodeId: id });
    commitGraph(next, "add-node", id, current);
  }, [commitGraph, locked]);

  const deleteSelection = useCallback(() => {
    if (locked) return;
    const current = graphRef.current;
    let next = current;
    if (selection.nodeId) {
      next = {
        ...current,
        nodes: current.nodes.filter((node) => node.id !== selection.nodeId),
        edges: current.edges.filter((edge) => edge.source !== selection.nodeId && edge.target !== selection.nodeId),
        entryNodeId: current.entryNodeId === selection.nodeId ? undefined : current.entryNodeId,
      };
    } else if (selection.edgeId) {
      next = { ...current, edges: current.edges.filter((edge) => edge.id !== selection.edgeId) };
    }
    if (next === current) return;
    setSelection({});
    setConnectingFrom(null);
    commitGraph(next, "delete-selection", undefined, current);
  }, [commitGraph, locked, selection]);

  const undo = useCallback(() => {
    if (locked) return;
    const previous = undoRef.current.pop();
    if (!previous) return;
    redoRef.current.push(cloneGraph(graphRef.current));
    setHistoryVersion((value) => value + 1);
    emitGraph(previous, "undo");
  }, [emitGraph, locked]);

  const redo = useCallback(() => {
    if (locked) return;
    const next = redoRef.current.pop();
    if (!next) return;
    undoRef.current.push(cloneGraph(graphRef.current));
    setHistoryVersion((value) => value + 1);
    emitGraph(next, "redo");
  }, [emitGraph, locked]);

  const fitView = useCallback(() => {
    if (stageRef.current) setViewport(fitViewport(stageRef.current, graphRef.current.nodes));
  }, []);

  const connectTo = useCallback((targetId: string, targetHandle?: string) => {
    if (!connectingFrom || connectingFrom.nodeId === targetId || locked) return;
    const current = graphRef.current;
    const duplicate = current.edges.some((edge) => edge.source === connectingFrom.nodeId
      && edge.target === targetId
      && edge.sourceHandle === connectingFrom.handle
      && edge.targetHandle === targetHandle);
    if (duplicate) {
      setConnectingFrom(null);
      return;
    }
    const edge: PythonFlowEdge = {
      id: createId("edge"),
      source: connectingFrom.nodeId,
      target: targetId,
      sourceHandle: connectingFrom.handle,
      targetHandle,
      label: connectingFrom.handle && !["out", "next", "result"].includes(connectingFrom.handle) ? connectingFrom.handle : undefined,
    };
    const next = { ...current, edges: [...current.edges, edge] };
    setSelection({ edgeId: edge.id });
    setConnectingFrom(null);
    commitGraph(next, "connect", undefined, current);
  }, [commitGraph, connectingFrom, locked]);

  const onCatalogDragStart = (event: DragEvent<HTMLButtonElement>, item: PythonFlowCatalogItem) => {
    event.dataTransfer.effectAllowed = "copy";
    event.dataTransfer.setData(MIME_NODE, item.id);
    event.dataTransfer.setData("text/plain", item.id);
  };

  const onCanvasDrop = (event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    const itemId = event.dataTransfer.getData(MIME_NODE) || event.dataTransfer.getData("text/plain");
    const item = catalog.find((entry) => entry.id === itemId);
    if (!item || !stageRef.current) return;
    const rect = stageRef.current.getBoundingClientRect();
    addNode(item, (event.clientX - rect.left - viewport.x) / viewport.zoom - NODE_WIDTH / 2, (event.clientY - rect.top - viewport.y) / viewport.zoom - NODE_HEIGHT / 2);
  };

  const startNodeDrag = (event: ReactPointerEvent<HTMLElement>, node: PythonFlowNode) => {
    if (locked || event.button !== 0 || (event.target as HTMLElement).closest("button,input,textarea,select")) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    setNodeDrag({
      nodeId: node.id,
      pointerId: event.pointerId,
      clientX: event.clientX,
      clientY: event.clientY,
      nodeX: node.x,
      nodeY: node.y,
      before: cloneGraph(graphRef.current),
    });
  };

  const moveNode = (event: ReactPointerEvent<HTMLElement>, node: PythonFlowNode) => {
    if (!nodeDrag || nodeDrag.nodeId !== node.id || nodeDrag.pointerId !== event.pointerId) return;
    const x = Math.max(0, Math.min(CANVAS_WIDTH - nodeWidth(node), nodeDrag.nodeX + (event.clientX - nodeDrag.clientX) / viewport.zoom));
    const y = Math.max(0, Math.min(CANVAS_HEIGHT - nodeHeight(node), nodeDrag.nodeY + (event.clientY - nodeDrag.clientY) / viewport.zoom));
    const current = graphRef.current;
    setDraftGraph({ ...current, nodes: current.nodes.map((item) => item.id === node.id ? { ...item, x, y } : item) });
  };

  const finishNodeDrag = (event: ReactPointerEvent<HTMLElement>) => {
    if (!nodeDrag || nodeDrag.pointerId !== event.pointerId) return;
    const next = graphRef.current;
    const before = nodeDrag.before;
    setNodeDrag(null);
    if (graphSignature(next) === graphSignature(before)) return;
    undoRef.current = [...undoRef.current.slice(-79), before];
    redoRef.current = [];
    setHistoryVersion((value) => value + 1);
    emitGraph(next, "move-node", nodeDrag.nodeId);
  };

  const applyAutoLayout = () => {
    const current = graphRef.current;
    const next = autoLayoutPythonFlow(current);
    commitGraph(next, "auto-layout", undefined, current);
    requestAnimationFrame(fitView);
  };

  const runValidation = async () => {
    setValidating(true);
    setIssuesOpen(true);
    try {
      setLocalIssues(onValidate ? await onValidate(cloneGraph(graphRef.current)) : []);
    } catch (error) {
      setLocalIssues([{ id: "validation-adapter", severity: "error", message: error instanceof Error ? error.message : String(error) }]);
    } finally {
      setValidating(false);
    }
  };

  const applyGraphToCode = async () => {
    setCodeOpen(true);
    setSyncBusy(true);
    setSyncMessage("");
    try {
      await onApplyToCode(cloneGraph(graphRef.current));
      setGraphDirty(false);
      setSyncMessage("流程图已应用到代码缓冲区");
      onNotice("Python Flow 已应用到当前代码");
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setSyncMessage(message);
      onNotice(message);
    } finally {
      setSyncBusy(false);
    }
  };

  const refreshGraphFromCode = async () => {
    setCodeOpen(true);
    setSyncBusy(true);
    setSyncMessage("");
    try {
      await onRefreshFromCode(source);
      setGraphDirty(false);
      setSyncMessage("已请求从当前代码刷新流程图");
      onNotice("Python Flow 已从当前代码刷新");
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setSyncMessage(message);
      onNotice(message);
    } finally {
      setSyncBusy(false);
    }
  };

  const applyRawConfig = () => {
    if (!selectedNode) return;
    try {
      const parsed = JSON.parse(rawConfig) as unknown;
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) throw new Error("配置必须是 JSON 对象");
      setConfigError("");
      updateNode(selectedNode.id, (node) => ({ ...node, config: parsed as Record<string, unknown> }));
    } catch (error) {
      setConfigError(error instanceof Error ? error.message : String(error));
    }
  };

  const filteredCatalog = useMemo(() => {
    const query = catalogQuery.trim().toLocaleLowerCase();
    return query
      ? catalog.filter((item) => `${item.title} ${item.description} ${item.category}`.toLocaleLowerCase().includes(query))
      : catalog;
  }, [catalog, catalogQuery]);

  const groupedCatalog = useMemo(() => {
    const groups = new Map<string, PythonFlowCatalogItem[]>();
    for (const item of filteredCatalog) groups.set(item.category, [...(groups.get(item.category) ?? []), item]);
    return [...groups.entries()];
  }, [filteredCatalog]);

  const edgeNodes = useMemo(() => new Map(draftGraph.nodes.map((node) => [node.id, node])), [draftGraph.nodes]);
  const syncLabel = {
    synced: "已同步",
    "graph-dirty": "流程图有改动",
    "source-stale": "源码已变化",
    conflict: "同步冲突",
  }[effectiveSyncStatus];

  const handleRootKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    const target = event.target as HTMLElement;
    const editing = Boolean(target.closest("input,textarea,select,[contenteditable='true']"));
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "z" && !editing) {
      event.preventDefault();
      event.shiftKey ? redo() : undo();
    } else if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "y" && !editing) {
      event.preventDefault();
      redo();
    } else if ((event.key === "Delete" || event.key === "Backspace") && !editing) {
      event.preventDefault();
      deleteSelection();
    } else if ((event.ctrlKey || event.metaKey) && event.key === "0") {
      event.preventDefault();
      fitView();
    }
  };

  return <div
    ref={rootRef}
    className={`python-flow-designer ${className}`.trim()}
    tabIndex={0}
    onKeyDown={handleRootKeyDown}
    aria-label="Python Flow 可视化编辑器"
  >
    <header className="python-flow-toolbar">
      <div className="python-flow-brand"><Route size={16} /><span><strong>Python Flow</strong><small>{draftGraph.sourceFile ?? "RPAZ workflow"}</small></span></div>
      <div className="python-flow-history" aria-label="编辑历史">
        <button type="button" title="撤销 Ctrl+Z" disabled={locked || !undoRef.current.length} onClick={undo}><Undo2 size={14} /></button>
        <button type="button" title="重做 Ctrl+Y" disabled={locked || !redoRef.current.length} onClick={redo}><Redo2 size={14} /></button>
        <span className="python-flow-history-sentinel" aria-hidden="true">{historyVersion}</span>
      </div>
      <button type="button" className="python-flow-tool" onClick={applyAutoLayout} disabled={locked || !draftGraph.nodes.length}><Route size={13} />自动布局</button>
      <button type="button" className="python-flow-tool" disabled={busy || validating} onClick={() => void runValidation()}><CheckCircle2 size={13} />{validating ? "检查中" : "验证"}<span>{allIssues.length}</span></button>
      <button type="button" className={`python-flow-tool ${codeOpen ? "active" : ""}`} onClick={() => setCodeOpen((value) => !value)}><PanelBottom size={13} />代码同步</button>
      <div className={`python-flow-sync-state ${effectiveSyncStatus}`}><span />{syncLabel}</div>
      <div className="python-flow-toolbar-spacer" />
      <button type="button" className="python-flow-tool danger" disabled={locked || (!selectedNode && !selectedEdge)} onClick={deleteSelection}><Trash2 size={13} />删除</button>
    </header>

    <div className="python-flow-grid">
      <aside className="python-flow-catalog">
        <header><Box size={14} /><strong>节点目录</strong></header>
        <label className="python-flow-search"><Search size={13} /><input value={catalogQuery} onChange={(event) => setCatalogQuery(event.target.value)} placeholder="搜索节点" /></label>
        <div className="python-flow-catalog-scroll">
          {groupedCatalog.map(([category, items]) => <section key={category}>
            <h3>{category}</h3>
            {items.map((item) => {
              const Icon = nodeIcon(item.kind);
              return <button
                key={item.id}
                type="button"
                draggable={!locked}
                onDragStart={(event) => onCatalogDragStart(event, item)}
                onDoubleClick={() => addNode(item, 120 + draftGraph.nodes.length * 24, 90 + draftGraph.nodes.length * 22)}
                disabled={locked}
              >
                <span><Icon size={14} /></span><span><strong>{item.title}</strong><small>{item.description}</small></span><Plus size={12} />
              </button>;
            })}
          </section>)}
          {!filteredCatalog.length && <p className="python-flow-empty">没有匹配的节点</p>}
        </div>
        <footer><MousePointer2 size={12} /><span>拖到画布添加，双击快速创建</span></footer>
      </aside>

      <main
        ref={stageRef}
        className="python-flow-canvas"
        onDragOver={(event) => { event.preventDefault(); event.dataTransfer.dropEffect = "copy"; }}
        onDrop={onCanvasDrop}
        onClick={() => { setSelection({}); setConnectingFrom(null); }}
      >
        <div className="python-flow-grid-dots" />
        <div
          className="python-flow-canvas-content"
          style={{ transform: `translate3d(${viewport.x}px, ${viewport.y}px, 0) scale(${viewport.zoom})` }}
        >
          <svg className="python-flow-edges" viewBox={`0 0 ${CANVAS_WIDTH} ${CANVAS_HEIGHT}`} aria-hidden="true">
            <defs><marker id="python-flow-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M 0 0 L 8 4 L 0 8 z" /></marker></defs>
            {draftGraph.edges.map((edge) => {
              const source = edgeNodes.get(edge.source);
              const target = edgeNodes.get(edge.target);
              if (!source || !target) return null;
              const path = edgePath(source, target);
              return <g
                key={edge.id}
                className={selection.edgeId === edge.id ? "selected" : ""}
                onClick={(event) => { event.stopPropagation(); setSelection({ edgeId: edge.id }); }}
              >
                <path className="python-flow-edge-hit" d={path} />
                <path className="python-flow-edge-line" d={path} markerEnd="url(#python-flow-arrow)" />
                {(edge.label || edge.condition) && <text x={(source.x + nodeWidth(source) + target.x) / 2} y={(source.y + target.y) / 2 + 30}>{edge.label || edge.condition}</text>}
              </g>;
            })}
          </svg>

          {draftGraph.nodes.map((node) => {
            const Icon = nodeIcon(node.kind);
            const outputs = node.outputs ?? [];
            const inputs = node.inputs ?? [];
            const style = {
              left: node.x,
              top: node.y,
              width: nodeWidth(node),
              minHeight: nodeHeight(node),
            } satisfies CSSProperties;
            return <article
              key={node.id}
              className={`python-flow-node kind-${node.kind} ${selection.nodeId === node.id ? "selected" : ""}`}
              style={style}
              onClick={(event) => { event.stopPropagation(); setSelection({ nodeId: node.id }); }}
              onPointerDown={(event) => startNodeDrag(event, node)}
              onPointerMove={(event) => moveNode(event, node)}
              onPointerUp={finishNodeDrag}
              onPointerCancel={finishNodeDrag}
            >
              <header><span><Icon size={14} /></span><div><strong>{node.title}</strong><code>{node.kind}</code></div>{node.source?.startLine && <small>L{node.source.startLine}</small>}</header>
              <p>{nodeSummary(node)}</p>
              {inputs.map((port, index) => <button
                key={port.id}
                type="button"
                className={`python-flow-port input ${connectingFrom ? "ready" : ""}`}
                style={{ top: `${((index + 1) / (inputs.length + 1)) * 100}%` }}
                title={`${port.label} · 接收连线`}
                aria-label={`${node.title} ${port.label} 输入`}
                onClick={(event) => { event.stopPropagation(); connectTo(node.id, port.id); }}
              />)}
              {outputs.map((port, index) => <button
                key={port.id}
                type="button"
                className={`python-flow-port output ${connectingFrom?.nodeId === node.id && connectingFrom.handle === port.id ? "active" : ""}`}
                style={{ top: `${((index + 1) / (outputs.length + 1)) * 100}%` }}
                title={`${port.label} · 开始连线`}
                aria-label={`${node.title} ${port.label} 输出`}
                onClick={(event) => { event.stopPropagation(); setConnectingFrom({ nodeId: node.id, handle: port.id }); }}
              >{outputs.length > 1 ? port.label.slice(0, 1) : ""}</button>)}
            </article>;
          })}
        </div>

        {!draftGraph.nodes.length && <div className="python-flow-canvas-empty"><Route size={30} /><strong>从 Python 或节点开始</strong><p>把左侧节点拖到画布，或通过代码同步解析入口函数。</p></div>}

        <div className="python-flow-zoom">
          <button type="button" title="缩小" onClick={(event) => { event.stopPropagation(); setViewport((value) => ({ ...value, zoom: Math.max(0.3, value.zoom - 0.1) })); }}><ZoomOut size={13} /></button>
          <span>{Math.round(viewport.zoom * 100)}%</span>
          <button type="button" title="放大" onClick={(event) => { event.stopPropagation(); setViewport((value) => ({ ...value, zoom: Math.min(2, value.zoom + 0.1) })); }}><ZoomIn size={13} /></button>
          <button type="button" title="适应画布 Ctrl+0" onClick={(event) => { event.stopPropagation(); fitView(); }}><Maximize2 size={13} /></button>
        </div>

        {connectingFrom && <div className="python-flow-connect-banner"><Link2 size={13} /><span>点击目标节点的输入端口完成连接</span><button type="button" onClick={(event) => { event.stopPropagation(); setConnectingFrom(null); }}><X size={12} /></button></div>}

        {effectiveSyncStatus === "conflict" && <div className="python-flow-conflict" role="alert">
          <CircleAlert size={15} />
          <span><strong>代码和流程图都发生了改动</strong><small>选择一个来源后由 AST 适配器重新建立映射。</small></span>
          <button type="button" disabled={busy || syncBusy} onClick={(event) => { event.stopPropagation(); void refreshGraphFromCode(); }}>采用代码</button>
          <button type="button" disabled={busy || syncBusy || readOnly} onClick={(event) => { event.stopPropagation(); void applyGraphToCode(); }}>采用流程图</button>
        </div>}

        {issuesOpen && <section className="python-flow-issues" onClick={(event) => event.stopPropagation()}>
          <header><CheckCircle2 size={13} /><strong>验证问题</strong><span>{allIssues.length}</span><button type="button" onClick={() => setIssuesOpen(false)}><X size={12} /></button></header>
          <div>
            {!allIssues.length && <p className="valid"><CheckCircle2 size={14} />当前流程通过结构验证</p>}
            {allIssues.map((issue) => <button
              key={issue.id}
              type="button"
              className={issue.severity}
              onClick={() => {
                if (issue.nodeId) setSelection({ nodeId: issue.nodeId });
                else if (issue.edgeId) setSelection({ edgeId: issue.edgeId });
                if (issue.source) onOpenSource?.(issue.source);
              }}
            ><CircleAlert size={12} /><span>{issue.message}</span><code>{issue.nodeId ?? issue.edgeId ?? issue.severity}</code></button>)}
          </div>
        </section>}

        {codeOpen && <section className="python-flow-code-panel" onClick={(event) => event.stopPropagation()}>
          <header><Code2 size={14} /><span><strong>Python 代码同步</strong><small>AST 映射保留 source binding，不直接执行代码</small></span><div className={`python-flow-sync-state ${effectiveSyncStatus}`}><span />{syncLabel}</div><button type="button" onClick={() => setCodeOpen(false)}><X size={13} /></button></header>
          <textarea
            value={source}
            readOnly
            spellCheck={false}
            aria-label="Python Flow 源代码"
          />
          <footer>
            <span>{syncMessage || `${source.split("\n").length} 行 · 源码由 Studio Monaco 管理`}</span>
            <button type="button" disabled={busy || syncBusy} onClick={() => void refreshGraphFromCode()}><Route size={12} />代码 → 流程图</button>
            <button type="button" disabled={busy || syncBusy || readOnly} onClick={() => void applyGraphToCode()}><Code2 size={12} />流程图 → 代码</button>
          </footer>
        </section>}
      </main>

      <aside className="python-flow-inspector">
        {selectedNode ? <>
          <header><span>{(() => { const Icon = nodeIcon(selectedNode.kind); return <Icon size={14} />; })()}</span><div><strong>节点属性</strong><small>{selectedNode.id}</small></div><button type="button" title="删除节点" disabled={locked} onClick={deleteSelection}><Trash2 size={13} /></button></header>
          <div className="python-flow-inspector-scroll">
            <label><span>名称</span><input value={selectedNode.title} readOnly={locked} onChange={(event) => updateNode(selectedNode.id, (node) => ({ ...node, title: event.target.value }))} /></label>
            <label><span>说明</span><textarea value={selectedNode.description ?? ""} readOnly={locked} onChange={(event) => updateNode(selectedNode.id, (node) => ({ ...node, description: event.target.value }))} /></label>
            <div className="python-flow-property-pair"><label><span>X</span><input type="number" value={Math.round(selectedNode.x)} readOnly={locked} onChange={(event) => updateNode(selectedNode.id, (node) => ({ ...node, x: Math.max(0, Number(event.target.value) || 0) }))} /></label><label><span>Y</span><input type="number" value={Math.round(selectedNode.y)} readOnly={locked} onChange={(event) => updateNode(selectedNode.id, (node) => ({ ...node, y: Math.max(0, Number(event.target.value) || 0) }))} /></label></div>
            {selectedNode.source && <section className="python-flow-source-binding"><header><Code2 size={12} /><strong>源码映射</strong></header><code>{selectedNode.source.filePath ?? draftGraph.sourceFile ?? "未绑定文件"}</code><span>{selectedNode.source.symbol ?? selectedNode.title} · L{selectedNode.source.startLine ?? "?"}–{selectedNode.source.endLine ?? "?"}</span>{onOpenSource && <button type="button" onClick={() => onOpenSource(selectedNode.source!)}>在编辑器中打开</button>}</section>}
            <label><span>节点配置 JSON</span><textarea className="code" value={rawConfig} readOnly={locked} spellCheck={false} onChange={(event) => setRawConfig(event.target.value)} /></label>
            {configError && <p className="python-flow-config-error"><CircleAlert size={12} />{configError}</p>}
            <button type="button" className="python-flow-apply" disabled={locked} onClick={applyRawConfig}>应用配置</button>
            <section className="python-flow-ports-summary"><header><Link2 size={12} /><strong>端口</strong></header><div>{(selectedNode.inputs ?? []).map((port) => <span key={`in-${port.id}`}><i className="input" />{port.label}<code>{port.dataType ?? "Any"}</code></span>)}{(selectedNode.outputs ?? []).map((port) => <span key={`out-${port.id}`}><i className="output" />{port.label}<code>{port.dataType ?? "Any"}</code></span>)}</div></section>
          </div>
        </> : selectedEdge ? <>
          <header><span><Link2 size={14} /></span><div><strong>连线属性</strong><small>{selectedEdge.id}</small></div><button type="button" title="删除连线" disabled={locked} onClick={deleteSelection}><Trash2 size={13} /></button></header>
          <div className="python-flow-inspector-scroll">
            <div className="python-flow-edge-summary"><code>{selectedEdge.source}</code><span>→</span><code>{selectedEdge.target}</code></div>
            <label><span>标签</span><input value={selectedEdge.label ?? ""} readOnly={locked} onChange={(event) => {
              const current = graphRef.current;
              const next = { ...current, edges: current.edges.map((edge) => edge.id === selectedEdge.id ? { ...edge, label: event.target.value } : edge) };
              commitGraph(next, "update-node", undefined, current);
            }} /></label>
            <label><span>条件表达式</span><textarea value={selectedEdge.condition ?? ""} readOnly={locked} onChange={(event) => {
              const current = graphRef.current;
              const next = { ...current, edges: current.edges.map((edge) => edge.id === selectedEdge.id ? { ...edge, condition: event.target.value } : edge) };
              commitGraph(next, "update-node", undefined, current);
            }} /></label>
          </div>
        </> : <div className="python-flow-inspector-empty"><MousePointer2 size={24} /><strong>选择节点或连线</strong><p>在这里编辑 Python 映射、参数和端口。</p></div>}
      </aside>
    </div>
  </div>;
}

export default PythonFlowDesigner;
