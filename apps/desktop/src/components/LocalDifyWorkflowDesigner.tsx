import {
  Bot,
  Box,
  Braces,
  CheckCircle2,
  Code2,
  Flag,
  GitBranch,
  Globe2,
  Link2,
  LoaderCircle,
  Maximize2,
  MessageSquareReply,
  MousePointer2,
  Play,
  Redo2,
  Save,
  ShieldCheck,
  Trash2,
  Type,
  Undo2,
  Workflow,
  X,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import type {
  LocalDifyApp,
  LocalDifyProvider,
  LocalDifyStreamEvent,
  LocalDifyWorkflowEdge,
  LocalDifyWorkflowGraph,
  LocalDifyWorkflowNode,
  LocalDifyWorkflowValidationReport,
  PackageSummary,
} from "../domain/models";
import { desktopGateway } from "../infra/gateway";
import { SidebarToggle, useSidebarCollapsed } from "./SidebarToggle";

interface LocalDifyWorkflowDesignerProps {
  app: LocalDifyApp;
  providers: LocalDifyProvider[];
  busy: boolean;
  onChange: (app: LocalDifyApp) => void;
  onSave: (app: LocalDifyApp) => Promise<LocalDifyApp>;
  onRunCompleted: () => Promise<void>;
  onNotice: (notice: string) => void;
}

interface WorkflowTrace {
  nodeId: string;
  title: string;
  kind: string;
  status: "running" | "success" | "failed";
  durationMs?: number;
  output?: unknown;
  error?: string;
}

interface DragState {
  id: string;
  pointerX: number;
  pointerY: number;
  nodeX: number;
  nodeY: number;
  original: LocalDifyWorkflowGraph;
}

const catalog = [
  { kind: "llm", title: "LLM", description: "调用应用 Provider", icon: Bot, tone: "violet" },
  { kind: "template-transform", title: "模板转换", description: "组合变量与文本", icon: Type, tone: "cyan" },
  { kind: "if-else", title: "条件分支", description: "按条件选择路径", icon: GitBranch, tone: "amber" },
  { kind: "http-request", title: "HTTP 请求", description: "访问 HTTP API", icon: Globe2, tone: "blue" },
  { kind: "code", title: "代码执行", description: "运行内置 Python", icon: Code2, tone: "green" },
  { kind: "question-classifier", title: "问题分类器", description: "用模型选择问题类别", icon: GitBranch, tone: "green" },
  { kind: "parameter-extractor", title: "参数提取器", description: "从文本提取结构化参数", icon: Braces, tone: "blue" },
  { kind: "variable-aggregator", title: "变量聚合器", description: "合并条件分支变量", icon: Link2, tone: "cyan" },
  { kind: "list-operator", title: "列表操作", description: "筛选、排序与截取列表", icon: Braces, tone: "amber" },
  { kind: "document-extractor", title: "文档提取器", description: "读取工作区文本文件", icon: Type, tone: "rose" },
  { kind: "knowledge-retrieval", title: "知识检索", description: "检索本地知识库", icon: Box, tone: "violet" },
  { kind: "rpaz-package", title: "RPAZ 包", description: "调用可复用自动化包", icon: Box, tone: "cyan" },
  { kind: "answer", title: "直接回复", description: "Chatflow 流式回复", icon: MessageSquareReply, tone: "rose" },
  { kind: "end", title: "结束", description: "输出 Workflow 结果", icon: Flag, tone: "slate" },
] as const;

const nodeMeta = Object.fromEntries(catalog.map((item) => [item.kind, item]));

function cloneGraph(graph: LocalDifyWorkflowGraph): LocalDifyWorkflowGraph {
  return structuredClone(graph);
}

function graphSignature(graph: LocalDifyWorkflowGraph): string {
  return JSON.stringify(graph);
}

function configString(node: LocalDifyWorkflowNode, key: string, fallback = ""): string {
  const value = node.config[key];
  return typeof value === "string" ? value : fallback;
}

function nodeSummary(node: LocalDifyWorkflowNode): string {
  switch (node.kind) {
    case "start": {
      const variables = Array.isArray(node.config.variables) ? node.config.variables : [];
      return `${variables.length} 个输入变量`;
    }
    case "llm":
      return "OpenAI Chat Completions";
    case "template-transform":
      return configString(node, "template", "等待配置模板").slice(0, 42);
    case "if-else":
      return "TRUE / FALSE";
    case "http-request":
      return `${configString(node, "method", "GET").toUpperCase()} ${configString(node, "url", "等待配置 URL")}`;
    case "code":
      return configString(node, "code_language", "python3");
    case "rpaz-package":
      return configString(node, "package_id") || "等待选择 RPAZ 包";
    case "question-classifier": {
      const classes = Array.isArray(node.config.classes) ? node.config.classes : [];
      return `${classes.length} 个分类`;
    }
    case "parameter-extractor": {
      const parameters = Array.isArray(node.config.parameters) ? node.config.parameters : [];
      return `${parameters.length} 个输出参数`;
    }
    case "variable-aggregator": {
      const variables = Array.isArray(node.config.variables) ? node.config.variables : [];
      return `${variables.length} 个候选变量`;
    }
    case "list-operator":
      return "筛选 · 排序 · 限制";
    case "document-extractor":
      return "工作区文本文件 → text";
    case "knowledge-retrieval":
      return `Top ${Number(node.config.top_k ?? 5)}`;
    case "answer":
      return configString(node, "answer", "等待配置回复").slice(0, 42);
    case "end":
      return "输出工作流结果";
    default:
      return node.kind;
  }
}

function nodeIcon(kind: string) {
  if (kind === "start") return Play;
  return nodeMeta[kind]?.icon ?? Workflow;
}

function edgePath(source: LocalDifyWorkflowNode, target: LocalDifyWorkflowNode): string {
  const x1 = source.x + source.width;
  const y1 = source.y + source.height / 2;
  const x2 = target.x;
  const y2 = target.y + target.height / 2;
  const bend = Math.max(70, Math.abs(x2 - x1) * 0.45);
  return `M ${x1} ${y1} C ${x1 + bend} ${y1}, ${x2 - bend} ${y2}, ${x2} ${y2}`;
}

function fittedViewport(stage: HTMLDivElement, nodes: LocalDifyWorkflowNode[]) {
  const minX = Math.min(...nodes.map((node) => node.x));
  const minY = Math.min(...nodes.map((node) => node.y));
  const maxX = Math.max(...nodes.map((node) => node.x + node.width));
  const maxY = Math.max(...nodes.map((node) => node.y + node.height));
  const zoom = Math.min(1.25, Math.max(0.35, Math.min((stage.clientWidth - 110) / (maxX - minX), (stage.clientHeight - 110) / (maxY - minY))));
  return {
    zoom,
    x: (stage.clientWidth - (maxX - minX) * zoom) / 2 - minX * zoom,
    y: (stage.clientHeight - (maxY - minY) * zoom) / 2 - minY * zoom,
  };
}

function promptMessages(node: LocalDifyWorkflowNode): Array<{ role: string; text: string }> {
  const value = node.config.prompt_template;
  if (!Array.isArray(value)) return [];
  return value.map((item) => {
    const message = item as Record<string, unknown>;
    return { role: String(message.role ?? "user"), text: String(message.text ?? "") };
  });
}

function getCondition(node: LocalDifyWorkflowNode) {
  const cases = Array.isArray(node.config.cases) ? node.config.cases as Array<Record<string, unknown>> : [];
  const firstCase = cases[0] ?? {};
  const conditions = Array.isArray(firstCase.conditions) ? firstCase.conditions as Array<Record<string, unknown>> : [];
  const condition = conditions[0] ?? {};
  const selector = Array.isArray(condition.variable_selector) ? condition.variable_selector.map(String) : ["start", "query"];
  return {
    selector: selector.join("."),
    operator: String(condition.comparison_operator ?? "contains"),
    value: String(condition.value ?? ""),
  };
}

function selectorText(node: LocalDifyWorkflowNode, key: string, fallback: string): string {
  const selector = node.config[key];
  return Array.isArray(selector) ? selector.map(String).join(".") : fallback;
}

function selectorValue(value: string): string[] {
  return value.split(".").map((item) => item.trim()).filter(Boolean);
}

function classifierClasses(node: LocalDifyWorkflowNode): Array<{ id: string; name: string }> {
  const classes = Array.isArray(node.config.classes) ? node.config.classes : [];
  return classes.map((item, index) => {
    const value = item && typeof item === "object" ? item as Record<string, unknown> : {};
    return { id: String(value.id ?? index + 1), name: String(value.name ?? `类别 ${index + 1}`) };
  });
}

function graphNeedsProvider(graph: LocalDifyWorkflowGraph): boolean {
  return graph.nodes.some((node) => ["llm", "question-classifier", "parameter-extractor"].includes(node.kind));
}

export function LocalDifyWorkflowDesigner({
  app,
  providers,
  busy,
  onChange,
  onSave,
  onRunCompleted,
  onNotice,
}: LocalDifyWorkflowDesignerProps) {
  const catalogCollapsed = useSidebarCollapsed("workflow-node-catalog");
  const inspectorCollapsed = useSidebarCollapsed("workflow-inspector");
  const stageRef = useRef<HTMLDivElement>(null);
  const fittedAppRef = useRef("");
  const undoRef = useRef<LocalDifyWorkflowGraph[]>([]);
  const redoRef = useRef<LocalDifyWorkflowGraph[]>([]);
  const [selectedNodeId, setSelectedNodeId] = useState(app.workflow.nodes[0]?.id ?? "");
  const [selectedEdgeId, setSelectedEdgeId] = useState("");
  const [connecting, setConnecting] = useState<{ nodeId: string; handle: string } | null>(null);
  const [drag, setDrag] = useState<DragState | null>(null);
  const [validation, setValidation] = useState<LocalDifyWorkflowValidationReport | null>(null);
  const [rawConfig, setRawConfig] = useState("");
  const [rawError, setRawError] = useState("");
  const [query, setQuery] = useState("");
  const [running, setRunning] = useState(false);
  const [trace, setTrace] = useState<WorkflowTrace[]>([]);
  const [result, setResult] = useState("");
  const [debugOpen, setDebugOpen] = useState(false);
  const [packages, setPackages] = useState<PackageSummary[]>([]);
  const [quickPackageId, setQuickPackageId] = useState("");

  const graph = app.workflow;
  const viewport = graph.viewport;
  const selectedNode = graph.nodes.find((node) => node.id === selectedNodeId) ?? null;
  const selectedEdge = graph.edges.find((edge) => edge.id === selectedEdgeId) ?? null;
  const provider = providers.find((item) => item.id === app.providerId);
  const nodeStatus = useMemo(() => new Map(trace.map((item) => [item.nodeId, item.status])), [trace]);

  useEffect(() => {
    setSelectedNodeId((current) => app.workflow.nodes.some((node) => node.id === current) ? current : (app.workflow.nodes[0]?.id ?? ""));
    setSelectedEdgeId("");
    setValidation(null);
    undoRef.current = [];
    redoRef.current = [];
  }, [app.id]);

  useEffect(() => {
    void desktopGateway.getWorkspaceSnapshot()
      .then((snapshot) => {
        setPackages(snapshot.packages);
        setQuickPackageId((current) => current || snapshot.packages[0]?.id || "");
      })
      .catch(() => setPackages([]));
  }, []);

  useEffect(() => {
    if (fittedAppRef.current === app.id || graph.nodes.length === 0) return;
    const frame = window.requestAnimationFrame(() => {
      const stage = stageRef.current;
      if (!stage) return;
      fittedAppRef.current = app.id;
      onChange({ ...app, workflow: { ...graph, viewport: fittedViewport(stage, graph.nodes) } });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [app, graph, onChange]);

  useEffect(() => {
    setRawConfig(selectedNode ? JSON.stringify(selectedNode.config, null, 2) : "");
    setRawError("");
  }, [selectedNodeId]);

  const applyGraph = useCallback((next: LocalDifyWorkflowGraph, track = true) => {
    if (track && graphSignature(next) !== graphSignature(graph)) {
      undoRef.current = [...undoRef.current.slice(-49), cloneGraph(graph)];
      redoRef.current = [];
    }
    onChange({ ...app, workflow: next });
    setValidation(null);
  }, [app, graph, onChange]);

  const updateNode = useCallback((nodeId: string, updater: (node: LocalDifyWorkflowNode) => LocalDifyWorkflowNode, track = true) => {
    const next = cloneGraph(graph);
    next.nodes = next.nodes.map((node) => node.id === nodeId ? updater(node) : node);
    applyGraph(next, track);
  }, [applyGraph, graph]);

  const updateNodeConfig = (key: string, value: unknown) => {
    if (!selectedNode) return;
    updateNode(selectedNode.id, (node) => ({ ...node, config: { ...node.config, [key]: value } }));
  };

  const setPrompt = (role: string, text: string) => {
    if (!selectedNode) return;
    const messages = promptMessages(selectedNode);
    const index = messages.findIndex((message) => message.role === role);
    if (index >= 0) messages[index] = { ...messages[index]!, text };
    else messages.unshift({ role, text });
    updateNodeConfig("prompt_template", messages);
  };

  const setCondition = (field: "selector" | "operator" | "value", value: string) => {
    if (!selectedNode) return;
    const condition = getCondition(selectedNode);
    const next = { ...condition, [field]: value };
    updateNodeConfig("cases", [{
      case_id: "true",
      logical_operator: "and",
      conditions: [{
        id: "condition",
        variable_selector: next.selector.split(".").filter(Boolean),
        comparison_operator: next.operator,
        value: next.value,
      }],
    }]);
  };

  const addNode = async (kind: string, clientX?: number, clientY?: number) => {
    const stage = stageRef.current;
    const bounds = stage?.getBoundingClientRect();
    const x = clientX !== undefined && bounds
      ? (clientX - bounds.left - viewport.x) / viewport.zoom
      : ((stage?.clientWidth ?? 900) / 2 - viewport.x) / viewport.zoom - 110;
    const y = clientY !== undefined && bounds
      ? (clientY - bounds.top - viewport.y) / viewport.zoom
      : ((stage?.clientHeight ?? 600) / 2 - viewport.y) / viewport.zoom - 50;
    try {
      const node = await desktopGateway.createLocalDifyWorkflowNode(kind, Math.max(20, x), Math.max(20, y));
      const next = cloneGraph(graph);
      next.nodes.push(node);
      if (selectedNode && !["end", "answer", "if-else", "question-classifier"].includes(selectedNode.kind)) {
        next.edges.push({
          id: `edge-${Date.now()}-${Math.random().toString(16).slice(2)}`,
          source: selectedNode.id,
          target: node.id,
          sourceHandle: "source",
          targetHandle: "target",
          label: "",
          data: {},
        });
      }
      applyGraph(next);
      setSelectedNodeId(node.id);
      setSelectedEdgeId("");
    } catch (error) {
      onNotice(String(error));
    }
  };

  const connectTo = (targetId: string) => {
    if (!connecting || connecting.nodeId === targetId) return;
    if (graph.edges.some((edge) => edge.source === connecting.nodeId && edge.target === targetId && edge.sourceHandle === connecting.handle)) {
      setConnecting(null);
      return;
    }
    const edge: LocalDifyWorkflowEdge = {
      id: `edge-${Date.now()}-${Math.random().toString(16).slice(2)}`,
      source: connecting.nodeId,
      target: targetId,
      sourceHandle: connecting.handle,
      targetHandle: "target",
      label: connecting.handle === "true" ? "TRUE" : connecting.handle === "false" ? "FALSE" : "",
      data: {},
    };
    const next = cloneGraph(graph);
    next.edges.push(edge);
    applyGraph(next);
    setConnecting(null);
    setSelectedEdgeId(edge.id);
    setSelectedNodeId("");
  };

  const deleteSelection = useCallback(() => {
    if (selectedEdgeId) {
      const next = cloneGraph(graph);
      next.edges = next.edges.filter((edge) => edge.id !== selectedEdgeId);
      applyGraph(next);
      setSelectedEdgeId("");
      return;
    }
    const node = graph.nodes.find((item) => item.id === selectedNodeId);
    if (!node || node.kind === "start") return;
    const next = cloneGraph(graph);
    next.nodes = next.nodes.filter((item) => item.id !== node.id);
    next.edges = next.edges.filter((edge) => edge.source !== node.id && edge.target !== node.id);
    applyGraph(next);
    setSelectedNodeId(next.nodes[0]?.id ?? "");
  }, [applyGraph, graph, selectedEdgeId, selectedNodeId]);

  const undo = useCallback(() => {
    const previous = undoRef.current.pop();
    if (!previous) return;
    redoRef.current.push(cloneGraph(graph));
    onChange({ ...app, workflow: previous });
    setValidation(null);
  }, [app, graph, onChange]);

  const redo = useCallback(() => {
    const next = redoRef.current.pop();
    if (!next) return;
    undoRef.current.push(cloneGraph(graph));
    onChange({ ...app, workflow: next });
    setValidation(null);
  }, [app, graph, onChange]);

  const validate = async () => {
    const report = await desktopGateway.validateLocalDifyWorkflow(graph, app.mode);
    setValidation(report);
    onNotice(report.valid
      ? `工作流校验通过 · ${report.nodeCount} 个节点 · ${report.edgeCount} 条连线`
      : `工作流存在 ${report.issues.filter((issue) => issue.level === "error").length} 个错误`);
    return report;
  };

  const save = useCallback(async () => {
    const report = await desktopGateway.validateLocalDifyWorkflow(graph, app.mode);
    setValidation(report);
    if (!report.valid) {
      onNotice("请先处理工作流校验错误");
      return;
    }
    await onSave(app);
    undoRef.current = [];
    redoRef.current = [];
  }, [app, graph, onNotice, onSave]);

  useEffect(() => {
    const handleKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const editing = target?.matches("input, textarea, select, [contenteditable=true]");
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
        event.preventDefault();
        void save();
      } else if (!editing && event.key === "Delete") {
        deleteSelection();
      } else if (!editing && (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "z") {
        event.preventDefault();
        if (event.shiftKey) redo(); else undo();
      }
    };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [deleteSelection, redo, save, undo]);

  const run = async () => {
    if (!query.trim() || running) return;
    try {
      const report = await validate();
      if (!report.valid) return;
      await onSave(app);
      const requestId = `workflow-${Date.now()}-${Math.random().toString(16).slice(2)}`;
      setTrace([]);
      setResult("");
      setRunning(true);
      setDebugOpen(true);
      let streamed = "";
      const stop = await desktopGateway.listenLocalDifyStream(requestId, (event: LocalDifyStreamEvent) => {
        if (event.type === "nodeStarted") {
          setTrace((current) => [...current.filter((item) => item.nodeId !== event.nodeId), {
            nodeId: event.nodeId,
            title: event.title,
            kind: event.nodeType,
            status: "running",
          }]);
        } else if (event.type === "nodeCompleted") {
          setTrace((current) => current.map((item) => item.nodeId === event.nodeId
            ? { ...item, status: "success", durationMs: event.durationMs, output: event.outputs }
            : item));
        } else if (event.type === "nodeFailed") {
          setTrace((current) => current.map((item) => item.nodeId === event.nodeId
            ? { ...item, status: "failed", durationMs: event.durationMs, error: event.error }
            : item));
        } else if (event.type === "delta") {
          streamed += event.content;
          setResult(streamed);
        }
      });
      try {
        const completed = await desktopGateway.runLocalDifyApp({
          requestId,
          appId: app.id,
          query: query.trim(),
          inputs: { [app.inputKey]: query.trim() },
          user: "local-developer",
          stream: provider?.streaming ?? true,
          conversationId: "",
        });
        setResult(completed.answer);
        onNotice(`工作流运行完成 · ${completed.durationMs}ms · ${completed.usage.totalTokens} tokens`);
        await onRunCompleted();
      } finally {
        stop();
      }
    } catch (error) {
      setResult(`## 工作流运行失败\n\n${String(error)}`);
      onNotice(String(error));
    } finally {
      setRunning(false);
    }
  };

  const setViewport = (next: Partial<LocalDifyWorkflowGraph["viewport"]>) => {
    const updated = cloneGraph(graph);
    updated.viewport = { ...updated.viewport, ...next };
    applyGraph(updated, false);
  };

  const fitView = () => {
    const stage = stageRef.current;
    if (!stage || graph.nodes.length === 0) return;
    setViewport(fittedViewport(stage, graph.nodes));
  };

  const commitRawConfig = () => {
    if (!selectedNode) return;
    try {
      const parsed = JSON.parse(rawConfig) as unknown;
      if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") throw new Error("配置必须是 JSON 对象");
      updateNode(selectedNode.id, (node) => ({ ...node, config: parsed as Record<string, unknown> }));
      setRawError("");
    } catch (error) {
      setRawError(String(error));
    }
  };

  const applyRpazQuickFlow = async () => {
    const selectedPackage = packages.find((item) => item.id === quickPackageId);
    if (!selectedPackage) {
      onNotice("请先选择一个已安装的 RPAZ 包");
      return;
    }
    try {
      const rpaz = await desktopGateway.createLocalDifyWorkflowNode("rpaz-package", 390, 210);
      const start = graph.nodes.find((node) => node.kind === "start") ?? {
        id: "start",
        kind: "start",
        title: "开始",
        x: 80,
        y: 210,
        width: 220,
        height: 84,
        config: { variables: [{ label: "query", variable: "query", type: "paragraph", required: true }] },
      };
      const end = graph.nodes.find((node) => node.kind === "end") ?? {
        id: "end",
        kind: "end",
        title: "结束",
        x: 700,
        y: 210,
        width: 220,
        height: 84,
        config: {},
      };
      const queryParameter = selectedPackage.parameters.find((item) => /^(query|input|text|keyword|url)$/i.test(item.id))
        ?? selectedPackage.parameters.find((item) => item.kind === "string");
      const parameters = Object.fromEntries(selectedPackage.parameters.map((item) => {
        if (item.id === queryParameter?.id) return [item.id, "{{#start.query#}}"];
        if (item.defaultValue !== undefined) return [item.id, item.defaultValue];
        if (item.kind === "boolean") return [item.id, false];
        if (item.kind === "number") return [item.id, 0];
        return [item.id, ""];
      }));
      const startNode: LocalDifyWorkflowNode = { ...start, x: 80, y: 210, config: { ...start.config, variables: [{ label: "query", variable: "query", type: "paragraph", required: true }] } };
      const rpazNode: LocalDifyWorkflowNode = { ...rpaz, x: 390, y: 210, config: { ...rpaz.config, package_id: selectedPackage.id, parameters } };
      const endNode: LocalDifyWorkflowNode = { ...end, x: 700, y: 210, config: { ...end.config, outputs: [{ variable: "answer", value_selector: [rpazNode.id, "result"] }] } };
      applyGraph({
        schema: 1,
        viewport: { x: 70, y: 100, zoom: 1 },
        nodes: [startNode, rpazNode, endNode],
        edges: [
          { id: `edge-${startNode.id}-${rpazNode.id}`, source: startNode.id, target: rpazNode.id, sourceHandle: "source", targetHandle: "target", label: "", data: {} },
          { id: `edge-${rpazNode.id}-${endNode.id}`, source: rpazNode.id, target: endNode.id, sourceHandle: "source", targetHandle: "target", label: "", data: {} },
        ],
      });
      setSelectedNodeId(rpazNode.id);
      setSelectedEdgeId("");
      onNotice(`已生成可运行流程：开始 → ${selectedPackage.name} → 结束`);
    } catch (error) {
      onNotice(`创建 RPAZ 快速流程失败：${String(error)}`);
    }
  };

  return <section className="workflow-designer">
    <header className="workflow-toolbar">
      <div className="workflow-toolbar-title"><Workflow size={15} /><span><strong>工作流编排</strong><small>{graph.nodes.length} nodes · {graph.edges.length} edges</small></span></div>
      <div className="workflow-toolbar-history">
        <button type="button" title="撤销 Ctrl+Z" onClick={undo}><Undo2 size={13} /></button>
        <button type="button" title="重做 Ctrl+Shift+Z" onClick={redo}><Redo2 size={13} /></button>
      </div>
      <div className="workflow-toolbar-actions">
        <select aria-label="RPAZ 快速流程包" value={quickPackageId} onChange={(event) => setQuickPackageId(event.target.value)} disabled={packages.length === 0}>
          {packages.length === 0 ? <option value="">尚未安装 RPAZ 包</option> : packages.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}
        </select>
        <button className="button secondary small" type="button" onClick={() => void applyRpazQuickFlow()} disabled={!quickPackageId}><Box size={13} /> RPAZ 快速流程</button>
        <button className="button secondary small" type="button" onClick={() => void validate()}><ShieldCheck size={13} /> 校验</button>
        <button className="button secondary small" type="button" onClick={() => void save()} disabled={busy}><Save size={13} /> 保存工作流</button>
        <button className="button primary small" type="button" onClick={() => setDebugOpen(true)}><Play size={13} /> 测试运行</button>
      </div>
    </header>

    <div className={`workflow-editor-grid${catalogCollapsed ? " catalog-collapsed" : ""}${inspectorCollapsed ? " inspector-collapsed" : ""}`}>
      {catalogCollapsed ? <SidebarToggle id="workflow-node-catalog" side="left" label="工作流节点侧边栏" restore /> : <aside className="workflow-node-catalog collapsible-sidebar">
        <SidebarToggle id="workflow-node-catalog" side="left" label="工作流节点侧边栏" />
        <header><Braces size={13} /><strong>节点</strong></header>
        <p>点击或拖到画布中</p>
        <div>
          {catalog.filter((item) => app.mode === "advanced-chat" ? item.kind !== "end" : item.kind !== "answer").map((item) => {
            const Icon = item.icon;
            return <button
              key={item.kind}
              type="button"
              draggable
              onDragStart={(event) => event.dataTransfer.setData("application/x-drpa-workflow-node", item.kind)}
              onClick={() => void addNode(item.kind)}
            ><span className={`workflow-kind-icon ${item.tone}`}><Icon size={14} /></span><span><strong>{item.title}</strong><small>{item.description}</small></span></button>;
          })}
        </div>
        <footer><MousePointer2 size={12} /> 拖动节点移动；从右侧端口连接到目标节点。</footer>
      </aside>}

      <div
        className={`workflow-canvas ${connecting ? "connecting" : ""}`}
        ref={stageRef}
        onClick={() => { setSelectedNodeId(""); setSelectedEdgeId(""); }}
        onDragOver={(event) => event.preventDefault()}
        onDrop={(event) => {
          event.preventDefault();
          const kind = event.dataTransfer.getData("application/x-drpa-workflow-node");
          if (kind) void addNode(kind, event.clientX, event.clientY);
        }}
        onWheel={(event) => {
          if (Math.abs(event.deltaY) < 1) return;
          event.preventDefault();
          setViewport({ zoom: Math.min(2, Math.max(0.35, viewport.zoom * (event.deltaY > 0 ? 0.92 : 1.08))) });
        }}
      >
        <div className="workflow-canvas-grid" />
        <div className="workflow-canvas-content" style={{ transform: `translate(${viewport.x}px, ${viewport.y}px) scale(${viewport.zoom})` }}>
          <svg className="workflow-edges" viewBox="0 0 2600 1600" preserveAspectRatio="none">
            {graph.edges.map((edge) => {
              const source = graph.nodes.find((node) => node.id === edge.source);
              const target = graph.nodes.find((node) => node.id === edge.target);
              if (!source || !target) return null;
              const path = edgePath(source, target);
              return <g key={edge.id} className={edge.id === selectedEdgeId ? "selected" : ""} onClick={(event) => { event.stopPropagation(); setSelectedNodeId(""); setSelectedEdgeId(edge.id); }}>
                <path className="workflow-edge-hit" d={path} />
                <path className="workflow-edge-line" d={path} />
                {edge.label && <text x={(source.x + source.width + target.x) / 2} y={(source.y + target.y) / 2 + 30}>{edge.label}</text>}
              </g>;
            })}
          </svg>
          {graph.nodes.map((node) => {
            const Icon = nodeIcon(node.kind);
            const meta = nodeMeta[node.kind];
            const status = nodeStatus.get(node.id);
            return <article
              key={node.id}
              className={`workflow-node ${selectedNodeId === node.id ? "selected" : ""} ${status ?? ""}`}
              style={{ left: node.x, top: node.y, width: node.width, minHeight: node.height }}
              onClick={(event) => { event.stopPropagation(); setSelectedNodeId(node.id); setSelectedEdgeId(""); if (connecting) connectTo(node.id); }}
              onPointerDown={(event) => {
                if ((event.target as HTMLElement).closest("button")) return;
                event.currentTarget.setPointerCapture(event.pointerId);
                setDrag({ id: node.id, pointerX: event.clientX, pointerY: event.clientY, nodeX: node.x, nodeY: node.y, original: cloneGraph(graph) });
              }}
              onPointerMove={(event) => {
                if (!drag || drag.id !== node.id) return;
                const x = Math.max(0, drag.nodeX + (event.clientX - drag.pointerX) / viewport.zoom);
                const y = Math.max(0, drag.nodeY + (event.clientY - drag.pointerY) / viewport.zoom);
                updateNode(node.id, (item) => ({ ...item, x, y }), false);
              }}
              onPointerUp={() => {
                if (drag?.id === node.id && graphSignature(drag.original) !== graphSignature(graph)) {
                  undoRef.current = [...undoRef.current.slice(-49), drag.original];
                  redoRef.current = [];
                }
                setDrag(null);
              }}
            >
              <header><span className={`workflow-kind-icon ${meta?.tone ?? "slate"}`}><Icon size={14} /></span><strong>{node.title}</strong><code>{node.kind}</code></header>
              <p>{nodeSummary(node)}</p>
              {node.kind !== "start" && <button className={`workflow-port input ${connecting ? "ready" : ""}`} type="button" aria-label={`连接到 ${node.title}`} onClick={(event) => { event.stopPropagation(); connectTo(node.id); }} />}
              {node.kind !== "end" && node.kind !== "answer" && node.kind !== "if-else" && node.kind !== "question-classifier" && <button className="workflow-port output" type="button" aria-label={`从 ${node.title} 连接`} onClick={(event) => { event.stopPropagation(); setConnecting({ nodeId: node.id, handle: "source" }); }} />}
              {node.kind === "if-else" && <><button className="workflow-port output branch true" type="button" title="TRUE 分支" onClick={(event) => { event.stopPropagation(); setConnecting({ nodeId: node.id, handle: "true" }); }}>T</button><button className="workflow-port output branch false" type="button" title="FALSE 分支" onClick={(event) => { event.stopPropagation(); setConnecting({ nodeId: node.id, handle: "false" }); }}>F</button></>}
              {node.kind === "question-classifier" && classifierClasses(node).map((item, index, items) => <button
                className="workflow-port output branch classifier"
                key={item.id}
                style={{ top: `${24 + (index * 52) / Math.max(1, items.length - 1)}%` }}
                type="button"
                title={`${item.name} 分支`}
                onClick={(event) => { event.stopPropagation(); setConnecting({ nodeId: node.id, handle: item.id }); }}
              >{index + 1}</button>)}
              {status && <span className={`workflow-node-status ${status}`}>{status === "running" ? <LoaderCircle className="spin" size={12} /> : status === "success" ? <CheckCircle2 size={12} /> : <X size={12} />}</span>}
            </article>;
          })}
        </div>
        <div className="workflow-zoom-controls">
          <button type="button" title="缩小" onClick={() => setViewport({ zoom: Math.max(0.35, viewport.zoom - 0.1) })}><ZoomOut size={13} /></button>
          <span>{Math.round(viewport.zoom * 100)}%</span>
          <button type="button" title="放大" onClick={() => setViewport({ zoom: Math.min(2, viewport.zoom + 0.1) })}><ZoomIn size={13} /></button>
          <button type="button" title="适应画布" onClick={fitView}><Maximize2 size={13} /></button>
        </div>
        {connecting && <div className="workflow-connecting-banner"><Link2 size={13} /> 选择目标节点完成连接 <button type="button" onClick={() => setConnecting(null)}><X size={12} /></button></div>}
      </div>

      {inspectorCollapsed ? <SidebarToggle id="workflow-inspector" side="right" label="工作流配置侧边栏" restore /> : <aside className="workflow-inspector collapsible-sidebar">
        <SidebarToggle id="workflow-inspector" side="right" label="工作流配置侧边栏" />
        {selectedNode ? <>
          <header><span className={`workflow-kind-icon ${nodeMeta[selectedNode.kind]?.tone ?? "slate"}`}>{(() => { const Icon = nodeIcon(selectedNode.kind); return <Icon size={14} />; })()}</span><div><strong>节点配置</strong><small>{selectedNode.id}</small></div>{selectedNode.kind !== "start" && <button type="button" title="删除节点" onClick={deleteSelection}><Trash2 size={13} /></button>}</header>
          <div className="workflow-inspector-scroll">
            <label><span>节点标题</span><input value={selectedNode.title} onChange={(event) => updateNode(selectedNode.id, (node) => ({ ...node, title: event.target.value }))} /></label>
            {selectedNode.kind === "start" && <StartInspector node={selectedNode} onConfig={updateNodeConfig} />}
            {selectedNode.kind === "llm" && <>
              <label><span>系统提示词</span><textarea value={promptMessages(selectedNode).find((item) => item.role === "system")?.text ?? ""} onChange={(event) => setPrompt("system", event.target.value)} /></label>
              <label><span>用户提示词</span><textarea className="large" value={promptMessages(selectedNode).find((item) => item.role === "user")?.text ?? ""} onChange={(event) => setPrompt("user", event.target.value)} /></label>
              <p className="workflow-field-help">使用 <code>{"{{#节点ID.变量#}}"}</code> 引用上游输出。</p>
            </>}
            {selectedNode.kind === "template-transform" && <label><span>Jinja 模板</span><textarea className="large" value={configString(selectedNode, "template")} onChange={(event) => updateNodeConfig("template", event.target.value)} /></label>}
            {selectedNode.kind === "if-else" && <ConditionInspector condition={getCondition(selectedNode)} onChange={setCondition} />}
            {selectedNode.kind === "http-request" && <HttpInspector node={selectedNode} onConfig={updateNodeConfig} />}
            {selectedNode.kind === "code" && <>
              <label><span>运行语言</span><select value={configString(selectedNode, "code_language", "python3")} onChange={(event) => updateNodeConfig("code_language", event.target.value)}><option value="python3">Python 3（本地可运行）</option><option value="javascript">JavaScript（导出 Dify）</option></select></label>
              <label><span>代码</span><textarea className="code" spellCheck={false} value={configString(selectedNode, "code")} onChange={(event) => updateNodeConfig("code", event.target.value)} /></label>
            </>}
            {selectedNode.kind === "question-classifier" && <QuestionClassifierInspector node={selectedNode} onConfig={updateNodeConfig} />}
            {selectedNode.kind === "parameter-extractor" && <ParameterExtractorInspector node={selectedNode} onConfig={updateNodeConfig} onError={setRawError} />}
            {selectedNode.kind === "variable-aggregator" && <VariableAggregatorInspector node={selectedNode} onConfig={updateNodeConfig} />}
            {selectedNode.kind === "list-operator" && <ListOperatorInspector node={selectedNode} onConfig={updateNodeConfig} />}
            {selectedNode.kind === "document-extractor" && <SelectorInspector node={selectedNode} configKey="variable_selector" label="文件路径变量" fallback="start.file_path" onConfig={updateNodeConfig} />}
            {selectedNode.kind === "knowledge-retrieval" && <KnowledgeRetrievalInspector node={selectedNode} onConfig={updateNodeConfig} />}
            {selectedNode.kind === "rpaz-package" && <>
              <label><span>已安装 RPAZ 包</span><select value={configString(selectedNode, "package_id")} onChange={(event) => updateNodeConfig("package_id", event.target.value)}><option value="">请选择 RPAZ 包</option>{packages.map((item) => <option key={item.id} value={item.id}>{item.name} · v{item.version}</option>)}</select></label>
              <label><span>参数 JSON</span><textarea key={selectedNode.id} className="code" spellCheck={false} defaultValue={JSON.stringify(selectedNode.config.parameters ?? { input: "{{#start.query#}}" }, null, 2)} onBlur={(event) => { try { updateNodeConfig("parameters", JSON.parse(event.target.value)); setRawError(""); } catch (error) { setRawError(`参数 JSON 无效：${String(error)}`); } }} /></label>
              <p className="workflow-field-help">字符串参数支持 <code>{"{{#节点ID.变量#}}"}</code> 引用；包返回的对象字段可供下游节点继续使用。</p>
            </>}
            {selectedNode.kind === "answer" && <label><span>回复模板</span><textarea className="large" value={configString(selectedNode, "answer")} onChange={(event) => updateNodeConfig("answer", event.target.value)} /></label>}
            {selectedNode.kind === "end" && <EndInspector node={selectedNode} onConfig={updateNodeConfig} />}
            <details className="workflow-raw-config"><summary>高级 JSON 配置</summary><textarea aria-label="工作流节点 JSON 配置" spellCheck={false} value={rawConfig} onChange={(event) => setRawConfig(event.target.value)} /><button className="button secondary small" type="button" onClick={commitRawConfig}>应用 JSON</button>{rawError && <p>{rawError}</p>}</details>
          </div>
        </> : selectedEdge ? <>
          <header><span className="workflow-kind-icon blue"><Link2 size={14} /></span><div><strong>连线配置</strong><small>{selectedEdge.id}</small></div><button type="button" title="删除连线" onClick={deleteSelection}><Trash2 size={13} /></button></header>
          <div className="workflow-inspector-scroll">
            <label><span>标签</span><input value={selectedEdge.label} onChange={(event) => { const next = cloneGraph(graph); next.edges = next.edges.map((edge) => edge.id === selectedEdge.id ? { ...edge, label: event.target.value } : edge); applyGraph(next); }} /></label>
            <label><span>源 Handle</span><input value={selectedEdge.sourceHandle} onChange={(event) => { const next = cloneGraph(graph); next.edges = next.edges.map((edge) => edge.id === selectedEdge.id ? { ...edge, sourceHandle: event.target.value } : edge); applyGraph(next); }} /></label>
            <div className="workflow-edge-summary"><code>{selectedEdge.source}</code><Link2 size={13} /><code>{selectedEdge.target}</code></div>
          </div>
        </> : <div className="workflow-inspector-empty"><MousePointer2 size={25} /><strong>选择节点或连线</strong><p>在画布中选择元素后编辑属性。</p></div>}
      </aside>}
    </div>

    {validation && <div className={`workflow-validation-bar ${validation.valid ? "valid" : "invalid"}`}><ShieldCheck size={13} /><strong>{validation.valid ? "工作流有效" : "工作流需要修复"}</strong><span>{validation.issues.length ? validation.issues.map((issue) => issue.message).join(" · ") : "节点、连线与输出路径校验通过"}</span><button type="button" onClick={() => setValidation(null)}><X size={12} /></button></div>}

    {debugOpen && <aside className="workflow-debug-drawer">
      <header><div><Play size={14} /><span><strong>运行调试</strong><small>{provider?.name ?? (graphNeedsProvider(graph) ? "尚未选择 Provider" : "本地节点流程 · 无需 Provider")}</small></span></div><button type="button" aria-label="关闭工作流调试" onClick={() => setDebugOpen(false)}><X size={14} /></button></header>
      <div className="workflow-debug-input"><textarea aria-label="工作流测试输入" value={query} onChange={(event) => setQuery(event.target.value)} placeholder={`输入 ${app.inputKey} 的测试值…`} /><button className="button primary" type="button" aria-label="运行工作流" disabled={running || !query.trim()} onClick={() => void run()}>{running ? <LoaderCircle className="spin" size={13} /> : <Play size={13} />} 运行</button></div>
      <div className="workflow-debug-body">
        <section className="workflow-trace"><header>节点轨迹</header>{trace.map((item, index) => <article key={`${item.nodeId}-${index}`} className={item.status}><span>{index + 1}</span><div><strong>{item.title}</strong><small>{item.kind}{item.durationMs !== undefined ? ` · ${item.durationMs}ms` : ""}</small></div>{item.status === "running" ? <LoaderCircle className="spin" size={13} /> : item.status === "success" ? <CheckCircle2 size={13} /> : <X size={13} />}</article>)}{trace.length === 0 && <p>运行后实时显示节点状态与耗时。</p>}</section>
        <section className="workflow-result"><header>最终输出</header><div>{result ? <ReactMarkdown remarkPlugins={[remarkGfm]}>{result}</ReactMarkdown> : <p>等待运行结果…</p>}</div></section>
      </div>
    </aside>}
  </section>;
}

function StartInspector({ node, onConfig }: { node: LocalDifyWorkflowNode; onConfig: (key: string, value: unknown) => void }) {
  const variables = Array.isArray(node.config.variables) ? node.config.variables as Array<Record<string, unknown>> : [];
  const first = variables[0] ?? { variable: "query", label: "query", type: "paragraph", required: true };
  const update = (value: string) => onConfig("variables", [{ ...first, variable: value, label: value }]);
  return <><label><span>主输入变量</span><input value={String(first.variable ?? "query")} onChange={(event) => update(event.target.value)} /></label><p className="workflow-field-help">额外输入变量可在高级 JSON 配置中添加。</p></>;
}

function SelectorInspector({ node, configKey, label, fallback, onConfig }: { node: LocalDifyWorkflowNode; configKey: string; label: string; fallback: string; onConfig: (key: string, value: unknown) => void }) {
  return <label><span>{label}</span><input value={selectorText(node, configKey, fallback)} onChange={(event) => onConfig(configKey, selectorValue(event.target.value))} placeholder={fallback} /></label>;
}

function QuestionClassifierInspector({ node, onConfig }: { node: LocalDifyWorkflowNode; onConfig: (key: string, value: unknown) => void }) {
  const classes = classifierClasses(node);
  return <>
    <SelectorInspector node={node} configKey="query_variable_selector" label="待分类变量" fallback="start.query" onConfig={onConfig} />
    <label><span>分类（每行一个）</span><textarea value={classes.map((item) => item.name).join("\n")} onChange={(event) => {
      const names = event.target.value.split(/\r?\n/).map((item) => item.trim()).filter(Boolean);
      onConfig("classes", names.map((name, index) => ({ id: classes[index]?.id ?? String(index + 1), name })));
    }} /></label>
    <p className="workflow-field-help">每个分类会生成独立的右侧连接端口，运行时使用当前流程 Provider 判定分支。</p>
  </>;
}

function ParameterExtractorInspector({ node, onConfig, onError }: { node: LocalDifyWorkflowNode; onConfig: (key: string, value: unknown) => void; onError: (message: string) => void }) {
  const parameters = Array.isArray(node.config.parameters) ? node.config.parameters : [];
  return <>
    <SelectorInspector node={node} configKey="query" label="输入变量" fallback="start.query" onConfig={onConfig} />
    <label><span>提取指令</span><textarea value={configString(node, "instruction", "从输入文本中提取结构化参数。")} onChange={(event) => onConfig("instruction", event.target.value)} /></label>
    <label><span>参数定义 JSON</span><textarea key={node.id} className="code" spellCheck={false} defaultValue={JSON.stringify(parameters, null, 2)} onBlur={(event) => {
      try {
        const value = JSON.parse(event.target.value);
        if (!Array.isArray(value)) throw new Error("必须是数组");
        onConfig("parameters", value);
        onError("");
      } catch (error) {
        onError(`参数定义 JSON 无效：${String(error)}`);
      }
    }} /></label>
  </>;
}

function VariableAggregatorInspector({ node, onConfig }: { node: LocalDifyWorkflowNode; onConfig: (key: string, value: unknown) => void }) {
  const variables = Array.isArray(node.config.variables) ? node.config.variables as unknown[] : [];
  const text = variables.map((value) => Array.isArray(value) ? value.map(String).join(".") : "").filter(Boolean).join("\n");
  return <>
    <label><span>候选变量（每行一个）</span><textarea value={text} onChange={(event) => onConfig("variables", event.target.value.split(/\r?\n/).map(selectorValue).filter((value) => value.length >= 2))} placeholder={"if-else.result\nllm.text"} /></label>
    <p className="workflow-field-help">与 Dify 变量聚合器一致，按顺序选择第一个非空分支结果，输出为 <code>output</code>。</p>
  </>;
}

function ListOperatorInspector({ node, onConfig }: { node: LocalDifyWorkflowNode; onConfig: (key: string, value: unknown) => void }) {
  const filter = node.config.filter_by && typeof node.config.filter_by === "object" && !Array.isArray(node.config.filter_by) ? node.config.filter_by as Record<string, unknown> : {};
  const conditions = Array.isArray(filter.conditions) ? filter.conditions as Array<Record<string, unknown>> : [];
  const condition = conditions[0] ?? {};
  const order = node.config.order_by && typeof node.config.order_by === "object" && !Array.isArray(node.config.order_by) ? node.config.order_by as Record<string, unknown> : {};
  const limit = node.config.limit && typeof node.config.limit === "object" && !Array.isArray(node.config.limit) ? node.config.limit as Record<string, unknown> : {};
  const setCondition = (key: string, value: unknown) => onConfig("filter_by", { enabled: true, conditions: [{ ...condition, [key]: value }] });
  return <>
    <SelectorInspector node={node} configKey="variable" label="数组变量" fallback="start.items" onConfig={onConfig} />
    <label><span>过滤字段（留空表示元素本身）</span><input value={String(condition.key ?? "")} onChange={(event) => setCondition("key", event.target.value)} /></label>
    <label><span>过滤方式</span><select value={String(condition.comparison_operator ?? "is")} onChange={(event) => setCondition("comparison_operator", event.target.value)}><option value="is">等于</option><option value="is_not">不等于</option><option value="contains">包含</option><option value="greater_than">大于</option><option value="less_than">小于</option><option value="is_not_empty">不为空</option></select></label>
    <label><span>过滤值</span><input value={String(condition.value ?? "")} onChange={(event) => setCondition("value", event.target.value)} /></label>
    <label><span>排序字段</span><input value={String(order.key ?? "")} onChange={(event) => onConfig("order_by", { ...order, enabled: Boolean(event.target.value), key: event.target.value })} /></label>
    <label><span>排序方向</span><select value={String(order.value ?? "asc")} onChange={(event) => onConfig("order_by", { ...order, enabled: true, value: event.target.value })}><option value="asc">升序</option><option value="desc">降序</option></select></label>
    <label><span>最多输出</span><input type="number" min={1} max={10000} value={Number(limit.size ?? 10)} onChange={(event) => onConfig("limit", { enabled: true, size: Math.max(1, Number(event.target.value) || 1) })} /></label>
  </>;
}

function KnowledgeRetrievalInspector({ node, onConfig }: { node: LocalDifyWorkflowNode; onConfig: (key: string, value: unknown) => void }) {
  return <>
    <SelectorInspector node={node} configKey="query_variable_selector" label="检索词变量" fallback="start.query" onConfig={onConfig} />
    <label><span>返回条数</span><input type="number" min={1} max={50} value={Number(node.config.top_k ?? 5)} onChange={(event) => onConfig("top_k", Math.max(1, Math.min(50, Number(event.target.value) || 1)))} /></label>
    <p className="workflow-field-help">检索当前工作区知识库中的 Markdown、文本、JSON、CSV 与 YAML 文件，输出 <code>result</code> 和拼接后的 <code>text</code>。</p>
  </>;
}

function ConditionInspector({ condition, onChange }: { condition: ReturnType<typeof getCondition>; onChange: (field: "selector" | "operator" | "value", value: string) => void }) {
  return <>
    <label><span>变量选择器</span><input value={condition.selector} onChange={(event) => onChange("selector", event.target.value)} placeholder="start.query" /></label>
    <label><span>比较方式</span><select value={condition.operator} onChange={(event) => onChange("operator", event.target.value)}><option value="contains">包含</option><option value="not_contains">不包含</option><option value="is">等于</option><option value="is not">不等于</option><option value="starts_with">开头是</option><option value="ends_with">结尾是</option><option value="is_empty">为空</option><option value="is_not_empty">不为空</option><option value="greater_than">大于</option><option value="less_than">小于</option></select></label>
    <label><span>比较值</span><input value={condition.value} onChange={(event) => onChange("value", event.target.value)} /></label>
  </>;
}

function HttpInspector({ node, onConfig }: { node: LocalDifyWorkflowNode; onConfig: (key: string, value: unknown) => void }) {
  const body = node.config.body && typeof node.config.body === "object" && !Array.isArray(node.config.body) ? node.config.body as Record<string, unknown> : {};
  return <>
    <div className="workflow-http-line"><select value={configString(node, "method", "get")} onChange={(event) => onConfig("method", event.target.value)}><option value="get">GET</option><option value="post">POST</option><option value="put">PUT</option><option value="patch">PATCH</option><option value="delete">DELETE</option></select><input aria-label="HTTP 节点 URL" value={configString(node, "url")} onChange={(event) => onConfig("url", event.target.value)} /></div>
    <label><span>Headers（每行 name: value）</span><textarea value={configString(node, "headers")} onChange={(event) => onConfig("headers", event.target.value)} /></label>
    <label><span>请求体</span><textarea className="large" value={typeof body.data === "string" ? body.data : JSON.stringify(body.data ?? "", null, 2)} onChange={(event) => onConfig("body", { ...body, type: "raw-text", data: event.target.value })} /></label>
  </>;
}

function EndInspector({ node, onConfig }: { node: LocalDifyWorkflowNode; onConfig: (key: string, value: unknown) => void }) {
  const outputs = Array.isArray(node.config.outputs) ? node.config.outputs as Array<Record<string, unknown>> : [];
  const first = outputs[0] ?? { variable: "answer", value_selector: ["llm", "text"] };
  const selector = Array.isArray(first.value_selector) ? first.value_selector.map(String).join(".") : "llm.text";
  return <><label><span>输出名称</span><input value={String(first.variable ?? "answer")} onChange={(event) => onConfig("outputs", [{ ...first, variable: event.target.value }])} /></label><label><span>变量选择器</span><input value={selector} onChange={(event) => onConfig("outputs", [{ ...first, value_selector: event.target.value.split(".").filter(Boolean) }])} /></label></>;
}
