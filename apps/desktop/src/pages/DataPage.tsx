import Editor, { loader, type Monaco } from "@monaco-editor/react";
import * as monaco from "monaco-editor/esm/vs/editor/editor.api.js";
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker.js?worker";
import "monaco-editor/esm/vs/basic-languages/sql/sql.contribution.js";
import {
  Braces,
  Check,
  ChevronRight,
  Clock3,
  Copy,
  Cable,
  Database,
  FolderOpen,
  KeyRound,
  LoaderCircle,
  Play,
  Plus,
  RefreshCw,
  Rows3,
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
import type { DatabaseColumn, DatabaseInfo, DatabaseQueryResult, DatabaseTable, RemoteConnectionTest, RemoteDatabaseProfile } from "../domain/models";
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
  const [database, setDatabase] = useState<DatabaseInfo | null>(null);
  const [tables, setTables] = useState<DatabaseTable[]>([]);
  const [selectedTable, setSelectedTable] = useState("");
  const [columns, setColumns] = useState<DatabaseColumn[]>([]);
  const [sql, setSql] = useState(DEFAULT_SQL);
  const [result, setResult] = useState<DatabaseQueryResult | null>(null);
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
  const activeProfile = useMemo(
    () => remoteProfiles.find((profile) => profile.id === activeConnectionId) ?? null,
    [activeConnectionId, remoteProfiles],
  );

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

  useEffect(() => {
    localStorage.setItem(queryHistoryStorageKey(), JSON.stringify(history.slice(0, 30)));
  }, [history]);

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
    setSql(`SELECT *\nFROM ${quoteTableIdentifier(table.name, activeProfile?.engine)}\nLIMIT 200;\n`);
    window.setTimeout(() => editorRef.current?.focus(), 0);
  };

  const runSql = async (editor = editorRef.current) => {
    if (!editor || executingRef.current) return;
    const model = editor.getModel();
    const selection = editor.getSelection();
    const selected = model && selection && !selection.isEmpty() ? model.getValueInRange(selection) : "";
    const statement = (selected || editor.getValue()).trim();
    if (!statement) return;
    executingRef.current = true;
    setExecuting(true);
    setError("");
    setCopied(false);
    try {
      const nextResult = activeProfile
        ? await desktopGateway.executeRemoteDatabaseSql(activeProfile.id, connectionPasswords[activeProfile.id] ?? "", statement)
        : await desktopGateway.executeDatabaseSql(statement);
      setResult(nextResult);
      setHistory((current) => [{
        id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
        sql: statement.slice(0, 20_000),
        statementType: nextResult.statementType,
        executedAt: Date.now(),
        durationMs: nextResult.durationMs,
        rowCount: nextResult.rows.length,
      }, ...current].slice(0, 30));
      if (!["SELECT", "EXPLAIN", "PRAGMA", "WITH"].includes(nextResult.statementType)) {
        await refreshSchema();
      }
    } catch (reason) {
      setResult(null);
      setError(String(reason));
    } finally {
      executingRef.current = false;
      setExecuting(false);
    }
  };

  const handleEditorMount = (editor: monaco.editor.IStandaloneCodeEditor, _monaco: Monaco) => {
    editorRef.current = editor;
    editor.addAction({
      id: "drpa.execute-sql",
      label: "执行 SQL",
      keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter],
      run: () => runSql(editor),
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

      <div className="data-workspace">
        <aside className="data-schema-pane">
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
          <div className="data-pane-section"><span>数据表</span><em>{tables.length}</em></div>
          <div className="database-tables">
            {loading && <div className="data-loading"><LoaderCircle className="spin" size={14} /> 正在读取结构</div>}
            {!loading && tables.length === 0 && <div className="data-empty">尚无数据表。执行 CREATE TABLE 开始建模。</div>}
            {tables.map((table) => (
              <button
                className={selectedTable === table.name ? "selected" : ""}
                key={table.name}
                type="button"
                onClick={() => void inspectTable(table)}
                onDoubleClick={() => openTableQuery(table)}
                title="单击查看字段，双击生成查询"
              >
                {table.kind === "view" ? <Braces size={14} /> : <Table2 size={14} />}
                <span>{table.name}</span>
                <small>{table.kind === "view" ? "视图" : "表"}</small>
                <ChevronRight size={13} />
              </button>
            ))}
          </div>
          <div className="database-path" title={database?.path}><span>{activeProfile ? "端点" : "文件"}</span><code>{database?.path ?? "正在定位…"}</code></div>
        </aside>

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
              <div><strong>查询结果</strong>{result && <span>{result.rows.length} 行 · {result.durationMs} ms{result.truncated ? " · 已截断" : ""}</span>}</div>
              <button type="button" onClick={() => void copyResult()} disabled={!result || result.columns.length === 0}>{copied ? <Check size={13} /> : <Copy size={13} />} {copied ? "已复制" : "复制 TSV"}</button>
            </header>
            {error && <div className="query-error"><strong>SQL 执行失败</strong><pre>{error}</pre></div>}
            {!error && !result && <div className="query-placeholder"><Rows3 size={23} /><span>执行 SQL 后在这里查看结果</span></div>}
            {!error && result && result.columns.length === 0 && <div className="query-success"><Check size={22} /><strong>执行成功</strong><span>影响 {result.affectedRows} 行 · {result.durationMs} ms</span></div>}
            {!error && result && result.columns.length > 0 && (
              <div className="result-grid-wrap">
                <table className="result-grid">
                  <thead><tr><th className="row-number">#</th>{result.columns.map((column, index) => <th key={`${column}-${index}`}>{column}</th>)}</tr></thead>
                  <tbody>{result.rows.map((row, rowIndex) => <tr key={rowIndex}><td className="row-number">{rowIndex + 1}</td>{row.map((value, columnIndex) => <td key={columnIndex} title={formatCell(value)}>{formatCell(value)}</td>)}</tr>)}</tbody>
                </table>
              </div>
            )}
          </section>
        </main>

        <aside className="data-inspector-pane">
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
        </aside>
      </div>

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

function queryHistoryStorageKey(): string {
  const workspaceId = localStorage.getItem("drpa-active-workspace-id") ?? "personal";
  return `drpa-data-query-history:${workspaceId}`;
}

function quoteTableIdentifier(value: string, engine?: RemoteDatabaseProfile["engine"]): string {
  return value.split(".").map((part) => engine === "mysql"
    ? `\`${part.replaceAll("`", "``")}\``
    : `"${part.replaceAll('"', '""')}"`).join(".");
}

function createRemoteProfile(): RemoteDatabaseProfile {
  return {
    id: `database-${Date.now()}-${Math.random().toString(16).slice(2)}`,
    name: "",
    engine: "postgresql",
    host: "localhost",
    port: 5432,
    database: "",
    username: "",
    tlsMode: "prefer",
  };
}

function isFileDatabase(engine: RemoteDatabaseProfile["engine"]): engine is "sqlite" | "excel" {
  return engine === "sqlite" || engine === "excel";
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
