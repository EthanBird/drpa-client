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
});
