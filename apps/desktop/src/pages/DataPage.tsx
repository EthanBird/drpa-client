import Editor, { loader, type Monaco } from "@monaco-editor/react";
import * as monaco from "monaco-editor/esm/vs/editor/editor.api.js";
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker.js?worker";
import "monaco-editor/esm/vs/basic-languages/sql/sql.contribution.js";
import {
  Braces,
  Check,
  ChevronLeft,
  ChevronRight,
  Clock3,
  Copy,
  ClipboardCopy,
  Cable,
  Database,
  Download,
  Eye,
  FileCode2,
  FolderOpen,
  Hash,
  KeyRound,
  LoaderCircle,
  Play,
  Plus,
  RefreshCw,
  Rows3,
  Search,
  Sparkles,
  Settings2,
  Table2,
  Trash2,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { useAppStore } from "../app/store";
import { SidebarToggle, useSidebarCollapsed } from "../components/SidebarToggle";
import type { DatabaseColumn, DatabaseExportFormat, DatabaseInfo, DatabaseQueryResult, DatabaseTable, RemoteConnectionTest, RemoteDatabaseProfile } from "../domain/models";
import { desktopGateway } from "../infra/gateway";

self.MonacoEnvironment = { getWorker: () => new EditorWorker() };
loader.config({ monaco });

const DEFAULT_SQL = `-- DRPA 工作区 SQLite
-- 按 Ctrl+Enter 执行选中内容；未选择时执行全部 SQL。
SELECT
  datetime('now', 'localtime') AS local_time,
  sqlite_version() AS sqlite_version;
`;

interface QueryHistoryEntry {
  id: string;
  sql: string;
  statementType: string;
  executedAt: number;
  durationMs: number;
  rowCount: number;
}

export function DataPage() {
  const theme = useAppStore((state) => state.theme);
  const schemaCollapsed = useSidebarCollapsed("data-schema");
  const inspectorCollapsed = useSidebarCollapsed("data-inspector");
  const [database, setDatabase] = useState<DatabaseInfo | null>(null);
  const [tables, setTables] = useState<DatabaseTable[]>([]);
  const [tableFilter, setTableFilter] = useState("");
  const [selectedTable, setSelectedTable] = useState("");
  const [columns, setColumns] = useState<DatabaseColumn[]>([]);
  const [sql, setSql] = useState(DEFAULT_SQL);
  const [result, setResult] = useState<DatabaseQueryResult | null>(null);
  const [lastStatement, setLastStatement] = useState("");
  const [pageSize, setPageSize] = useState(loadQueryPageSize);
  const [resultFilter, setResultFilter] = useState("");
  const [resultSort, setResultSort] = useState<ResultSort>(null);
  const [exportFormat, setExportFormat] = useState<DatabaseExportFormat>("xlsx");
  const [exportScope, setExportScope] = useState<"page" | "all">("all");
  const [exporting, setExporting] = useState(false);
  const [exportStatus, setExportStatus] = useState("");
  const [tableMenu, setTableMenu] = useState<TableContextMenu | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const [executing, setExecuting] = useState(false);
  const [copied, setCopied] = useState(false);
  const [history, setHistory] = useState<QueryHistoryEntry[]>(loadQueryHistory);
  const [assistantOpen, setAssistantOpen] = useState(false);
  const [assistantPrompt, setAssistantPrompt] = useState("");
  const [assistantResponse, setAssistantResponse] = useState("");
  const [assistantError, setAssistantError] = useState("");
  const [assistantBusy, setAssistantBusy] = useState(false);
  const [schemaContext, setSchemaContext] = useState("");
  const [remoteProfiles, setRemoteProfiles] = useState<RemoteDatabaseProfile[]>([]);
  const [activeConnectionId, setActiveConnectionId] = useState("local");
  const [connectionPasswords, setConnectionPasswords] = useState<Record<string, string>>({});
  const [connectionDraft, setConnectionDraft] = useState<RemoteDatabaseProfile | null>(null);
  const [connectionPassword, setConnectionPassword] = useState("");
  const [connectionBusy, setConnectionBusy] = useState(false);
  const [connectionError, setConnectionError] = useState("");
  const [connectionTest, setConnectionTest] = useState<RemoteConnectionTest | null>(null);
  const editorRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const executingRef = useRef(false);
  const runSqlRef = useRef<(editor?: monaco.editor.IStandaloneCodeEditor | null, page?: number, requestedPageSize?: number) => Promise<void>>(async () => {});
  const activeProfile = useMemo(
    () => remoteProfiles.find((profile) => profile.id === activeConnectionId) ?? null,
    [activeConnectionId, remoteProfiles],
  );
  const visibleTables = useMemo(() => {
    const filter = tableFilter.trim().toLocaleLowerCase();
    return filter ? tables.filter((table) => table.name.toLocaleLowerCase().includes(filter)) : tables;
  }, [tableFilter, tables]);

  const refreshSchema = useCallback(async () => {
    if (activeProfile) {
      const password = connectionPasswords[activeProfile.id] ?? "";
      const nextTables = await desktopGateway.listRemoteDatabaseTables(activeProfile.id, password);
      setDatabase({
        name: activeProfile.name,
        engine: databaseEngineLabel(activeProfile.engine),
        path: isFileDatabase(activeProfile.engine)
          ? activeProfile.database
          : `${activeProfile.host}:${activeProfile.port}/${activeProfile.database}`,
        sizeBytes: 0,
      });
      setTables(nextTables);
    } else {
      const [info, nextTables] = await Promise.all([
        desktopGateway.getWorkspaceDatabaseInfo(),
        desktopGateway.listDatabaseTables(),
      ]);
      setDatabase(info);
      setTables(nextTables);
    }
    setSelectedTable("");
    setColumns([]);
    setSchemaContext("");
  }, [activeProfile, connectionPasswords]);

  useEffect(() => {
    let active = true;
    setLoading(true);
    void refreshSchema()
      .catch((reason: unknown) => { if (active) setError(String(reason)); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [refreshSchema]);

  useEffect(() => {
    let active = true;
    void desktopGateway.listRemoteDatabaseProfiles()
      .then((profiles) => { if (active) setRemoteProfiles(profiles); })
      .catch((reason: unknown) => { if (active) setError(`读取数据库连接失败：${String(reason)}`); });
    return () => { active = false; };
  }, []);

  const connectDroppedFiles = useCallback(async (paths: string[]) => {
    const sources = paths
      .map((path) => ({ path, engine: fileDatabaseEngine(path) }))
      .filter((source): source is { path: string; engine: "sqlite" | "excel" } => source.engine !== null);
    if (sources.length === 0) {
      setError("请拖入 SQLite（.db/.sqlite/.sqlite3）或 Excel（.xls/.xlsx/.xlsb/.ods）文件");
      return;
    }
    setConnectionBusy(true);
    setError("");
    try {
      const savedProfiles: RemoteDatabaseProfile[] = [];
      for (const source of sources) {
        const profile = createRemoteProfile(source.engine, source.path);
        await desktopGateway.testRemoteDatabaseConnection(profile, "");
        savedProfiles.push(await desktopGateway.saveRemoteDatabaseProfile(profile));
      }
      setRemoteProfiles((current) => {
        const ids = new Set(savedProfiles.map((profile) => profile.id));
        return [...current.filter((profile) => !ids.has(profile.id)), ...savedProfiles]
          .sort((left, right) => left.name.localeCompare(right.name, "zh-CN"));
      });
      const last = savedProfiles.at(-1);
      if (last) {
        setActiveConnectionId(last.id);
        setResult(null);
      }
    } catch (reason) {
      setError(`创建文件数据源失败：${String(reason)}`);
    } finally {
      setConnectionBusy(false);
    }
  }, []);

  useEffect(() => {
    const handleNativeDrop = (event: Event) => {
      const paths = (event as CustomEvent<{ paths?: string[] }>).detail?.paths ?? [];
      void connectDroppedFiles(paths);
    };
    window.addEventListener("drpa-data-file-drop", handleNativeDrop);
    return () => window.removeEventListener("drpa-data-file-drop", handleNativeDrop);
  }, [connectDroppedFiles]);

  useEffect(() => {
    localStorage.setItem(queryHistoryStorageKey(), JSON.stringify(history.slice(0, 30)));
  }, [history]);

  useEffect(() => {
    localStorage.setItem(queryPageSizeStorageKey(), String(pageSize));
  }, [pageSize]);

  useEffect(() => {
    if (!tableMenu) return;
    const close = () => setTableMenu(null);
    const closeOnKey = (event: KeyboardEvent) => { if (event.key === "Escape") close(); };
    window.addEventListener("pointerdown", close);
    window.addEventListener("blur", close);
    window.addEventListener("resize", close);
    window.addEventListener("keydown", closeOnKey);
    return () => {
      window.removeEventListener("pointerdown", close);
      window.removeEventListener("blur", close);
      window.removeEventListener("resize", close);
      window.removeEventListener("keydown", closeOnKey);
    };
  }, [tableMenu]);

  const inspectTable = async (table: DatabaseTable) => {
    setSelectedTable(table.name);
    setError("");
    try {
      setColumns(activeProfile
        ? await desktopGateway.describeRemoteDatabaseTable(activeProfile.id, connectionPasswords[activeProfile.id] ?? "", table.name)
        : await desktopGateway.describeDatabaseTable(table.name));
    } catch (reason) {
      setColumns([]);
      setError(String(reason));
    }
  };

  const openTableQuery = (table: DatabaseTable) => {
    void inspectTable(table);
    setSql(`SELECT *\nFROM ${quoteTableIdentifier(table.name, activeProfile?.engine)};\n`);
    window.setTimeout(() => editorRef.current?.focus(), 0);
  };

  const executeStatement = useCallback(async (statement: string, page = 1, requestedPageSize = pageSize, recordHistory = true) => {
    statement = statement.trim();
    if (!statement || executingRef.current) return;
    const offset = Math.max(0, page - 1) * requestedPageSize;
    executingRef.current = true;
    setExecuting(true);
    setError("");
    setCopied(false);
    setExportStatus("");
    try {
      const nextResult = activeProfile
        ? await desktopGateway.executeRemoteDatabaseSql(activeProfile.id, connectionPasswords[activeProfile.id] ?? "", statement, offset, requestedPageSize)
        : await desktopGateway.executeDatabaseSql(statement, offset, requestedPageSize);
      setResult(nextResult);
      setLastStatement(statement);
      if (recordHistory) {
        setResultFilter("");
        setResultSort(null);
        setHistory((current) => [{
          id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
          sql: statement.slice(0, 20_000),
          statementType: nextResult.statementType,
          executedAt: Date.now(),
          durationMs: nextResult.durationMs,
          rowCount: nextResult.rows.length,
        }, ...current].slice(0, 30));
      }
      if (!["SELECT", "EXPLAIN", "PRAGMA", "WITH"].includes(nextResult.statementType)) {
        await refreshSchema();
      }
    } catch (reason) {
      setError(String(reason));
    } finally {
      executingRef.current = false;
      setExecuting(false);
    }
  }, [activeProfile, connectionPasswords, pageSize, refreshSchema]);

  const runSql = useCallback(async (editor = editorRef.current, page = 1, requestedPageSize = pageSize) => {
    if (!editor) return;
    const model = editor.getModel();
    const selection = editor.getSelection();
    const selected = model && selection && !selection.isEmpty() ? model.getValueInRange(selection) : "";
    const statement = (selected || editor.getValue()).trim();
    await executeStatement(statement, page, requestedPageSize, page === 1);
  }, [executeStatement, pageSize]);

  runSqlRef.current = runSql;

  const handleEditorMount = (editor: monaco.editor.IStandaloneCodeEditor, _monaco: Monaco) => {
    editorRef.current = editor;
    editor.addAction({
      id: "drpa.execute-sql",
      label: "执行 SQL",
      keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter],
      run: () => runSqlRef.current(editor),
    });
  };

  const copyResult = async () => {
    if (!result || result.columns.length === 0) return;
    const text = [result.columns, ...result.rows]
      .map((row) => row.map(formatCell).join("\t"))
      .join("\n");
    await navigator.clipboard.writeText(text);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };

  const openAssistant = async () => {
    setAssistantOpen(true);
    setAssistantError("");
    if (schemaContext) return;
    try {
      setSchemaContext(activeProfile
        ? await desktopGateway.getRemoteDatabaseSchemaContext(activeProfile.id, connectionPasswords[activeProfile.id] ?? "")
        : await desktopGateway.getDatabaseSchemaContext());
    } catch (reason) {
      setAssistantError(`读取数据库结构失败：${String(reason)}`);
    }
  };

  const generateSql = async () => {
    const requirement = assistantPrompt.trim();
    if (!requirement || assistantBusy) return;
    const config = useAppStore.getState();
    if (!config.agentBaseUrl.trim() || !config.agentModel.trim()) {
      setAssistantError("请先在设置中填写 OpenAI 兼容 URL 和模型名称");
      return;
    }
    setAssistantBusy(true);
    setAssistantError("");
    setAssistantResponse("");
    const requestId = `sql-${Date.now()}-${Math.random().toString(16).slice(2)}`;
    let unlisten: (() => void) | undefined;
    try {
      const schema = schemaContext || (activeProfile
        ? await desktopGateway.getRemoteDatabaseSchemaContext(activeProfile.id, connectionPasswords[activeProfile.id] ?? "")
        : await desktopGateway.getDatabaseSchemaContext());
      setSchemaContext(schema);
      if (config.agentStreamEnabled) {
        unlisten = await desktopGateway.listenAgentStream(requestId, (event) => {
          if (event.type === "roundStarted") setAssistantResponse("");
          if (event.type === "delta") setAssistantResponse((current) => current + event.content);
        });
      }
      const response = await desktopGateway.runAgentTurn({
        requestId,
        baseUrl: config.agentBaseUrl.trim(),
        model: config.agentModel.trim(),
        mode: "sql",
        databaseDialect: activeProfile?.engine === "postgresql" || activeProfile?.engine === "mysql"
          ? activeProfile.engine
          : "sqlite",
        apiKey: config.agentApiKey,
        projectId: "",
        stream: config.agentStreamEnabled,
        contextWindow: config.agentContextWindow,
        maxOutputTokens: config.agentMaxOutputTokens,
        maxRounds: config.agentMaxRounds,
        temperature: config.agentTemperature,
        pythonTimeoutSeconds: config.agentPythonTimeoutSeconds,
        selectedSkillIds: [],
        toolPolicy: config.agentToolPolicy,
        messages: [{
          role: "user",
          content: [
            `需求：${requirement}`,
            "",
            schema,
            "",
            "当前编辑器内容（仅供参考，可重新编写）：",
            "```sql",
            sql.slice(0, 20_000),
            "```",
          ].join("\n"),
        }],
      });
      setAssistantResponse(response.message);
    } catch (reason) {
      setAssistantError(`生成 SQL 失败：${String(reason)}`);
    } finally {
      unlisten?.();
      setAssistantBusy(false);
    }
  };

  const generatedSql = useMemo(() => extractSqlBlock(assistantResponse), [assistantResponse]);
  const insertGeneratedSql = (append: boolean) => {
    if (!generatedSql) return;
    setSql((current) => append && current.trim() ? `${current.trimEnd()}\n\n${generatedSql}\n` : `${generatedSql}\n`);
    setAssistantOpen(false);
    window.setTimeout(() => editorRef.current?.focus(), 0);
  };

  const editConnection = (profile?: RemoteDatabaseProfile) => {
    const next = profile ?? createRemoteProfile();
    setConnectionDraft({ ...next });
    setConnectionPassword(profile ? connectionPasswords[profile.id] ?? "" : "");
    setConnectionError("");
    setConnectionTest(null);
  };

  const pickConnectionFile = async () => {
    if (!connectionDraft || !isFileDatabase(connectionDraft.engine)) return;
    const selected = await desktopGateway.selectDatabaseSourceFile(connectionDraft.engine);
    if (selected) {
      setConnectionDraft({ ...connectionDraft, database: selected });
    }
  };

  const testConnection = async () => {
    if (!connectionDraft || connectionBusy) return;
    setConnectionBusy(true);
    setConnectionError("");
    setConnectionTest(null);
    try {
      setConnectionTest(await desktopGateway.testRemoteDatabaseConnection(connectionDraft, connectionPassword));
    } catch (reason) {
      setConnectionError(String(reason));
    } finally {
      setConnectionBusy(false);
    }
  };

  const saveAndConnect = async () => {
    if (!connectionDraft || connectionBusy) return;
    setConnectionBusy(true);
    setConnectionError("");
    try {
      const tested = await desktopGateway.testRemoteDatabaseConnection(connectionDraft, connectionPassword);
      const saved = await desktopGateway.saveRemoteDatabaseProfile(connectionDraft);
      setRemoteProfiles((current) => [...current.filter((profile) => profile.id !== saved.id), saved].sort((left, right) => left.name.localeCompare(right.name, "zh-CN")));
      setConnectionPasswords((current) => ({ ...current, [saved.id]: connectionPassword }));
      setConnectionTest(tested);
      setConnectionDraft(null);
      setActiveConnectionId(saved.id);
      setResult(null);
      setError("");
    } catch (reason) {
      setConnectionError(String(reason));
    } finally {
      setConnectionBusy(false);
    }
  };

  const selectRemoteConnection = (profile: RemoteDatabaseProfile) => {
    if (!isFileDatabase(profile.engine) && !(profile.id in connectionPasswords)) {
      editConnection(profile);
      return;
    }
    setActiveConnectionId(profile.id);
    setResult(null);
    setError("");
  };

  const deleteConnection = async () => {
    if (!connectionDraft?.id || connectionBusy) return;
    setConnectionBusy(true);
    setConnectionError("");
    try {
      await desktopGateway.deleteRemoteDatabaseProfile(connectionDraft.id);
      setRemoteProfiles((current) => current.filter((profile) => profile.id !== connectionDraft.id));
      setConnectionPasswords((current) => {
        const next = { ...current };
        delete next[connectionDraft.id];
        return next;
      });
      if (activeConnectionId === connectionDraft.id) setActiveConnectionId("local");
      setConnectionDraft(null);
    } catch (reason) {
      setConnectionError(String(reason));
    } finally {
      setConnectionBusy(false);
    }
  };

  const currentPage = result ? Math.floor(result.offset / Math.max(1, result.limit)) + 1 : 1;
  const displayedRows = useMemo(() => {
    if (!result) return [];
    const filter = resultFilter.trim().toLocaleLowerCase();
    const rows = result.rows
      .map((row, index) => ({ row, index }))
      .filter(({ row }) => !filter || row.some((value) => formatCell(value).toLocaleLowerCase().includes(filter)));
    if (!resultSort) return rows;
    return [...rows].sort((left, right) => {
      const comparison = compareResultValues(left.row[resultSort.column], right.row[resultSort.column]);
      return resultSort.direction === "asc" ? comparison : -comparison;
    });
  }, [result, resultFilter, resultSort]);

  const changeResultSort = (column: number) => {
    setResultSort((current) => current?.column === column
      ? current.direction === "asc" ? { column, direction: "desc" } : null
      : { column, direction: "asc" });
  };

  const exportResult = async () => {
    if (!result || result.columns.length === 0 || exporting) return;
    const sourceName = sanitizeExportName(selectedTable || activeProfile?.name || "query-result");
    const targetPath = await desktopGateway.selectDatabaseExportPath(exportFormat, sourceName);
    if (!targetPath) return;
    setExporting(true);
    setExportStatus(exportScope === "all" ? "正在读取全部结果…" : "");
    try {
      let exportPayload = result;
      if (exportScope === "all" && lastStatement) {
        const rows: unknown[][] = [];
        let offset = 0;
        let chunk: DatabaseQueryResult;
        do {
          chunk = activeProfile
            ? await desktopGateway.executeRemoteDatabaseSql(activeProfile.id, connectionPasswords[activeProfile.id] ?? "", lastStatement, offset, 20_000)
            : await desktopGateway.executeDatabaseSql(lastStatement, offset, 20_000);
          rows.push(...chunk.rows);
          offset += chunk.rows.length;
          setExportStatus(`正在读取全部结果… ${offset.toLocaleString()} 行`);
          if (rows.length >= 1_000_000 && chunk.hasMore) throw new Error("结果超过 1,000,000 行，请增加 WHERE 条件后分批导出");
        } while (chunk.hasMore && chunk.rows.length > 0);
        exportPayload = { ...chunk, rows, offset: 0, limit: rows.length, hasMore: false };
      }
      const exported = await desktopGateway.exportDatabaseQueryResult(exportPayload, exportFormat, targetPath, selectedTable || "query_result");
      setExportStatus(`已导出 ${exported.rowCount} 行：${exported.path}`);
    } catch (reason) {
      setError(`导出失败：${String(reason)}`);
    } finally {
      setExporting(false);
    }
  };

  const setTableStatement = (table: DatabaseTable, statement: string) => {
    setSelectedTable(table.name);
    setSql(`${statement.trim()}\n`);
    setTableMenu(null);
    window.setTimeout(() => editorRef.current?.focus(), 0);
  };

  const browseTable = async (table: DatabaseTable) => {
    const statement = `SELECT *\nFROM ${quoteTableIdentifier(table.name, activeProfile?.engine)};`;
    setTableStatement(table, statement);
    void inspectTable(table);
    await executeStatement(statement, 1, pageSize);
  };

  const countTable = async (table: DatabaseTable) => {
    const statement = `SELECT COUNT(*) AS row_count\nFROM ${quoteTableIdentifier(table.name, activeProfile?.engine)};`;
    setTableStatement(table, statement);
    await executeStatement(statement, 1, pageSize);
  };

  const createMutationTemplate = async (table: DatabaseTable, kind: "insert" | "update") => {
    setTableMenu(null);
    let tableColumns: DatabaseColumn[];
    try {
      tableColumns = activeProfile
        ? await desktopGateway.describeRemoteDatabaseTable(activeProfile.id, connectionPasswords[activeProfile.id] ?? "", table.name)
        : await desktopGateway.describeDatabaseTable(table.name);
    } catch (reason) {
      setError(`读取字段结构失败：${String(reason)}`);
      return;
    }
    setSelectedTable(table.name);
    setColumns(tableColumns);
    const quotedTable = quoteTableIdentifier(table.name, activeProfile?.engine);
    const quotedColumns = tableColumns.map((column) => quoteTableIdentifier(column.name, activeProfile?.engine));
    if (kind === "insert") {
      setSql(`INSERT INTO ${quotedTable} (\n  ${quotedColumns.join(",\n  ")}\n) VALUES (\n  ${tableColumns.map((column) => `:${parameterName(column.name)}`).join(",\n  ")}\n);\n`);
    } else {
      const key = tableColumns.find((column) => column.primaryKey) ?? tableColumns[0];
      const editable = tableColumns.filter((column) => column !== key);
      setSql(`UPDATE ${quotedTable}\nSET\n  ${editable.map((column) => `${quoteTableIdentifier(column.name, activeProfile?.engine)} = :${parameterName(column.name)}`).join(",\n  ")}\nWHERE ${key ? `${quoteTableIdentifier(key.name, activeProfile?.engine)} = :${parameterName(key.name)}` : "/* 请填写条件 */"};\n`);
    }
    window.setTimeout(() => editorRef.current?.focus(), 0);
  };

  return (
    <div className="page data-page">
      <header className="page-header data-header">
        <div>
          <div className="eyebrow">SQLite · PostgreSQL · MySQL · Excel · AI SQL</div>
          <h1>数据工作台</h1>
          <p>连接本地或远程数据库、浏览结构并直接编写和执行 SQL。</p>
        </div>
        <div className="header-actions">
          <button className="button secondary" type="button" onClick={() => void desktopGateway.openWorkspaceDatabaseDirectory()} disabled={Boolean(activeProfile)} title={activeProfile ? "远程连接没有本地目录" : undefined}><FolderOpen size={15} /> 打开目录</button>
          <button className="button secondary" type="button" onClick={() => void refreshSchema()} disabled={loading}><RefreshCw size={15} /> 刷新结构</button>
          <button className="button secondary ai-sql-button" type="button" onClick={() => void openAssistant()}><Sparkles size={15} /> AI 写 SQL</button>
          <button className="button primary" type="button" onClick={() => void runSql()} disabled={executing}>{executing ? <LoaderCircle className="spin" size={15} /> : <Play size={15} fill="currentColor" />} 执行 <kbd>Ctrl Enter</kbd></button>
        </div>
      </header>

      <div className={`data-workspace${schemaCollapsed ? " schema-collapsed" : ""}${inspectorCollapsed ? " inspector-collapsed" : ""}`}>
        {schemaCollapsed ? <SidebarToggle id="data-schema" side="left" label="数据连接侧边栏" restore /> : <aside className="data-schema-pane collapsible-sidebar">
          <SidebarToggle id="data-schema" side="left" label="数据连接侧边栏" />
          <div className="data-pane-title"><Database size={15} /> 连接 <button type="button" aria-label="新建数据库连接" title="新建 PostgreSQL / MySQL / SQLite / Excel 数据源" onClick={() => editConnection()}><Plus size={13} /></button></div>
          <div className="database-connections-list">
            <button className={`database-connection ${activeConnectionId === "local" ? "active" : ""}`} type="button" onClick={() => { setActiveConnectionId("local"); setResult(null); setError(""); }}>
              <span className="database-icon"><Database size={16} /></span>
              <span><strong>工作区数据库</strong><small>SQLite · 本地脚本共享</small></span>
              <span className="connection-state" title="连接正常" />
            </button>
            {remoteProfiles.map((profile) => (
              <div className={`remote-connection-row ${activeConnectionId === profile.id ? "active" : ""}`} key={profile.id}>
                <button className="remote-connection-select" type="button" onClick={() => selectRemoteConnection(profile)}>
                  <span className="database-icon remote"><Cable size={15} /></span>
                  <span><strong>{profile.name}</strong><small>{databaseEngineLabel(profile.engine)} · {connectionLocation(profile)}</small></span>
                  <span className={`connection-state ${isFileDatabase(profile.engine) || profile.id in connectionPasswords ? "" : "idle"}`} title={isFileDatabase(profile.engine) || profile.id in connectionPasswords ? "本次会话已连接" : "需要输入密码"} />
                </button>
                <button className="remote-connection-settings" type="button" aria-label={`编辑数据库连接 ${profile.name}`} onClick={() => editConnection(profile)}><Settings2 size={12} /></button>
              </div>
            ))}
          </div>
          <div className="data-pane-section"><span>数据表</span><em>{visibleTables.length === tables.length ? tables.length : `${visibleTables.length}/${tables.length}`}</em></div>
          <label className="database-table-search"><Search size={12} /><input value={tableFilter} onChange={(event) => setTableFilter(event.target.value)} placeholder="搜索表或视图" /></label>
          <div className="database-tables">
            {loading && <div className="data-loading"><LoaderCircle className="spin" size={14} /> 正在读取结构</div>}
            {!loading && tables.length === 0 && <div className="data-empty">尚无数据表。执行 CREATE TABLE 开始建模。</div>}
            {!loading && tables.length > 0 && visibleTables.length === 0 && <div className="data-empty">没有匹配的表或视图。</div>}
            {visibleTables.map((table) => (
              <button
                className={selectedTable === table.name ? "selected" : ""}
                key={table.name}
                type="button"
                onClick={() => void inspectTable(table)}
                onDoubleClick={() => openTableQuery(table)}
                onContextMenu={(event) => {
                  event.preventDefault();
                  event.stopPropagation();
                  setTableMenu({ table, x: Math.min(event.clientX, window.innerWidth - 220), y: Math.min(event.clientY, window.innerHeight - 300) });
                }}
                title="单击查看字段，双击生成查询，右键打开表操作"
              >
                {table.kind === "view" ? <Braces size={14} /> : <Table2 size={14} />}
                <span>{table.name}</span>
                <small>{table.kind === "view" ? "视图" : "表"}</small>
                <ChevronRight size={13} />
              </button>
            ))}
          </div>
          <div className="database-path" title={database?.path}><span>{activeProfile ? "端点" : "文件"}</span><code>{database?.path ?? "正在定位…"}</code></div>
        </aside>}

        <main className="data-query-pane">
          <div className="query-tabs">
            <button className="active" type="button"><span className="query-dot" /> 查询 1</button>
            <button type="button" aria-label="新建查询" onClick={() => { setSql(""); setResult(null); setError(""); }}><Plus size={14} /></button>
            <span>{database?.engine ?? "SQLite"}</span>
          </div>
          <div className="sql-editor">
            <Editor
              path="drpa-data://workspace/query.sql"
              height="100%"
              language="sql"
              value={sql}
              onChange={(value) => setSql(value ?? "")}
              onMount={handleEditorMount}
              theme={theme === "dark" ? "vs-dark" : "light"}
              options={{
                fontSize: 13,
                fontFamily: "var(--font-mono)",
                minimap: { enabled: false },
                automaticLayout: true,
                lineNumbersMinChars: 3,
                scrollBeyondLastLine: false,
                wordWrap: "on",
                padding: { top: 12, bottom: 12 },
              }}
            />
          </div>
          <section className="query-results">
            <header>
              <div className="query-result-title"><strong>查询结果</strong>{result && <span>第 {currentPage} 页 · {result.rows.length} 行 · {result.durationMs} ms{result.hasMore ? " · 还有更多" : ""}{result.truncated ? " · 单元格/容量已截断" : ""}</span>}</div>
              <div className="query-result-tools">
                <label className="result-search" title="筛选当前页"><Search size={12} /><input value={resultFilter} onChange={(event) => setResultFilter(event.target.value)} placeholder="筛选当前页" /></label>
                <label className="result-page-size"><span>每页</span><select value={pageSize} onChange={(event) => { const nextSize = Number(event.target.value); setPageSize(nextSize); if (lastStatement) void executeStatement(lastStatement, 1, nextSize, false); }} disabled={executing}>{[100, 500, 1_500, 5_000, 10_000, 20_000].map((size) => <option value={size} key={size}>{size.toLocaleString()}</option>)}</select></label>
                <label className="result-export-format"><select aria-label="导出格式" value={exportFormat} onChange={(event) => setExportFormat(event.target.value as DatabaseExportFormat)}><option value="xlsx">XLSX</option><option value="xls">XLS</option><option value="csv">CSV</option><option value="json">JSON</option><option value="sql">SQL</option></select></label>
                <label className="result-export-format"><select aria-label="导出范围" value={exportScope} onChange={(event) => setExportScope(event.target.value as "page" | "all")}><option value="all">全部结果</option><option value="page">当前页</option></select></label>
                <button type="button" onClick={() => void exportResult()} disabled={!result || result.columns.length === 0 || exporting} title={exportScope === "all" ? "重新执行查询并导出全部结果" : "导出当前页查询结果"}>{exporting ? <LoaderCircle className="spin" size={13} /> : <Download size={13} />} 导出</button>
                <button type="button" onClick={() => void copyResult()} disabled={!result || result.columns.length === 0}>{copied ? <Check size={13} /> : <Copy size={13} />} {copied ? "已复制" : "TSV"}</button>
              </div>
            </header>
            {exportStatus && <div className="query-export-status"><Check size={12} /> <span title={exportStatus}>{exportStatus}</span></div>}
            {error && <div className="query-error"><strong>SQL 执行失败</strong><pre>{error}</pre></div>}
            {!error && !result && <div className="query-placeholder"><Rows3 size={23} /><span>执行 SQL 后在这里查看结果</span></div>}
            {!error && result && result.columns.length === 0 && <div className="query-success"><Check size={22} /><strong>执行成功</strong><span>影响 {result.affectedRows} 行 · {result.durationMs} ms</span></div>}
            {!error && result && result.columns.length > 0 && (
              <div className="result-grid-wrap">
                <table className="result-grid">
                  <thead><tr><th className="row-number">#</th>{result.columns.map((column, index) => <th key={`${column}-${index}`}><button type="button" onClick={() => changeResultSort(index)} title="按此列排序当前页"><span>{column}</span>{resultSort?.column === index && <em>{resultSort.direction === "asc" ? "↑" : "↓"}</em>}</button></th>)}</tr></thead>
                  <tbody>{displayedRows.map(({ row, index: rowIndex }) => <tr key={rowIndex}><td className="row-number">{result.offset + rowIndex + 1}</td>{row.map((value, columnIndex) => <td key={columnIndex} title={formatCell(value)} onDoubleClick={() => void navigator.clipboard.writeText(formatCell(value))}>{formatCell(value)}</td>)}</tr>)}</tbody>
                </table>
              </div>
            )}
            {!error && result && result.columns.length > 0 && <footer className="query-pagination"><span>显示 {displayedRows.length} / {result.rows.length} 行{resultFilter ? "（当前页筛选）" : ""}</span><div><button type="button" disabled={executing || result.offset === 0} onClick={() => void executeStatement(lastStatement, currentPage - 1, pageSize, false)}><ChevronLeft size={13} /> 上一页</button><strong>第 {currentPage} 页</strong><button type="button" disabled={executing || !result.hasMore} onClick={() => void executeStatement(lastStatement, currentPage + 1, pageSize, false)}>下一页 <ChevronRight size={13} /></button></div></footer>}
          </section>
        </main>

        {inspectorCollapsed ? <SidebarToggle id="data-inspector" side="right" label="字段结构侧边栏" restore /> : <aside className="data-inspector-pane collapsible-sidebar">
          <SidebarToggle id="data-inspector" side="right" label="字段结构侧边栏" />
          <div className="data-pane-title"><Rows3 size={15} /> 字段结构</div>
          {selectedTable ? <div className="inspector-table-name"><Table2 size={14} /><strong>{selectedTable}</strong><span>{columns.length} 个字段</span></div> : <div className="data-empty inspector-empty">选择左侧数据表查看字段定义。</div>}
          <div className="column-list">
            {columns.map((column) => <div key={column.name} className="column-card"><span className={column.primaryKey ? "column-key primary" : "column-key"}>{column.primaryKey ? <KeyRound size={12} /> : column.ordinal + 1}</span><span><strong>{column.name}</strong><small>{column.dataType || "ANY"}{column.notNull ? " · NOT NULL" : ""}</small></span>{column.defaultValue && <code title={column.defaultValue}>{column.defaultValue}</code>}</div>)}
          </div>
          <div className="data-pane-section history-title"><span>查询历史</span><em>{history.length}</em></div>
          <div className="query-history">
            {history.length === 0 && <div className="data-empty">本次执行的查询将显示在这里。</div>}
            {history.map((entry) => <button type="button" key={entry.id} onClick={() => { setSql(entry.sql); window.setTimeout(() => editorRef.current?.focus(), 0); }}><span><strong>{entry.statementType}</strong><small>{entry.sql.replace(/\s+/g, " ").slice(0, 68)}</small></span><em><Clock3 size={11} /> {formatTime(entry.executedAt)} · {entry.durationMs} ms</em></button>)}
          </div>
        </aside>}
      </div>

      {tableMenu && (
        <div className="data-table-context-menu" style={{ left: tableMenu.x, top: tableMenu.y }} role="menu" onPointerDown={(event) => event.stopPropagation()} onContextMenu={(event) => event.preventDefault()}>
          <header>{tableMenu.table.kind === "view" ? <Braces size={14} /> : <Table2 size={14} />}<span><strong>{tableMenu.table.name}</strong><small>{tableMenu.table.kind === "view" ? "视图操作" : "数据表操作"}</small></span></header>
          <button type="button" role="menuitem" onClick={() => void browseTable(tableMenu.table)}><Eye size={14} /><span>浏览数据</span><kbd>双击</kbd></button>
          <button type="button" role="menuitem" onClick={() => void countTable(tableMenu.table)}><Hash size={14} /><span>统计行数</span></button>
          <button type="button" role="menuitem" onClick={() => { void inspectTable(tableMenu.table); setTableMenu(null); }}><Rows3 size={14} /><span>查看字段结构</span></button>
          <div className="context-menu-separator" />
          <button type="button" role="menuitem" onClick={() => setTableStatement(tableMenu.table, `SELECT *\nFROM ${quoteTableIdentifier(tableMenu.table.name, activeProfile?.engine)};`)}><FileCode2 size={14} /><span>生成 SELECT</span></button>
          <button type="button" role="menuitem" disabled={tableMenu.table.kind === "view" || activeProfile?.engine === "excel"} onClick={() => void createMutationTemplate(tableMenu.table, "insert")}><Plus size={14} /><span>生成 INSERT</span></button>
          <button type="button" role="menuitem" disabled={tableMenu.table.kind === "view" || activeProfile?.engine === "excel"} onClick={() => void createMutationTemplate(tableMenu.table, "update")}><FileCode2 size={14} /><span>生成 UPDATE</span></button>
          {(activeProfile?.engine === "sqlite" || !activeProfile) && <button type="button" role="menuitem" onClick={() => setTableStatement(tableMenu.table, `SELECT type, name, sql\nFROM sqlite_master\nWHERE name = '${escapeSqlString(tableMenu.table.name)}';`)}><Braces size={14} /><span>查看建表 SQL</span></button>}
          <div className="context-menu-separator" />
          <button type="button" role="menuitem" onClick={() => { void navigator.clipboard.writeText(tableMenu.table.name); setTableMenu(null); }}><ClipboardCopy size={14} /><span>复制表名</span></button>
          <button type="button" role="menuitem" onClick={() => { setTableMenu(null); void refreshSchema(); }}><RefreshCw size={14} /><span>刷新数据库结构</span></button>
        </div>
      )}

      {connectionDraft && (
        <div className="database-dialog-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget && !connectionBusy) setConnectionDraft(null); }}>
          <form className="database-dialog" role="dialog" aria-modal="true" aria-labelledby="database-dialog-title" onSubmit={(event) => { event.preventDefault(); void saveAndConnect(); }}>
            <header>
              <span className="sql-ai-icon"><Cable size={17} /></span>
              <div><h2 id="database-dialog-title">{remoteProfiles.some((profile) => profile.id === connectionDraft.id) ? "编辑数据源" : "新建数据源"}</h2><p>{isFileDatabase(connectionDraft.engine) ? "文件路径保存在工作区；Excel 工作簿以只读方式查询。" : "连接信息保存在工作区，密码仅保留到本次应用退出。"}</p></div>
              <button type="button" aria-label="关闭数据库连接设置" onClick={() => setConnectionDraft(null)} disabled={connectionBusy}><X size={16} /></button>
            </header>
            <div className="database-dialog-fields">
              <label><span>连接名称</span><input required maxLength={80} value={connectionDraft.name} onChange={(event) => setConnectionDraft({ ...connectionDraft, name: event.target.value })} placeholder="例如：业务分析库" /></label>
              <div className={`database-field-row ${isFileDatabase(connectionDraft.engine) ? "single" : ""}`}>
                <label><span>数据源类型</span><select value={connectionDraft.engine} onChange={(event) => { const engine = event.target.value as RemoteDatabaseProfile["engine"]; setConnectionDraft({ ...connectionDraft, engine, port: engine === "postgresql" ? 5432 : engine === "mysql" ? 3306 : 0, host: isFileDatabase(engine) ? "" : connectionDraft.host || "localhost", username: isFileDatabase(engine) ? "" : connectionDraft.username, database: "" }); setConnectionTest(null); setConnectionError(""); }}><option value="postgresql">PostgreSQL</option><option value="mysql">MySQL</option><option value="sqlite">SQLite 文件</option><option value="excel">Excel 工作簿（只读）</option></select></label>
                {!isFileDatabase(connectionDraft.engine) && <label><span>TLS</span><select value={connectionDraft.tlsMode} onChange={(event) => setConnectionDraft({ ...connectionDraft, tlsMode: event.target.value as RemoteDatabaseProfile["tlsMode"] })}><option value="require">必须</option><option value="prefer">优先</option><option value="disable">关闭</option></select></label>}
              </div>
              {isFileDatabase(connectionDraft.engine) ? (
                <>
                  <label><span>{connectionDraft.engine === "excel" ? "工作簿文件" : "SQLite 数据库文件"}</span><div className="database-file-input"><input required value={connectionDraft.database} onChange={(event) => setConnectionDraft({ ...connectionDraft, database: event.target.value })} placeholder={connectionDraft.engine === "excel" ? "选择 .xls / .xlsx / .xlsb / .ods 文件" : "选择 .db / .sqlite / .sqlite3 文件"} /><button className="button secondary" type="button" onClick={() => void pickConnectionFile()}><FolderOpen size={14} /> 浏览</button></div></label>
                  <div className="database-source-note"><Database size={14} /><span>{connectionDraft.engine === "excel" ? "首行作为字段名，每个工作表映射为一张只读表，可使用 SELECT 查询。" : "直接连接已有 SQLite 文件，支持结构浏览和 SQL 读写。"}</span></div>
                </>
              ) : (
                <>
                  <div className="database-field-row host-row">
                    <label><span>主机</span><input required value={connectionDraft.host} onChange={(event) => setConnectionDraft({ ...connectionDraft, host: event.target.value })} placeholder="db.example.com" /></label>
                    <label><span>端口</span><input required type="number" min={1} max={65535} value={connectionDraft.port} onChange={(event) => setConnectionDraft({ ...connectionDraft, port: Number(event.target.value) })} /></label>
                  </div>
                  <label><span>数据库</span><input required value={connectionDraft.database} onChange={(event) => setConnectionDraft({ ...connectionDraft, database: event.target.value })} placeholder="database_name" /></label>
                  <label><span>用户名</span><input required value={connectionDraft.username} onChange={(event) => setConnectionDraft({ ...connectionDraft, username: event.target.value })} autoComplete="username" /></label>
                  <label><span>密码</span><input type="password" value={connectionPassword} onChange={(event) => setConnectionPassword(event.target.value)} autoComplete="current-password" placeholder="仅保存在内存中" /></label>
                </>
              )}
              {connectionTest && <div className="database-test-success"><Check size={14} /><span><strong>连接成功 · {connectionTest.latencyMs} ms</strong><small>{connectionTest.serverVersion}</small></span></div>}
              {connectionError && <div className="sql-ai-error">{connectionError}</div>}
            </div>
            <footer>
              {remoteProfiles.some((profile) => profile.id === connectionDraft.id) && <button className="button danger database-delete-connection" type="button" onClick={() => void deleteConnection()} disabled={connectionBusy}><Trash2 size={14} /> 删除</button>}
              <span />
              <button className="button secondary" type="button" onClick={() => void testConnection()} disabled={connectionBusy}>{connectionBusy ? <LoaderCircle className="spin" size={14} /> : <Cable size={14} />} 测试连接</button>
              <button className="button primary" type="submit" disabled={connectionBusy}>{connectionBusy ? <LoaderCircle className="spin" size={14} /> : <Check size={14} />} 保存并连接</button>
            </footer>
          </form>
        </div>
      )}

      {assistantOpen && (
        <div className="sql-ai-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget && !assistantBusy) setAssistantOpen(false); }}>
          <section className="sql-ai-drawer" role="dialog" aria-modal="true" aria-labelledby="sql-ai-title">
            <header>
              <span className="sql-ai-icon"><Sparkles size={17} /></span>
              <div><h2 id="sql-ai-title">AI SQL 助手</h2><p>结合当前 {database?.engine ?? "SQLite"} 结构生成代码，不会自动执行。</p></div>
              <button type="button" aria-label="关闭 AI SQL 助手" onClick={() => setAssistantOpen(false)} disabled={assistantBusy}><X size={16} /></button>
            </header>
            <div className="sql-ai-body">
              <label htmlFor="sql-ai-prompt">描述查询需求</label>
              <textarea
                id="sql-ai-prompt"
                autoFocus
                value={assistantPrompt}
                onChange={(event) => setAssistantPrompt(event.target.value)}
                onKeyDown={(event) => { if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) { event.preventDefault(); void generateSql(); } }}
                placeholder="例如：按状态统计任务数量，并按数量从高到低排序"
              />
              <div className="sql-ai-context"><Database size={13} /><span>{schemaContext ? `已读取结构 · ${formatBytes(new Blob([schemaContext]).size)}` : "正在等待读取数据库结构"}</span><em>{database?.engine ?? "SQLite"}</em></div>
              <button className="button primary sql-ai-generate" type="button" onClick={() => void generateSql()} disabled={!assistantPrompt.trim() || assistantBusy}>{assistantBusy ? <LoaderCircle className="spin" size={15} /> : <Sparkles size={15} />} {assistantBusy ? "正在生成…" : "生成 SQL"}<kbd>Ctrl Enter</kbd></button>
              {assistantError && <div className="sql-ai-error">{assistantError}</div>}
              {(assistantResponse || assistantBusy) && (
                <section className={`sql-ai-response ${assistantBusy ? "streaming" : ""}`} aria-live="polite">
                  <header><strong>生成结果</strong><span>{assistantBusy ? "实时生成中" : generatedSql ? "SQL 已就绪" : "未识别到 SQL 代码块"}</span></header>
                  <div className="sql-ai-markdown"><Markdown remarkPlugins={[remarkGfm]}>{assistantResponse || "正在分析数据库结构…"}</Markdown>{assistantBusy && <span className="sql-ai-caret" />}</div>
                </section>
              )}
            </div>
            <footer>
              <span>插入后请检查 SQL，再手动执行。</span>
              <button className="button secondary" type="button" onClick={() => insertGeneratedSql(true)} disabled={!generatedSql || assistantBusy}>追加到编辑器</button>
              <button className="button primary" type="button" onClick={() => insertGeneratedSql(false)} disabled={!generatedSql || assistantBusy}>替换编辑器</button>
            </footer>
          </section>
        </div>
      )}
    </div>
  );
}

export function extractSqlBlock(markdown: string): string {
  const match = /```(?:sql|sqlite)\s*\r?\n([\s\S]*?)```/i.exec(markdown);
  return match?.[1].trim() ?? "";
}

function loadQueryHistory(): QueryHistoryEntry[] {
  try {
    const key = queryHistoryStorageKey();
    let source = localStorage.getItem(key);
    if (source === null && key.endsWith(":personal")) {
      source = localStorage.getItem("drpa-data-query-history");
      if (source !== null) localStorage.setItem(key, source);
    }
    const value = JSON.parse(source ?? "[]");
    return Array.isArray(value) ? value.slice(0, 30) : [];
  } catch {
    return [];
  }
}

interface TableContextMenu {
  table: DatabaseTable;
  x: number;
  y: number;
}

type ResultSort = { column: number; direction: "asc" | "desc" } | null;

function queryHistoryStorageKey(): string {
  const workspaceId = localStorage.getItem("drpa-active-workspace-id") ?? "personal";
  return `drpa-data-query-history:${workspaceId}`;
}

function loadQueryPageSize(): number {
  const parsed = Number(localStorage.getItem(queryPageSizeStorageKey()));
  return [100, 500, 1_500, 5_000, 10_000, 20_000].includes(parsed) ? parsed : 500;
}

function queryPageSizeStorageKey(): string {
  const workspaceId = localStorage.getItem("drpa-active-workspace-id") ?? "personal";
  return `drpa-data-query-page-size:${workspaceId}`;
}

function quoteTableIdentifier(value: string, engine?: RemoteDatabaseProfile["engine"]): string {
  return value.split(".").map((part) => engine === "mysql"
    ? `\`${part.replaceAll("`", "``")}\``
    : `"${part.replaceAll('"', '""')}"`).join(".");
}

function createRemoteProfile(engine: RemoteDatabaseProfile["engine"] = "postgresql", sourcePath = ""): RemoteDatabaseProfile {
  const fileName = sourcePath.split(/[\\/]/).at(-1)?.replace(/\.[^.]+$/, "") ?? "";
  return {
    id: `database-${Date.now()}-${Math.random().toString(16).slice(2)}`,
    name: fileName,
    engine,
    host: isFileDatabase(engine) ? "" : "localhost",
    port: engine === "postgresql" ? 5432 : engine === "mysql" ? 3306 : 0,
    database: sourcePath,
    username: "",
    tlsMode: "prefer",
  };
}

function escapeSqlString(value: string): string {
  return value.replaceAll("'", "''");
}

function parameterName(value: string): string {
  const normalized = value.replace(/[^\p{Letter}\p{Number}_]+/gu, "_").replace(/^\d+/, "");
  return normalized || "value";
}

function sanitizeExportName(value: string): string {
  const sanitized = value.replace(/[<>:"/\\|?*\u0000-\u001f]/g, "_").replace(/[. ]+$/g, "").slice(0, 80);
  return sanitized || "query-result";
}

function compareResultValues(left: unknown, right: unknown): number {
  if (left === right) return 0;
  if (left === null || left === undefined) return -1;
  if (right === null || right === undefined) return 1;
  if (typeof left === "number" && typeof right === "number") return left - right;
  return formatCell(left).localeCompare(formatCell(right), "zh-CN", { numeric: true, sensitivity: "base" });
}

function isFileDatabase(engine: RemoteDatabaseProfile["engine"]): engine is "sqlite" | "excel" {
  return engine === "sqlite" || engine === "excel";
}

function fileDatabaseEngine(path: string): "sqlite" | "excel" | null {
  if (/\.(db|sqlite|sqlite3)$/i.test(path)) return "sqlite";
  if (/\.(xls|xlsx|xlsb|ods)$/i.test(path)) return "excel";
  return null;
}

function databaseEngineLabel(engine: RemoteDatabaseProfile["engine"]): string {
  if (engine === "postgresql") return "PostgreSQL";
  if (engine === "mysql") return "MySQL";
  if (engine === "excel") return "Excel";
  return "SQLite";
}

function connectionLocation(profile: RemoteDatabaseProfile): string {
  if (isFileDatabase(profile.engine)) {
    return profile.database.split(/[\\/]/).filter(Boolean).at(-1) ?? profile.database;
  }
  return `${profile.host}:${profile.port}`;
}

function formatCell(value: unknown): string {
  if (value === null || value === undefined) return "NULL";
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function formatTime(timestamp: number): string {
  return new Intl.DateTimeFormat("zh-CN", { hour: "2-digit", minute: "2-digit" }).format(timestamp);
}
