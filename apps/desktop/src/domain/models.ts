export type NavigationId =
  | "overview"
  | "library"
  | "studio"
  | "workbench"
  | "runs"
  | "automations"
  | "runtimes"
  | "secrets"
  | "settings";

export type RunStatus = "running" | "queued" | "success" | "failed" | "cancelled";
export type TrustLevel = "verified" | "local" | "untrusted";

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
  variables: StudioVariable[];
  durationMs: number;
}

export interface TaskProfile {
  id: string;
  name: string;
  schedule?: string;
  lastRun?: string;
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
  logs: LogEntry[];
  stats: {
    activeRuns: number;
    successRate: number;
    packages: number;
    savedHours: number;
  };
}
