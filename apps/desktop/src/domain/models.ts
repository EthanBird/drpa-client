export type NavigationId =
  | "overview"
  | "library"
  | "studio"
  | "workbench"
  | "runs"
  | "automations"
  | "agent"
  | "docs"
  | "runtimes"
  | "secrets"
  | "settings";

export type RunStatus = "running" | "queued" | "success" | "failed" | "cancelled";
export type TrustLevel = "verified" | "local" | "untrusted";
export type AutomationStatus = "enabled" | "paused" | "needsAttention";

export interface PackageSummary {
  id: string;
  name: string;
  description: string;
  version: string;
  runtime: string;
  trust: TrustLevel;
  accent: string;
  initials: string;
  parameters: ParameterSummary[];
  profiles: TaskProfile[];
}

export interface ParameterSummary {
  id: string;
  kind: "string" | "number" | "boolean" | "secret" | "file" | "directory";
  required: boolean;
  defaultValue?: string | number | boolean;
}

export interface StudioProject {
  id: string;
  name: string;
  files: string[];
}

export interface StudioVariable {
  name: string;
  typeName: string;
  preview: string;
}

export interface StudioCellResult {
  executionCount: number;
  stdout: string;
  stderr: string;
  result?: string;
  error?: string;
  traceback: string[];
  outputs: Array<Record<string, unknown>>;
  variables: StudioVariable[];
  durationMs: number;
}

export interface RuntimeStatus {
  state: "ready" | "notInitialized" | "broken";
  bundleVersion: string;
  pythonVersion: string;
  runtimeRoot: string;
  environmentRoot: string;
  browserExecutable: string;
  message: string;
}

export interface CurrentUser {
  displayName: string;
  accountName: string;
  initials: string;
}

export interface KnowledgeEntry {
  path: string;
  name: string;
  kind: "file" | "directory";
  size: number;
  modifiedAt: number;
}

export interface AgentMessage {
  role: "user" | "assistant";
  content: string;
}

export interface AgentTurnRequest {
  baseUrl: string;
  model: string;
  apiKey: string;
  projectId: string;
  messages: AgentMessage[];
}

export interface AgentToolEvent {
  callId: string;
  name: string;
  status: "completed" | "failed";
  summary: string;
  output: string;
}

export interface AgentConversationMessage extends AgentMessage {
  id: string;
  tools?: AgentToolEvent[];
  durationMs?: number;
  tokens?: number;
}

export interface AgentConversationSession {
  id: string;
  title: string;
  projectId: string;
  createdAt: number;
  updatedAt: number;
  messages: AgentConversationMessage[];
}

export interface AgentTurnResult {
  message: string;
  tools: AgentToolEvent[];
  usage: {
    promptTokens: number;
    completionTokens: number;
  };
  durationMs: number;
}

export type WindowsUpdatePhase =
  | "verifying"
  | "applying"
  | "waitingForRestart"
  | "restarting"
  | "completed"
  | "failed";

export interface WindowsUpdateSession {
  id: string;
  version: string;
  totalFiles: number;
  totalBytes: number;
}

export interface WindowsUpdateStatus {
  sessionId: string;
  version: string;
  phase: WindowsUpdatePhase;
  progress: number;
  completedFiles: number;
  totalFiles: number;
  currentFile?: string;
  message: string;
}

export interface TaskProfile {
  id: string;
  name: string;
  schedule?: string;
  lastRun?: string;
  runtimeProfileId?: string;
}

export interface RunSummary {
  id: string;
  packageName: string;
  profileName: string;
  status: RunStatus;
  startedAt: string;
  duration: string;
  progress?: number;
}

export interface AutomationSummary {
  id: string;
  name: string;
  packageName: string;
  profileName: string;
  triggerLabel: string;
  nextRun: string;
  lastRun?: string;
  health: string;
  status: AutomationStatus;
  concurrencyPolicy: "allow" | "forbid" | "replace" | "queueOne";
  retryPolicy: string;
}

export interface LogEntry {
  id: number;
  time: string;
  level: "trace" | "info" | "success" | "warning" | "error";
  scope: string;
  message: string;
}

export interface WorkspaceSnapshot {
  packages: PackageSummary[];
  runs: RunSummary[];
  automations: AutomationSummary[];
  logs: LogEntry[];
  stats: {
    activeRuns: number;
    successRate: number;
    packages: number;
    savedHours: number;
  };
}
