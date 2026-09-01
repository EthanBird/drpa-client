import { describe, expect, it } from "vitest";

import type { PythonFlowGraph } from "../domain/models";
import { canvasFlowToCore, coreFlowToCanvas } from "./pythonFlowAdapter";

function coreFixture(): PythonFlowGraph {
  return {
    schemaVersion: 1,
    kind: "drpa.python-flow",
    source: { name: "main.py", sha256: "source-hash" },
    entrypoint: "main",
    metadata: {
      body: ["call"],
      moduleBefore: "import rpa as r\n\n",
      moduleAfter: "\n",
      functionHeader: "def main(ctx):",
      indent: "    ",
    },
    nodes: [
      { id: "start", type: "start", label: "Start", code: "", span: { startLine: 3, startColumn: 0, endLine: 3, endColumn: 0 }, data: {} },
      { id: "call", type: "rpa-call", label: "r.click", code: "r.click(\"login\")", span: { startLine: 4, startColumn: 4, endLine: 4, endColumn: 20 }, data: { statement: "r.click(\"login\")", callName: "r.click", arguments: ["\"login\""], keywords: [], assignTargets: [] } },
      { id: "end", type: "end", label: "End", code: "", span: { startLine: 4, startColumn: 20, endLine: 4, endColumn: 20 }, data: {} },
    ],
    edges: [
      { id: "first", source: "start", target: "call", kind: "next" },
      { id: "last", source: "call", target: "end", kind: "next" },
    ],
  };
}

describe("Python Flow canvas adapter", () => {
  it("keeps the AST projection as embedded semantic state", () => {
    const core = coreFixture();
    const canvas = coreFlowToCanvas(core);
    const restored = canvasFlowToCore(canvas);

    expect(restored.kind).toBe("drpa.python-flow");
    expect(restored.metadata.body).toEqual(["call"]);
    expect(restored.nodes.find((node) => node.id === "call")?.type).toBe("rpa-call");
    expect(restored.nodes.find((node) => node.id === "call")?.code).toBe("r.click(\"login\")");
    expect(restored.edges.map((edge) => edge.kind)).toEqual(["next", "next"]);
  });

  it("preserves unedited calls, keyword arguments, aliases, and return expressions exactly", () => {
    const core = coreFixture();
    core.metadata.body = ["call", "ctx", "return"];
    core.nodes = [
      core.nodes[0],
      {
        ...core.nodes[1],
        code: "robot.init(visual_automation=True, chrome_browser=True)",
        data: {
          statement: "robot.init(visual_automation=True, chrome_browser=True)",
          callName: "robot.init",
          arguments: [],
          keywords: [{ name: "visual_automation", value: "True" }, { name: "chrome_browser", value: "True" }],
          assignTargets: [],
        },
      },
      {
        id: "ctx",
        type: "ctx-call",
        label: "ctx.browser",
        code: "page = ctx.browser(address='127.0.0.1:9222')",
        span: { startLine: 5, startColumn: 4, endLine: 5, endColumn: 52 },
        data: {
          statement: "page = ctx.browser(address='127.0.0.1:9222')",
          callName: "ctx.browser",
          arguments: [],
          keywords: [{ name: "address", value: "'127.0.0.1:9222'" }],
          assignTargets: ["page"],
        },
      },
      {
        id: "return",
        type: "return",
        label: "Return",
        code: "return {'ok': True}",
        span: { startLine: 6, startColumn: 4, endLine: 6, endColumn: 23 },
        data: { statement: "return {'ok': True}", value: "{'ok': True}" },
      },
      core.nodes[2],
    ];
    core.edges = [
      { id: "a", source: "start", target: "call", kind: "next" },
      { id: "b", source: "call", target: "ctx", kind: "next" },
      { id: "c", source: "ctx", target: "return", kind: "next" },
      { id: "d", source: "return", target: "end", kind: "return" },
    ];

    const restored = canvasFlowToCore(coreFlowToCanvas(core));

    expect(restored.nodes.find((node) => node.id === "call")?.code).toBe("robot.init(visual_automation=True, chrome_browser=True)");
    expect(restored.nodes.find((node) => node.id === "ctx")?.code).toBe("page = ctx.browser(address='127.0.0.1:9222')");
    expect(restored.nodes.find((node) => node.id === "return")?.code).toBe("return {'ok': True}");
  });

  it("compiles an edited RPA action back to ordinary Python", () => {
    const canvas = coreFlowToCanvas(coreFixture());
    const action = canvas.nodes.find((node) => node.id === "call")!;
    action.config = { ...action.config, action: "type", target: "\"//input\"", arguments: ["\"login\"", "\"hello\""] };

    const restored = canvasFlowToCore(canvas);
    const node = restored.nodes.find((item) => item.id === "call")!;

    expect(node.data.callName).toBe("r.type");
    expect(node.code).toBe("r.type(\"//input\", \"hello\")");
  });

  it("reconstructs if regions from branch edges without swallowing the join", () => {
    const core = coreFixture();
    core.metadata.body = ["if", "after"];
    core.nodes = [
      core.nodes[0],
      { id: "if", type: "if", label: "If enabled", code: "if enabled:\n    pass", span: { startLine: 4, startColumn: 4, endLine: 5, endColumn: 8 }, data: { test: "enabled", body: ["call"], orelse: [] } },
      core.nodes[1],
      { id: "after", type: "ctx-call", label: "ctx.progress", code: "ctx.progress(100)", span: { startLine: 6, startColumn: 4, endLine: 6, endColumn: 21 }, data: { statement: "ctx.progress(100)", callName: "ctx.progress", arguments: ["100"], keywords: [], assignTargets: [] } },
      core.nodes[2],
    ];
    core.edges = [
      { id: "s", source: "start", target: "if", kind: "next" },
      { id: "t", source: "if", target: "call", kind: "true" },
      { id: "tc", source: "call", target: "after", kind: "next" },
      { id: "f", source: "if", target: "after", kind: "false" },
      { id: "e", source: "after", target: "end", kind: "next" },
    ];

    const restored = canvasFlowToCore(coreFlowToCanvas(core));
    const condition = restored.nodes.find((node) => node.id === "if")!;

    expect(condition.data.body).toEqual(["call"]);
    expect(condition.data.orelse).toEqual([]);
    expect(restored.metadata.body).toEqual(["if", "after"]);
  });
});
