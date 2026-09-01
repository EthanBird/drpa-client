export type NavigationId =
  | "overview"
  | "library"
  | "studio"
  | "data"
  | "workbench"
  | "runs"
  | "automations"
  | "agent"
  | "localDify"
  | "plugins"
  | "docs"
  | "knowledgeBase"
  | "opticalTransfer"
  | "extensionTools"
  | "runtimes"
  | "secrets"
  | "settings";

export type RunStatus = "running" | "queued" | "success" | "failed" | "cancelled" | "interrupted";
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
  label?: string;
  description?: string;
  kind: "string" | "number" | "boolean" | "secret" | "file" | "directory";
  required: boolean;
  defaultValue?: string | number | boolean;
}

export interface StudioProject {
  id: string;
  name: string;
  files: string[];
}

export interface PythonFlowSourceSpan {
  startLine: number;
  startColumn: number;
  endLine: number;
  endColumn: number;
}

export interface PythonFlowPosition {
  x: number;
  y: number;
}

export interface PythonFlowNode {
  id: string;
  type: "start" | "end" | "assign" | "call" | "ctx-call" | "rpa-call" | "if" | "for" | "while" | "try" | "return" | "raw-code";
  label: string;
  code: string;
  span: PythonFlowSourceSpan;
  data: Record<string, unknown>;
  position?: PythonFlowPosition;
}

export interface PythonFlowEdge {
  id?: string;
  source: string;
  target: string;
  kind: string;
  label?: string;
}

export interface PythonFlowGraph {
  schemaVersion: 1;
  kind: "drpa.python-flow";
  source: { name: string; sha256: string };
  entrypoint: string;
  nodes: PythonFlowNode[];
  edges: PythonFlowEdge[];
  metadata: Record<string, unknown>;
}

export interface PythonFlowValidationResult {
  ok: true;
  valid: true;
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

export interface StudioCompletionResult {
  matches: string[];
  cursorStart: number;
  cursorEnd: number;
  metadata: unknown;
  status: string;
}

export interface StudioInspectResult {
  found: boolean;
  data: Record<string, string>;
  metadata: unknown;
  status: string;
}

export interface DatabaseInfo {
  name: string;
  engine: "SQLite" | string;
  path: string;
  sizeBytes: number;
}

export interface DatabaseTable {
  name: string;
  kind: "table" | "view" | string;
  rowCount?: number;
}

export interface DatabaseColumn {
  ordinal: number;
  name: string;
  dataType: string;
  notNull: boolean;
  defaultValue?: string;
  primaryKey: boolean;
}

export interface DatabaseQueryResult {
  columns: string[];
  rows: unknown[][];
  affectedRows: number;
  durationMs: number;
  truncated: boolean;
  statementType: string;
  offset: number;
  limit: number;
  hasMore: boolean;
}

export type DatabaseExportFormat = "xlsx" | "xls" | "csv" | "json" | "sql";

export interface DatabaseExportResult {
  path: string;
  format: DatabaseExportFormat;
  rowCount: number;
}

export type DashboardWidgetKind = "metric" | "line" | "bar" | "pie" | "table" | "markdown";
export type DashboardBuiltinDataset = "workspaceSummary" | "runHistory" | "runStatus" | "packages";
export type DashboardNumberFormat = "number" | "compact" | "percent" | "currency" | "hours" | "text";

export interface DashboardDocument {
  schema: number;
  activeDashboardId: string;
  dashboards: DashboardDefinition[];
}

export interface DashboardDefinition {
  id: string;
  title: string;
  description: string;
  columns: number;
  rowHeight: number;
  widgets: DashboardWidget[];
}

export interface DashboardWidget {
  id: string;
  title: string;
  kind: DashboardWidgetKind;
  layout: DashboardWidgetLayout;
  source?: DashboardDataSource;
  encoding: DashboardEncoding;
  options: DashboardWidgetOptions;
}

export interface DashboardWidgetLayout {
  x: number;
  y: number;
  w: number;
  h: number;
}

export type DashboardDataSource =
  | { kind: "builtin"; dataset: DashboardBuiltinDataset }
  | { kind: "database"; profileId: string; sql: string };

export interface DashboardEncoding {
  categoryField: string;
  valueField: string;
  seriesField: string;
}

export interface DashboardWidgetOptions {
  text: string;
  numberFormat: DashboardNumberFormat;
  color: string;
  showLegend: boolean;
  refreshSeconds: number;
}

export interface DashboardDataset {
  columns: string[];
  rows: Array<Record<string, unknown>>;
  durationMs: number;
  truncated: boolean;
}

export type VaultCredentialKind = "login" | "apiKey" | "token" | "database" | "ssh" | "secureNote";

export interface VaultServiceStatus {
  running: boolean;
  port: number;
  endpoint: string;
  startedAt?: number;
  lastError: string;
}

export interface VaultStatus {
  initialized: boolean;
  unlocked: boolean;
  unlockedUntil?: number;
  itemCount: number;
  failedAttempts: number;
  retryAfterSeconds: number;
  service: VaultServiceStatus;
}

export interface VaultSetup {
  setupId: string;
  account: string;
  issuer: string;
  manualKey: string;
  otpAuthUri: string;
  expiresAt: number;
}

export interface VaultUnlockResult {
  status: VaultStatus;
  serviceToken: string;
  recoveryCode?: string;
}

export interface VaultCredentialSummary {
  id: string;
  name: string;
  kind: VaultCredentialKind;
  username: string;
  uri: string;
  tags: string[];
  favorite: boolean;
  hasSecret: boolean;
  updatedAt: number;
}

export interface VaultCredential extends VaultCredentialSummary {
  secret: string;
  notes: string;
  createdAt: number;
}

export interface VaultCredentialInput {
  id: string;
  name: string;
  kind: VaultCredentialKind;
  username: string;
  secret: string;
  uri: string;
  notes: string;
  tags: string[];
  favorite: boolean;
}

export interface RemoteDatabaseProfile {
  id: string;
  name: string;
  engine: "postgresql" | "mysql" | "mariadb" | "sqlite" | "excel" | "csv" | "json";
  host: string;
  port: number;
  database: string;
  username: string;
  tlsMode: "disable" | "prefer" | "require";
}

export interface RemoteConnectionTest {
  serverVersion: string;
  latencyMs: number;
}

export type LocalDifyAppMode = "chat" | "completion" | "advanced-chat" | "workflow";

export interface LocalDifyProvider {
  id: string;
  name: string;
  baseUrl: string;
  model: string;
  contextWindow: number;
  maxOutputTokens: number;
  temperature: number;
  streaming: boolean;
  supportsTools: boolean;
  supportsJson: boolean;
  supportsVision: boolean;
  timeoutSeconds: number;
  customHeaders: Record<string, string>;
  difyProvider: string;
  difyModel: string;
  hasApiKey: boolean;
  updatedAt: number;
}

export interface LocalDifyProviderInput extends Omit<LocalDifyProvider, "hasApiKey" | "updatedAt"> {
  apiKey: string;
}

export type LocalDifyWorkflowNodeKind =
  | "start"
  | "llm"
  | "template-transform"
  | "if-else"
  | "http-request"
  | "code"
  | "rpaz-package"
  | "question-classifier"
  | "parameter-extractor"
  | "variable-aggregator"
  | "list-operator"
  | "document-extractor"
  | "knowledge-retrieval"
  | "answer"
  | "end"
  | string;

export interface LocalDifyWorkflowViewport {
  x: number;
  y: number;
  zoom: number;
}

export interface LocalDifyWorkflowNode {
  id: string;
  kind: LocalDifyWorkflowNodeKind;
  title: string;
  x: number;
  y: number;
  width: number;
  height: number;
  config: Record<string, unknown>;
}

export interface LocalDifyWorkflowEdge {
  id: string;
  source: string;
  target: string;
  sourceHandle: string;
  targetHandle: string;
  label: string;
  data: Record<string, unknown>;
}

export interface LocalDifyWorkflowGraph {
  schema: number;
  viewport: LocalDifyWorkflowViewport;
  nodes: LocalDifyWorkflowNode[];
  edges: LocalDifyWorkflowEdge[];
}

export interface LocalDifyWorkflowValidationIssue {
  level: "info" | "warning" | "error" | string;
  code: string;
  message: string;
  nodeId?: string;
  edgeId?: string;
}

export interface LocalDifyWorkflowValidationReport {
  valid: boolean;
  nodeCount: number;
  edgeCount: number;
  issues: LocalDifyWorkflowValidationIssue[];
}

export interface LocalDifyApp {
  schema: number;
  id: string;
  name: string;
  description: string;
  mode: LocalDifyAppMode;
  providerId: string;
  systemPrompt: string;
  openingStatement: string;
  inputKey: string;
  temperature: number;
  maxOutputTokens: number;
  workflow: LocalDifyWorkflowGraph;
  publishedVersion: number;
  apiEnabled: boolean;
  createdAt: number;
  updatedAt: number;
}

export interface LocalDifyRunRequest {
  requestId: string;
  appId: string;
  query: string;
  inputs: Record<string, unknown>;
  user: string;
  stream: boolean;
  conversationId: string;
  providerRoute?: string[];
}

export interface LocalDifyUsage {
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
}

export interface LocalDifyRunResult {
  runId: string;
  appId: string;
  answer: string;
  conversationId: string;
  providerId: string;
  model: string;
  usage: LocalDifyUsage;
  durationMs: number;
}

export type LocalDifyStreamEvent =
  | { type: "started"; runId: string }
  | { type: "nodeStarted"; runId: string; nodeId: string; nodeType: string; title: string }
  | { type: "nodeCompleted"; runId: string; nodeId: string; outputs: unknown; durationMs: number }
  | { type: "nodeFailed"; runId: string; nodeId: string; error: string; durationMs: number }
  | { type: "delta"; content: string }
  | { type: "completed"; runId: string };

export interface LocalDifyRunSummary {
  id: string;
  appId: string;
  appName: string;
  status: "success" | "failed" | string;
  query: string;
  answer: string;
  providerId: string;
  model: string;
  promptTokens: number;
  completionTokens: number;
  durationMs: number;
  error: string;
  createdAt: number;
}

export interface LocalDifyProviderTest {
  ok: boolean;
  message: string;
  model: string;
  durationMs: number;
}

export interface DifyCompatibilityIssue {
  level: "info" | "warning" | "error" | string;
  code: string;
  message: string;
}

export interface DifyCompatibilityReport {
  compatible: boolean;
  targetVersion: string;
  issues: DifyCompatibilityIssue[];
}

export interface LocalDifyServiceStatus {
  running: boolean;
  port: number;
  endpoint: string;
  startedAt?: number;
  lastError: string;
}

export interface RuntimeStatus {
  state: "ready" | "notInitialized" | "broken";
  bundleVersion: string;
  pythonVersion: string;
  runtimeRoot: string;
  environmentRoot: string;
  browserExecutable: string;
  browserName: string;
  browserFamily: "chromium" | "firefox" | "none" | string;
  browserAutomationCompatible: boolean;
  message: string;
  profileId: string;
  profileName: string;
  features: string[];
  profiles: RuntimeProfileSummary[];
}

export interface RuntimeProfileSummary {
  id: string;
  name: string;
  componentVersion: string;
  pythonVersion: string;
  environmentMode: "materialized" | "frozen" | string;
  features: string[];
  selected: boolean;
  ready: boolean;
  inUse: number;
  runtimeRoot: string;
  environmentRoot: string;
}

export interface RuntimePythonPackage {
  name: string;
  version: string;
  source: "user" | "runtime" | string;
  location: string;
  removable: boolean;
}

export interface RuntimePythonPackageCatalog {
  profileId: string;
  profileName: string;
  pythonVersion: string;
  backend: "uv" | "pip" | "unavailable" | string;
  backendVersion: string;
  backendPath: string;
  overlayRoot: string;
  packages: RuntimePythonPackage[];
}

export interface RuntimeProfileExportResult {
  path: string;
  componentId: string;
  displayName: string;
  description: string;
  packageCount: number;
  fileCount: number;
  bytes: number;
}

export interface RuntimeBrowserCandidate {
  id: string;
  name: string;
  family: "chromium" | "firefox" | string;
  source: string;
  executable: string;
  selected: boolean;
  automationCompatible: boolean;
}

export interface RuntimeBrowserConfiguration {
  mode: "auto" | "manual" | string;
  activeName: string;
  activeFamily: "chromium" | "firefox" | "none" | string;
  activeExecutable: string;
  automationCompatible: boolean;
  message: string;
  candidates: RuntimeBrowserCandidate[];
}

export interface SystemResourceMetric {
  usedBytes: number;
  totalBytes: number;
  usagePercent: number;
}

export interface SystemCpuMetric {
  usagePercent: number;
  logicalCores: number;
}

export interface SystemDiskVolume extends SystemResourceMetric {
  name: string;
  mountPoint: string;
}

export interface SystemMetricsSnapshot {
  sampledAt: number;
  cpu: SystemCpuMetric;
  memory: SystemResourceMetric;
  disk: SystemResourceMetric & {
    volumes: SystemDiskVolume[];
  };
}

export interface PlatformCapabilities {
  os: "windows" | "linux" | "macos";
  displayName: string;
  runtimeTarget: string;
  supportsWindowsUpdates: boolean;
  fileManagerName: string;
  dataDirectoryPolicy: string;
  reducedVisualEffects: boolean;
}

export interface CurrentUser {
  displayName: string;
  accountName: string;
  initials: string;
}

export interface WorkspaceInfo {
  id: string;
  name: string;
  path: string;
  active: boolean;
  createdAt: number;
}

export interface UserDataTransferResult {
  path: string;
  fileCount: number;
  totalBytes: number;
  workspaceName: string;
  restartRequired: boolean;
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

export interface AgentToolPolicy {
  enabled: boolean;
  databaseRead: boolean;
  databaseConnections: boolean;
  arbitraryFileRead: boolean;
  fileReadScope?: "project" | "system";
  knowledgeBaseRead: boolean;
  documentRead: boolean;
  documentWrite: boolean;
  documentConvert: boolean;
  projectWrite: boolean;
  python: boolean;
  workspaceWrite: boolean;
  extensions: boolean;
  browser: boolean;
  rpazRuns: boolean;
  runRecords: boolean;
  vaultRead: boolean;
  vaultWrite: boolean;
}

export interface AgentExtensionToolSummary {
  name: string;
  exposedName: string;
  label: string;
  description: string;
  parameters: Record<string, unknown>;
}

export interface AgentExtensionSummary {
  id: string;
  name: string;
  version: string;
  description: string;
  enabled: boolean;
  runtime: string;
  source: string;
  integrity: string;
  directory: string;
  tools: AgentExtensionToolSummary[];
}

export interface AgentDocumentAttachment {
  id: string;
  name: string;
  format: "pdf" | "docx" | "xlsx" | "pptx" | string;
  sizeBytes: number;
  importedAt: string;
}

export interface AgentDocumentArtifact {
  id: string;
  name: string;
  format: "pdf" | "docx" | "xlsx" | "pptx" | string;
  sizeBytes: number;
  createdAt: string;
  sourceId?: string;
}

export interface AgentDocumentExport {
  artifactId: string;
  path: string;
  sizeBytes: number;
}

export interface KnowledgeBaseSummary {
  id: string;
  name: string;
  description: string;
  sourceCount: number;
  chunkCount: number;
  status: string;
  updatedAt: number;
}

export interface KnowledgeBaseSource {
  id: string;
  knowledgeBaseId: string;
  name: string;
  kind: string;
  status: string;
  chunkCount: number;
  sizeBytes: number;
  uri: string;
  lastError: string;
  updatedAt: number;
}

export interface KnowledgeBaseSearchResult {
  knowledgeBaseId: string;
  knowledgeBaseName: string;
  sourceId: string;
  sourceName: string;
  chunkId: string;
  content: string;
  citation: string;
  score: number;
  vectorScore: number;
  keywordScore: number;
}

export type AutomationScheduleKind = "cron" | "daily" | "weekly" | "interval";
export type AutomationConcurrencyPolicy = "skip" | "queue" | "parallel";
export type AutomationRunStatus = "queued" | "running" | "succeeded" | "failed" | "skipped";

export interface AutomationSchedule {
  type: AutomationScheduleKind;
  cron: string;
  time: string;
  daysOfWeek: number[];
  intervalMinutes: number;
}

export interface AutomationAction {
  type: "rpaz-package" | string;
  packageId: string;
  entrypoint: string;
  parameters: Record<string, unknown>;
}

export interface AutomationRetryPolicy {
  maxAttempts: number;
  delaySeconds: number;
  backoffMultiplier: number;
}

export interface AutomationDeliveryTarget {
  id: string;
  type: string;
  name: string;
  enabled: boolean;
  configuration: Record<string, unknown>;
}

export interface AutomationPlanInput {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  schedule: AutomationSchedule;
  action: AutomationAction;
  concurrencyPolicy: AutomationConcurrencyPolicy;
  retryPolicy: AutomationRetryPolicy;
  timeoutSeconds: number;
  deliveryTargets: AutomationDeliveryTarget[];
}

export interface AutomationPlan extends AutomationPlanInput {
  createdAt: number;
  updatedAt: number;
  lastRunAt?: number;
  nextRunAt?: number;
  lastScheduledMinute?: number;
}

export interface AutomationDeliveryResult {
  targetId: string;
  targetName: string;
  status: string;
  error: string;
}

export interface AutomationRun {
  id: string;
  planId: string;
  planName: string;
  trigger: "manual" | "scheduled";
  status: AutomationRunStatus;
  queuedAt: number;
  startedAt?: number;
  finishedAt?: number;
  attempt: number;
  result?: unknown;
  error: string;
  deliveryResults: AutomationDeliveryResult[];
}

export interface AgentProviderRef {
  pluginId: string;
  providerId: string;
}

export type AgentMode = "rpaz" | "developer";

export interface AgentContextCheckpoint {
  checkpointId: string;
  summary: string;
  coversMessages: number;
  sourceDigest: string;
  createdAt: number;
  estimatedTokens: number;
  method: "model" | "deterministic-fallback" | string;
}

export interface AgentTurnRequest {
  requestId: string;
  sessionId?: string;
  baseUrl: string;
  model: string;
  mode?: AgentMode | "sql";
  databaseDialect?: "sqlite" | "postgresql" | "mysql";
  apiKey: string;
  providerRef?: AgentProviderRef | null;
  projectId: string;
  stream: boolean;
  contextWindow: number;
  maxOutputTokens: number;
  maxRounds: number;
  temperature: number;
  pythonTimeoutSeconds: number;
  maxToolCalls?: number;
  maxWallTimeSeconds?: number;
  selectedSkillIds: string[];
  toolPolicy: AgentToolPolicy;
  contextCheckpoint?: AgentContextCheckpoint | null;
  messages: AgentMessage[];
}

export interface AgentToolEvent {
  callId: string;
  name: string;
  status: "running" | "completed" | "failed";
  summary: string;
  output: string;
  /** Stable action order within one run. Older persisted sessions may omit it. */
  ordinal?: number;
  /** Model step/round that produced this action. */
  round?: number;
  /** Redacted, bounded action input for the run inspector. */
  input?: string;
  durationMs?: number;
}

export interface AgentRunRoundProjection {
  round: number;
  status: "running" | "completed";
  estimatedTokens?: number;
  omittedMessages?: number;
  omittedTools?: number;
}

export interface AgentRunRetry {
  round: number;
  attempt: number;
  maxAttempts: number;
  delayMs: number;
  error: string;
}

export type AgentStreamEvent =
  | { type: "started"; runId: string; sessionId: string }
  | { type: "roundStarted"; round: number }
  | { type: "contextAssembled"; round: number; estimatedTokens: number; omittedMessages: number; omittedTools: number }
  | { type: "retrying"; round: number; attempt: number; maxAttempts: number; delayMs: number; error: string }
  | { type: "contextCompacted"; round: number; checkpoint: AgentContextCheckpoint }
  | { type: "delta"; content: string }
  | { type: "contentReplace"; content: string }
  | { type: "tool"; tool: AgentToolEvent }
  | { type: "completed"; runId: string; usage: AgentTurnResult["usage"]; durationMs: number; stopReason: string }
  | { type: "failed"; runId: string; error: string }
  | { type: "cancelled"; runId: string };

export interface AgentConversationMessage extends AgentMessage {
  id: string;
  tools?: AgentToolEvent[];
  durationMs?: number;
  tokens?: number;
  run?: {
    requestId: string;
    stopReason: string;
    rounds: number;
    toolCalls: number;
    status?: "completed" | "failed" | "cancelled";
    error?: string;
    retryCount?: number;
    retries?: AgentRunRetry[];
    roundDetails?: AgentRunRoundProjection[];
    contextCheckpoint?: AgentContextCheckpoint;
  };
}

export interface AgentConversationSession {
  id: string;
  title: string;
  projectId: string;
  createdAt: number;
  updatedAt: number;
  revision?: number;
  messages: AgentConversationMessage[];
  selectedSkillIds: string[];
  contextFiles?: string[];
  messageCount?: number;
  bodyState?: "summary" | "loading" | "ready" | "failed";
}

export interface AgentConversationSessionSummary {
  id: string;
  title: string;
  projectId: string | null;
  createdAt: number;
  updatedAt: number;
  revision?: number;
  messageCount: number;
  selectedSkillIds: string[];
}

export interface AgentConversationProject {
  id: string;
  name: string;
  path: string;
  createdAt: number;
  updatedAt: number;
  sessionCount: number;
  source?: "managed" | "external";
}

export interface AgentTurnResult {
  message: string;
  tools: AgentToolEvent[];
  usage: {
    promptTokens: number;
    completionTokens: number;
  };
  durationMs: number;
  stopReason: string;
  rounds: number;
  toolCalls: number;
  retryCount: number;
  contextCheckpoint?: AgentContextCheckpoint;
}

export type AgentRunStatus = "queued" | "running" | "cancelling" | "completed" | "failed" | "cancelled";

export interface AgentRunSnapshot {
  requestId: string;
  sessionId: string;
  status: AgentRunStatus;
  createdAt: number;
  startedAt?: number;
  finishedAt?: number;
  stopReason: string;
  error: string;
  usage: AgentTurnResult["usage"];
  durationMs: number;
  events: Array<{ sequence: number; at: number; event: AgentStreamEvent }>;
}

export interface AgentSkillSummary {
  name: string;
  displayName?: string;
  version?: string;
  description: string;
  format?: "skill-v2" | "legacy-markdown";
  toolCount?: number;
  libraryCount?: number;
  modifiedAt: number;
}

export interface AgentSkillPackage {
  name: string;
  manifestYaml: string;
  instructionsMarkdown: string;
  files: string[];
  entries: AgentSkillEntry[];
}

export interface AgentSkillEntry {
  path: string;
  kind: "file" | "directory";
}

export interface AgentWorkspaceConfig {
  agentsMarkdown: string;
  memoryMarkdown: string;
  skills: AgentSkillSummary[];
  rootDirectory: string;
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
  packageId?: string;
  packageName: string;
  packageVersion?: string;
  profileId?: string;
  profileName: string;
  status: RunStatus;
  startedAt: string;
  finishedAt?: string;
  duration: string;
  durationMs?: number;
  progress?: number;
  exitCode?: number;
}

export type PluginJsonPrimitive = string | number | boolean | null;
export type PluginJsonValue =
  | PluginJsonPrimitive
  | PluginJsonValue[]
  | { [key: string]: PluginJsonValue };

export interface PluginServiceSummary {
  id: string;
  title: string;
  primary: boolean;
  transport: string;
  status: "disabled" | "stopped" | "running" | "error";
  endpoint: string;
  healthcheck: string;
}

export interface PluginProviderSummary {
  id: string;
  title: string;
  protocol: string;
  serviceId: string;
  endpoint: string;
  modelConfigKey: string;
  apiKeyConfigKey: string;
}

export interface PluginDebuggerEndpoint {
  id: string;
  title: string;
  kind: string;
  method: "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
  endpoint: string;
  bearerConfigKey: string;
  requestDefaults: PluginJsonValue;
  timeoutSeconds: number;
}

export interface PluginDebuggerPanel {
  id: string;
  title: string;
  kind: string;
  endpoint: string;
  config: PluginJsonValue;
}

export interface PluginDebuggerManifest {
  endpoints: PluginDebuggerEndpoint[];
  panels: PluginDebuggerPanel[];
}

export interface PluginSummary {
  id: string;
  name: string;
  version: string;
  description: string;
  types: string[];
  enabled: boolean;
  autostart: boolean;
  status: "disabled" | "stopped" | "running" | "error";
  endpoint: string;
  toolCount: number;
  serviceCount: number;
  toolProviderCount: number;
  services: PluginServiceSummary[];
  providers: PluginProviderSummary[];
  debugger: PluginDebuggerManifest;
  config: Record<string, unknown>;
  configuredSecrets: Record<string, boolean>;
  configSchema: {
    type?: string;
    properties?: Record<string, {
      type?: "string" | "integer" | "number" | "boolean";
      title?: string;
      description?: string;
      enum?: string[];
      secret?: boolean;
      minimum?: number;
      maximum?: number;
      minLength?: number;
      maxLength?: number;
      pattern?: string;
    }>;
    required?: string[];
  };
  directory: string;
  lastError: string;
}

export interface PluginLogLine {
  timestamp: number;
  stream: "stdout" | "stderr";
  message: string;
  serviceId?: string;
  event?: PluginJsonValue;
}

export interface PluginDebuggerResponse {
  endpointId: string;
  status: number;
  durationMs: number;
  contentType: string;
  body: PluginJsonValue;
  truncated: boolean;
}

export interface PluginToolDescriptor {
  name: string;
  title: string;
  description: string;
  inputSchema: PluginJsonValue;
}

export interface PluginToolWorkbenchResult {
  ok: boolean;
  output: PluginJsonValue;
  durationMs: number;
}

export interface PluginProjectSummary {
  id: string;
  name: string;
  version: string;
  description: string;
  types: string[];
  directory: string;
  valid: boolean;
  validationMessage: string;
}

export interface PluginConnectionTest {
  ok: boolean;
  message: string;
  duration_ms: number;
  details: Record<string, unknown>;
}

export interface RunEventRecord {
  id: number;
  runId: string;
  sequence?: number;
  recordedAt: string;
  eventType: string;
  level?: LogEntry["level"];
  scope?: string;
  message: string;
  payload: unknown;
}

export interface RunArtifactRecord {
  id: number;
  runId: string;
  sequence?: number;
  label: string;
  path: string;
  mediaType?: string;
  size?: number;
  createdAt: string;
}

export interface RunDetail {
  summary: RunSummary;
  parameters: Record<string, unknown>;
  outputDir: string;
  errorMessage?: string;
  errorTraceback?: string;
  events: RunEventRecord[];
  artifacts: RunArtifactRecord[];
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
