import type {
  PythonFlowEdge as CoreEdge,
  PythonFlowGraph as CoreGraph,
  PythonFlowNode as CoreNode,
} from "../domain/models";
import type {
  PythonFlowEdge as CanvasEdge,
  PythonFlowGraph as CanvasGraph,
  PythonFlowNode as CanvasNode,
  PythonFlowNodeKind,
} from "./PythonFlowDesigner";

const CORE_FLOW_KEY = "drpaCoreFlow";

function canvasKind(type: CoreNode["type"]): PythonFlowNodeKind {
  switch (type) {
    case "start": return "start";
    case "end":
    case "return": return "return";
    case "if": return "condition";
    case "for":
    case "while": return "loop";
    case "rpa-call": return "rpa-action";
    case "call":
    case "ctx-call": return "python-call";
    case "assign": return "data";
    default: return "custom";
  }
}

function canvasConfig(node: CoreNode): Record<string, unknown> {
  const base = {
    pythonFlowType: node.type,
    code: node.code,
    data: structuredClone(node.data),
  };
  switch (node.type) {
    case "call":
    case "ctx-call":
      return {
        ...base,
        callable: String(node.data.callName ?? ""),
        arguments: structuredClone(node.data.arguments ?? []),
        assignTargets: structuredClone(node.data.assignTargets ?? []),
      };
    case "rpa-call": {
      const callName = String(node.data.callName ?? "r.click");
      const args = Array.isArray(node.data.arguments) ? node.data.arguments.map(String) : [];
      return {
        ...base,
        action: callName.split(".").at(-1) ?? "click",
        target: args[0] ?? "",
        arguments: args,
      };
    }
    case "if": return { ...base, expression: String(node.data.test ?? "True") };
    case "for": return {
      ...base,
      loopType: "for",
      itemName: String(node.data.target ?? "item"),
      iterable: String(node.data.iterator ?? "items"),
    };
    case "while": return { ...base, loopType: "while", expression: String(node.data.test ?? "True") };
    case "return": return { ...base, expression: String(node.data.value ?? "") };
    case "assign":
    case "raw-code": return { ...base, expression: node.code };
    default: return base;
  }
}

function canvasNode(node: CoreNode, sourceName: string, sourceHash: string, index: number): CanvasNode {
  return {
    id: node.id,
    kind: canvasKind(node.type),
    title: node.label || node.type,
    description: node.code || (node.type === "end" ? "流程结束" : "Python Flow 节点"),
    x: node.position?.x ?? 72 + Math.floor(index / 7) * 320,
    y: node.position?.y ?? 64 + (index % 7) * 132,
    width: node.type === "start" || node.type === "end" ? 184 : 230,
    height: node.type === "if" || node.type === "for" || node.type === "while" ? 112 : 96,
    inputs: node.type === "start" ? [] : [{ id: "in", label: "输入" }],
    outputs: node.type === "end" || node.type === "return" ? [] : node.type === "if"
      ? [{ id: "true", label: "True" }, { id: "false", label: "False" }]
      : node.type === "for" || node.type === "while"
        ? [{ id: "body", label: "循环体" }, { id: "exit", label: "完成" }]
        : [{ id: "next", label: "下一步" }],
    config: canvasConfig(node),
    source: {
      filePath: sourceName,
      startLine: node.span.startLine,
      endLine: node.span.endLine,
      sourceHash,
    },
  };
}

function edgeHandle(kind: string): string | undefined {
  if (["true", "false", "body", "exit"].includes(kind)) return kind;
  return undefined;
}

export function coreFlowToCanvas(flow: CoreGraph): CanvasGraph {
  return {
    version: 1,
    nodes: flow.nodes.map((node, index) => canvasNode(node, flow.source.name, flow.source.sha256, index)),
    edges: flow.edges.map((edge) => ({
      id: edge.id ?? `edge-${edge.source}-${edge.target}-${edge.kind}`,
      source: edge.source,
      target: edge.target,
      sourceHandle: edgeHandle(edge.kind),
      condition: edge.kind,
      label: edge.label,
    })),
    entryNodeId: flow.nodes.find((node) => node.type === "start")?.id,
    sourceFile: flow.source.name,
    metadata: { [CORE_FLOW_KEY]: structuredClone(flow) },
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function coreType(node: CanvasNode): CoreNode["type"] {
  const configured = node.config.pythonFlowType;
  if (typeof configured === "string" && [
    "start", "end", "assign", "call", "ctx-call", "rpa-call", "if", "for", "while", "try", "return", "raw-code",
  ].includes(configured)) return configured as CoreNode["type"];
  switch (node.kind) {
    case "start": return "start";
    case "return": return "return";
    case "condition": return "if";
    case "loop": return node.config.loopType === "while" ? "while" : "for";
    case "rpa-action": return "rpa-call";
    case "python-call": return String(node.config.callable ?? "").startsWith("ctx.") ? "ctx-call" : "call";
    case "data": return String(node.config.expression ?? "").includes("=") ? "assign" : "raw-code";
    default: return "raw-code";
  }
}

function expressionList(value: unknown): string[] {
  return Array.isArray(value) ? value.map(String) : [];
}

function jsonEqual(left: unknown, right: unknown): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

function keywordExpressions(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((item) => {
    if (!isRecord(item) || typeof item.value !== "string") return [];
    return item.name === null || item.name === undefined
      ? [`**${item.value}`]
      : [`${String(item.name)}=${item.value}`];
  });
}

function callableCode(node: CanvasNode, fallback: string, keywords: unknown): string {
  const callable = String(node.config.callable ?? "").trim();
  if (!callable) return fallback || "pass";
  const args = [...expressionList(node.config.arguments), ...keywordExpressions(keywords)];
  const targets = expressionList(node.config.assignTargets);
  const call = `${callable}(${args.join(", ")})`;
  return targets.length ? `${targets.join(" = ")} = ${call}` : call;
}

function rpaCode(node: CanvasNode, fallback: string, callName: string, keywords: unknown): string {
  const action = String(node.config.action ?? "click").trim() || "click";
  const args = expressionList(node.config.arguments);
  const target = String(node.config.target ?? "").trim();
  if (target) args[0] = target;
  args.push(...keywordExpressions(keywords));
  const owner = callName.includes(".") ? callName.slice(0, callName.lastIndexOf(".")) : "r";
  const generated = `${owner || "r"}.${action}(${args.join(", ")})`;
  return generated.endsWith(".click()") && fallback ? fallback : generated;
}

function coreNode(node: CanvasNode, existing?: CoreNode): CoreNode {
  const type = coreType(node);
  const configuredData = isRecord(node.config.data) ? structuredClone(node.config.data) : {};
  const fallbackCode = String(node.config.code ?? existing?.code ?? "");
  const baseUnchanged = Boolean(
    existing
    && existing.type === type
    && fallbackCode === existing.code
    && jsonEqual(configuredData, existing.data),
  );
  let code = fallbackCode;
  let data: Record<string, unknown> = configuredData;
  if (type === "call" || type === "ctx-call") {
    const callName = String(node.config.callable ?? configuredData.callName ?? "");
    const argumentsList = expressionList(node.config.arguments);
    const assignTargets = expressionList(node.config.assignTargets);
    const unchanged = baseUnchanged
      && callName === String(existing?.data.callName ?? "")
      && jsonEqual(argumentsList, existing?.data.arguments ?? [])
      && jsonEqual(assignTargets, existing?.data.assignTargets ?? []);
    code = unchanged ? existing!.code : callableCode(node, fallbackCode, configuredData.keywords);
    data = {
      ...configuredData,
      statement: code,
      callName,
      arguments: argumentsList,
      keywords: Array.isArray(configuredData.keywords) ? configuredData.keywords : [],
      assignTargets,
    };
  } else if (type === "rpa-call") {
    const action = String(node.config.action ?? "click");
    const originalCallName = String(existing?.data.callName ?? configuredData.callName ?? "r.click");
    const callOwner = originalCallName.includes(".") ? originalCallName.slice(0, originalCallName.lastIndexOf(".")) : "r";
    const callName = `${callOwner || "r"}.${action}`;
    const argumentsList = expressionList(node.config.arguments);
    const target = String(node.config.target ?? "").trim();
    const originalArguments = expressionList(existing?.data.arguments);
    const unchanged = baseUnchanged
      && callName === String(existing?.data.callName ?? "")
      && jsonEqual(argumentsList, originalArguments)
      && target === (originalArguments[0] ?? "");
    code = unchanged ? existing!.code : rpaCode(node, fallbackCode, callName, configuredData.keywords);
    data = {
      ...configuredData,
      statement: code,
      callName,
      arguments: target ? [target, ...argumentsList.slice(1)] : argumentsList,
      keywords: Array.isArray(configuredData.keywords) ? configuredData.keywords : [],
      assignTargets: [],
    };
  } else if (type === "if") {
    data = { ...configuredData, test: String(node.config.expression ?? configuredData.test ?? "True"), body: configuredData.body ?? [], orelse: configuredData.orelse ?? [] };
  } else if (type === "for") {
    data = { ...configuredData, target: String(node.config.itemName ?? configuredData.target ?? "item"), iterator: String(node.config.iterable ?? configuredData.iterator ?? "items"), body: configuredData.body ?? [], orelse: configuredData.orelse ?? [], async: false };
  } else if (type === "while") {
    data = { ...configuredData, test: String(node.config.expression ?? configuredData.test ?? "True"), body: configuredData.body ?? [], orelse: configuredData.orelse ?? [] };
  } else if (type === "return") {
    const expression = String(node.config.expression ?? configuredData.value ?? "").trim();
    const unchanged = baseUnchanged && expression === String(existing?.data.value ?? "");
    code = unchanged ? existing!.code : expression ? `return ${expression}` : "return";
    data = { ...configuredData, statement: code, value: expression };
  } else if (type === "assign" || type === "raw-code") {
    const expression = String(node.config.expression ?? fallbackCode);
    code = baseUnchanged && expression === existing?.code ? existing.code : expression.trim() || "pass";
    data = { ...configuredData, statement: code };
  }
  return {
    id: node.id,
    type,
    label: node.title,
    code,
    span: existing?.span ?? { startLine: 1, startColumn: 0, endLine: 1, endColumn: Math.max(0, code.length) },
    data,
    position: { x: node.x, y: node.y },
  };
}

function canvasEdgeKind(edge: CanvasEdge): string {
  if (edge.condition) return edge.condition;
  if (edge.sourceHandle && !["out", "result"].includes(edge.sourceHandle)) return edge.sourceHandle === "done" ? "exit" : edge.sourceHandle;
  return "next";
}

function childNodeIds(nodes: CoreNode[]): Set<string> {
  const ids = new Set<string>();
  const add = (value: unknown) => {
    if (Array.isArray(value)) for (const item of value) if (typeof item === "string") ids.add(item);
  };
  for (const node of nodes) {
    add(node.data.body);
    add(node.data.orelse);
    add(node.data.finalbody);
    if (Array.isArray(node.data.handlers)) {
      for (const handler of node.data.handlers) if (isRecord(handler)) add(handler.body);
    }
  }
  return ids;
}

function linearPath(canvas: CanvasGraph, first: string | undefined, nodeById: Map<string, CoreNode>): string[] {
  if (!first) return [];
  const path: string[] = [];
  const visited = new Set<string>();
  let current: string | undefined = first;
  while (current && !visited.has(current)) {
    const node = nodeById.get(current);
    if (!node || node.type === "start" || node.type === "end") break;
    path.push(current);
    visited.add(current);
    const next: CanvasEdge | undefined = canvas.edges.find((edge) => edge.source === current && canvasEdgeKind(edge) === "next");
    current = next?.target;
  }
  return path;
}

function compileControlRegions(canvas: CanvasGraph, input: CoreNode[]): CoreNode[] {
  const nodes = input.map((node) => ({ ...node, data: structuredClone(node.data) }));
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const outgoing = (nodeId: string, kind: string) => canvas.edges.find((edge) => edge.source === nodeId && canvasEdgeKind(edge) === kind)?.target;
  for (const node of nodes) {
    if (node.type === "if") {
      const trueTarget = outgoing(node.id, "true");
      const falseTarget = outgoing(node.id, "false");
      if (!trueTarget && !falseTarget) continue;
      let body = linearPath(canvas, trueTarget, byId);
      let orelse = linearPath(canvas, falseTarget, byId);
      if (trueTarget && falseTarget) {
        const falseSet = new Set(orelse);
        const join = body.find((id) => falseSet.has(id));
        if (join) {
          body = body.slice(0, body.indexOf(join));
          orelse = orelse.slice(0, orelse.indexOf(join));
        }
      } else {
        body = body.slice(0, trueTarget ? 1 : 0);
        orelse = orelse.slice(0, falseTarget ? 1 : 0);
      }
      node.data.body = body;
      node.data.orelse = orelse;
    } else if (node.type === "for" || node.type === "while") {
      const bodyTarget = outgoing(node.id, "body");
      if (bodyTarget) node.data.body = linearPath(canvas, bodyTarget, byId);
    }
  }
  return nodes;
}

function compileRootBody(canvas: CanvasGraph, nodes: CoreNode[], previousBody: unknown): string[] {
  const semanticIds = new Set(nodes.filter((node) => node.type !== "start" && node.type !== "end").map((node) => node.id));
  for (const child of childNodeIds(nodes)) semanticIds.delete(child);
  const previous = Array.isArray(previousBody) ? previousBody.filter((id): id is string => typeof id === "string" && semanticIds.has(id)) : [];
  const ordered: string[] = [];
  const visited = new Set<string>();
  const start = nodes.find((node) => node.type === "start")?.id;
  let current = start;
  while (current) {
    const next = canvas.edges.find((edge) => edge.source === current && canvasEdgeKind(edge) === "next" && semanticIds.has(edge.target) && !visited.has(edge.target));
    if (!next) break;
    ordered.push(next.target);
    visited.add(next.target);
    current = next.target;
  }
  for (const id of previous) if (!visited.has(id)) { ordered.push(id); visited.add(id); }
  const positions = new Map(canvas.nodes.map((node) => [node.id, node]));
  for (const id of [...semanticIds].filter((candidate) => !visited.has(candidate)).sort((left, right) => {
    const a = positions.get(left);
    const b = positions.get(right);
    return (a?.x ?? 0) - (b?.x ?? 0) || (a?.y ?? 0) - (b?.y ?? 0);
  })) ordered.push(id);
  return ordered;
}

export function canvasFlowToCore(canvas: CanvasGraph): CoreGraph {
  const stored = canvas.metadata?.[CORE_FLOW_KEY];
  if (!isRecord(stored) || stored.kind !== "drpa.python-flow" || !Array.isArray(stored.nodes)) {
    throw new Error("当前画布缺少 Python AST 投影，请先从代码刷新流程图");
  }
  const base = structuredClone(stored) as unknown as CoreGraph;
  const existing = new Map(base.nodes.map((node) => [node.id, node]));
  const nodes = compileControlRegions(canvas, canvas.nodes.map((node) => coreNode(node, existing.get(node.id))));
  const nodeIds = new Set(nodes.map((node) => node.id));
  const edges: CoreEdge[] = canvas.edges
    .filter((edge) => nodeIds.has(edge.source) && nodeIds.has(edge.target))
    .map((edge) => ({ id: edge.id, source: edge.source, target: edge.target, kind: canvasEdgeKind(edge), label: edge.label }));
  const metadata = { ...base.metadata };
  metadata.body = compileRootBody(canvas, nodes, metadata.body);
  return { ...base, nodes, edges, metadata };
}
