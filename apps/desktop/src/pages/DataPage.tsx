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
  Database,
  FolderOpen,
  KeyRound,
  LoaderCircle,
  Play,
  Plus,
  RefreshCw,
  Rows3,
  Table2,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

import { useAppStore } from "../app/store";
import type { DatabaseColumn, DatabaseInfo, DatabaseQueryResult, DatabaseTable } from "../domain/models";
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
  const editorRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const executingRef = useRef(false);

  const refreshSchema = useCallback(async () => {
    const [info, nextTables] = await Promise.all([
      desktopGateway.getWorkspaceDatabaseInfo(),
      desktopGateway.listDatabaseTables(),
    ]);
    setDatabase(info);
    setTables(nextTables);
  }, []);

  useEffect(() => {
    let active = true;
    setLoading(true);
    void refreshSchema()
      .catch((reason: unknown) => { if (active) setError(String(reason)); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [refreshSchema]);

  useEffect(() => {
    localStorage.setItem("drpa-data-query-history", JSON.stringify(history.slice(0, 30)));
  }, [history]);

  const inspectTable = async (table: DatabaseTable) => {
    setSelectedTable(table.name);
    setError("");
    try {
      setColumns(await desktopGateway.describeDatabaseTable(table.name));
    } catch (reason) {
      setColumns([]);
      setError(String(reason));
    }
  };

  const openTableQuery = (table: DatabaseTable) => {
    void inspectTable(table);
    setSql(`SELECT *\nFROM ${quoteIdentifier(table.name)}\nLIMIT 200;\n`);
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
      const nextResult = await desktopGateway.executeDatabaseSql(statement);
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

  return (
    <div className="page data-page">
      <header className="page-header data-header">
        <div>
          <div className="eyebrow">SQLite · SQL · RPA 数据</div>
          <h1>数据工作台</h1>
          <p>管理脚本共享数据、浏览结构并直接编写和执行 SQL。</p>
        </div>
        <div className="header-actions">
          <button className="button secondary" type="button" onClick={() => void desktopGateway.openWorkspaceDatabaseDirectory()}><FolderOpen size={15} /> 打开目录</button>
          <button className="button secondary" type="button" onClick={() => void refreshSchema()} disabled={loading}><RefreshCw size={15} /> 刷新结构</button>
          <button className="button primary" type="button" onClick={() => void runSql()} disabled={executing}>{executing ? <LoaderCircle className="spin" size={15} /> : <Play size={15} fill="currentColor" />} 执行 <kbd>Ctrl Enter</kbd></button>
        </div>
      </header>

      <div className="data-workspace">
        <aside className="data-schema-pane">
          <div className="data-pane-title"><Database size={15} /> 连接</div>
          <button className="database-connection active" type="button">
            <span className="database-icon"><Database size={16} /></span>
            <span><strong>{database?.name ?? "工作区数据库"}</strong><small>{database?.engine ?? "SQLite"} · {formatBytes(database?.sizeBytes ?? 0)}</small></span>
            <span className="connection-state" title="连接正常" />
          </button>
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
          <div className="database-path" title={database?.path}><span>文件</span><code>{database?.path ?? "正在定位…"}</code></div>
        </aside>

        <main className="data-query-pane">
          <div className="query-tabs">
            <button className="active" type="button"><span className="query-dot" /> 查询 1</button>
            <button type="button" aria-label="新建查询" onClick={() => { setSql(""); setResult(null); setError(""); }}><Plus size={14} /></button>
            <span>SQLite</span>
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
    </div>
  );
}

function loadQueryHistory(): QueryHistoryEntry[] {
  try {
    const value = JSON.parse(localStorage.getItem("drpa-data-query-history") ?? "[]");
    return Array.isArray(value) ? value.slice(0, 30) : [];
  } catch {
    return [];
  }
}

function quoteIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
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
