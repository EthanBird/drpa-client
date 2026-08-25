import { afterEach, describe, expect, it, vi } from "vitest";

import type { AgentStreamEvent, AgentTurnRequest, AgentTurnResult } from "../../domain/models";
import { desktopGateway } from "../../infra/gateway";
import { AgentWorkspaceRuntime } from "./AgentWorkspaceRuntime";

function request(requestId: string): AgentTurnRequest {
  return {
    requestId,
    sessionId: "shared-session",
    baseUrl: "http://127.0.0.1:8000/v1",
    model: "model",
    apiKey: "",
    projectId: "",
    stream: true,
    contextWindow: 32_000,
    maxOutputTokens: 4_096,
    maxRounds: 16,
    temperature: 0.2,
    pythonTimeoutSeconds: 300,
    selectedSkillIds: [],
    toolPolicy: {
      enabled: true,
      databaseRead: true,
      databaseConnections: true,
      arbitraryFileRead: true,
      knowledgeBaseRead: true,
      documentRead: true,
      documentWrite: true,
      documentConvert: true,
      projectWrite: true,
      python: true,
      workspaceWrite: true,
      extensions: true,
      browser: true,
      rpazRuns: true,
      runRecords: true,
      vaultRead: true,
      vaultWrite: true,
    },
    messages: [{ role: "user", content: "hello" }],
  };
}

describe("AgentWorkspaceRuntime run projection", () => {
  afterEach(() => vi.restoreAllMocks());

  it("owns one run per session and publishes the same stream projection to every surface", async () => {
    const runtime = new AgentWorkspaceRuntime("test-workspace");
    let listener: ((event: AgentStreamEvent) => void) | undefined;
    vi.spyOn(desktopGateway, "listenAgentStream").mockImplementation(async (_requestId, next) => {
      listener = next;
      return () => undefined;
    });
    let resolveRun!: (result: AgentTurnResult) => void;
    vi.spyOn(desktopGateway, "runAgentTurn").mockImplementation(() => (
      new Promise((resolve) => { resolveRun = resolve; })
    ));
    const observed: string[] = [];
    const unsubscribe = runtime.subscribeRun("shared-session", () => {
      observed.push(runtime.getRunProjection("shared-session").content);
    });

    const first = runtime.runTurn(request("req-first"));
    await expect(runtime.runTurn(request("req-second"))).rejects.toThrow("已有 Agent Run");
    listener?.({ type: "delta", content: "streamed" });
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    expect(runtime.getRunProjection("shared-session").content).toBe("streamed");

    resolveRun({
      message: "done",
      tools: [],
      usage: { promptTokens: 2, completionTokens: 1 },
      durationMs: 10,
      stopReason: "completed",
      rounds: 1,
      toolCalls: 0,
      retryCount: 0,
    });
    await expect(first).resolves.toEqual(expect.objectContaining({ message: "done" }));
    expect(runtime.getRunProjection("shared-session")).toEqual(expect.objectContaining({
      status: "completed",
      content: "done",
    }));
    expect(observed).toContain("streamed");
    unsubscribe();
    runtime.dispose();
  });

  it("keeps action order stable while running events are replaced by observations", async () => {
    const runtime = new AgentWorkspaceRuntime("ordered-workspace");
    let listener: ((event: AgentStreamEvent) => void) | undefined;
    vi.spyOn(desktopGateway, "listenAgentStream").mockImplementation(async (_requestId, next) => {
      listener = next;
      return () => undefined;
    });
    let resolveRun!: (result: AgentTurnResult) => void;
    vi.spyOn(desktopGateway, "runAgentTurn").mockImplementation(() => (
      new Promise((resolve) => { resolveRun = resolve; })
    ));

    const pending = runtime.runTurn(request("req-ordered"));
    await vi.waitFor(() => expect(resolveRun).toBeTypeOf("function"));
    listener?.({ type: "roundStarted", round: 1 });
    listener?.({ type: "contextAssembled", round: 1, estimatedTokens: 1200, omittedMessages: 0, omittedTools: 0 });
    listener?.({ type: "tool", tool: { callId: "first", name: "read_file", status: "running", summary: "读取中", output: "", ordinal: 1, round: 1 } });
    listener?.({ type: "tool", tool: { callId: "second", name: "search_text", status: "running", summary: "检索中", output: "", ordinal: 2, round: 1 } });
    listener?.({ type: "tool", tool: { callId: "first", name: "read_file", status: "completed", summary: "完成", output: "a", ordinal: 1, round: 1, durationMs: 5 } });

    const projection = runtime.getRunProjection("shared-session");
    expect(projection.tools.map((tool) => tool.callId)).toEqual(["first", "second"]);
    expect(projection.tools[0]).toEqual(expect.objectContaining({ status: "completed", durationMs: 5 }));
    expect(projection.rounds).toEqual([expect.objectContaining({ round: 1, estimatedTokens: 1200 })]);

    resolveRun({
      message: "done",
      tools: projection.tools,
      usage: { promptTokens: 2, completionTokens: 1 },
      durationMs: 10,
      stopReason: "completed",
      rounds: 1,
      toolCalls: 2,
      retryCount: 0,
    });
    await pending;
    runtime.dispose();
  });

  it("keeps partial output, ordered actions, retries, and checkpoints after a failed run", async () => {
    const runtime = new AgentWorkspaceRuntime("failed-workspace");
    let listener: ((event: AgentStreamEvent) => void) | undefined;
    vi.spyOn(desktopGateway, "listenAgentStream").mockImplementation(async (_requestId, next) => {
      listener = next;
      return () => undefined;
    });
    vi.spyOn(desktopGateway, "runAgentTurn").mockImplementation(async () => {
      listener?.({ type: "delta", content: "已经完成一部分" });
      listener?.({
        type: "tool",
        tool: { callId: "read-1", name: "read_file", status: "completed", summary: "读取完成", output: "ok", ordinal: 1, round: 1 },
      });
      listener?.({ type: "retrying", round: 2, attempt: 2, maxAttempts: 4, delayMs: 1_000, error: "HTTP 503" });
      listener?.({
        type: "contextCompacted",
        round: 2,
        checkpoint: {
          checkpointId: "checkpoint-1",
          summary: "已完成文件读取。",
          coversMessages: 6,
          sourceDigest: "abc123",
          createdAt: 1,
          estimatedTokens: 12,
          method: "model",
        },
      });
      throw new Error("HTTP 503");
    });

    await expect(runtime.runTurn(request("req-failed"))).rejects.toThrow("HTTP 503");
    expect(runtime.getRunProjection("shared-session")).toEqual(expect.objectContaining({
      status: "failed",
      content: "已经完成一部分",
      error: "Error: HTTP 503",
      tools: [expect.objectContaining({ callId: "read-1", status: "completed" })],
      retries: [expect.objectContaining({ attempt: 2, maxAttempts: 4 })],
      contextCheckpoint: expect.objectContaining({ checkpointId: "checkpoint-1" }),
    }));
    runtime.dispose();
  });

  it("transitions to failed if the stream listener cannot be installed", async () => {
    const runtime = new AgentWorkspaceRuntime("listener-failure-workspace");
    vi.spyOn(desktopGateway, "listenAgentStream").mockRejectedValue(new Error("listener unavailable"));
    const runSpy = vi.spyOn(desktopGateway, "runAgentTurn");

    await expect(runtime.runTurn(request("req-listener-failed"))).rejects.toThrow("listener unavailable");
    expect(runSpy).not.toHaveBeenCalled();
    expect(runtime.getRunProjection("shared-session")).toEqual(expect.objectContaining({
      status: "failed",
      error: "Error: listener unavailable",
    }));
    runtime.dispose();
  });
});
