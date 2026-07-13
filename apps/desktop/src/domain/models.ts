export type NavigationId =
  | "overview"
  | "library"
  | "workbench"
  | "runs"
  | "automations"
  | "runtimes"
  | "secrets";

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
  profiles: TaskProfile[];
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
