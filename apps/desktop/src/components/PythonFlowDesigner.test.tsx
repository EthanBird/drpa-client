import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  PythonFlowDesigner,
  autoLayoutPythonFlow,
  validatePythonFlowGraph,
  type PythonFlowGraph,
} from "./PythonFlowDesigner";

afterEach(cleanup);

function graphFixture(): PythonFlowGraph {
  return {
    version: 1,
    entryNodeId: "start",
    nodes: [
      { id: "start", kind: "start", title: "入口", x: 800, y: 500, width: 180, height: 80, config: {} },
      { id: "call", kind: "python-call", title: "调用", x: 120, y: 700, width: 250, height: 120, config: { callable: "tasks.fetch" } },
      { id: "branch", kind: "condition", title: "判断", x: 400, y: 30, width: 210, height: 110, config: { expression: "result.ok" } },
      { id: "done", kind: "return", title: "输出", x: 40, y: 30, width: 190, height: 80, config: { expression: "result" } },
    ],
    edges: [
      { id: "e1", source: "start", target: "call" },
      { id: "e2", source: "call", target: "branch" },
      { id: "e3", source: "branch", target: "done" },
    ],
  };
}

describe("Python Flow graph helpers", () => {
  it("lays a workflow out by dependency rank while respecting variable node sizes", () => {
    const original = graphFixture();
    const laidOut = autoLayoutPythonFlow(original);
    const byId = new Map(laidOut.nodes.map((node) => [node.id, node]));

    expect(byId.get("start")!.x).toBeLessThan(byId.get("call")!.x);
    expect(byId.get("call")!.x).toBeLessThan(byId.get("branch")!.x);
    expect(byId.get("branch")!.x).toBeLessThan(byId.get("done")!.x);
    expect(original.nodes[0].x).toBe(800);
  });

  it("keeps cyclic nodes finite and places them after acyclic roots", () => {
    const graph = graphFixture();
    graph.edges.push({ id: "cycle", source: "done", target: "call" });
    const laidOut = autoLayoutPythonFlow(graph);

    expect(laidOut.nodes.every((node) => Number.isFinite(node.x) && Number.isFinite(node.y))).toBe(true);
    expect(new Set(laidOut.nodes.map((node) => `${node.x}:${node.y}`)).size).toBe(laidOut.nodes.length);
  });

  it("reports structural, dangling and Python callable problems", () => {
    const graph: PythonFlowGraph = {
      version: 1,
      nodes: [
        { id: "duplicate", kind: "python-call", title: "空调用", x: 0, y: 0, config: { callable: "" } },
        { id: "duplicate", kind: "data", title: "重复", x: 0, y: 0, config: {} },
      ],
      edges: [{ id: "dangling", source: "duplicate", target: "missing" }],
    };
    const issueIds = validatePythonFlowGraph(graph).map((issue) => issue.id);

    expect(issueIds).toContain("missing-start");
    expect(issueIds).toContain("missing-return");
    expect(issueIds).toContain("duplicate-duplicate");
    expect(issueIds).toContain("dangling-dangling");
    expect(issueIds).toContain("callable-duplicate");
  });

  it("accepts a connected entry-call-return graph", () => {
    const graph = graphFixture();
    graph.nodes = graph.nodes.filter((node) => node.id !== "branch");
    graph.edges = [
      { id: "e1", source: "start", target: "call" },
      { id: "e2", source: "call", target: "done" },
    ];

    expect(validatePythonFlowGraph(graph)).toEqual([]);
  });

  it("routes code conversion through controlled Studio callbacks", async () => {
    const refresh = vi.fn().mockResolvedValue(undefined);
    const apply = vi.fn().mockResolvedValue(undefined);
    const notice = vi.fn();
    render(<PythonFlowDesigner
      graph={graphFixture()}
      source={'def main(ctx):\n    return 1\n'}
      sourceStale
      busy={false}
      onGraphChange={vi.fn()}
      onApplyToCode={apply}
      onRefreshFromCode={refresh}
      onNotice={notice}
    />);

    fireEvent.click(screen.getByRole("button", { name: "代码同步" }));
    const sourcePreview = screen.getByRole("textbox", { name: "Python Flow 源代码" });
    expect(sourcePreview).toHaveAttribute("readonly");
    fireEvent.click(screen.getByRole("button", { name: "代码 → 流程图" }));
    await waitFor(() => expect(refresh).toHaveBeenCalledWith('def main(ctx):\n    return 1\n'));

    fireEvent.click(screen.getByRole("button", { name: "流程图 → 代码" }));
    await waitFor(() => expect(apply).toHaveBeenCalledWith(expect.objectContaining({ version: 1 })));
    expect(notice).toHaveBeenCalledTimes(2);
  });

  it("locks mutation controls while the parent is busy", () => {
    render(<PythonFlowDesigner
      graph={graphFixture()}
      source=""
      sourceStale={false}
      busy
      onGraphChange={vi.fn()}
      onApplyToCode={vi.fn()}
      onRefreshFromCode={vi.fn()}
      onNotice={vi.fn()}
    />);

    expect(screen.getByRole("button", { name: "自动布局" })).toBeDisabled();
    expect(screen.getByRole("button", { name: /流程入口/ })).toBeDisabled();
  });
});
