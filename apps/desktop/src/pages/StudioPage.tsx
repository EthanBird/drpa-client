import Editor, { loader } from "@monaco-editor/react";
import * as monaco from "monaco-editor/esm/vs/editor/editor.api.js";
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker.js?worker";
import "monaco-editor/esm/vs/basic-languages/python/python.contribution.js";
import "monaco-editor/esm/vs/basic-languages/yaml/yaml.contribution.js";
import {
  BookOpen,
  Box,
  Code2,
  FileCode2,
  FolderInput,
  FolderTree,
  PackageCheck,
  Play,
  Plus,
  RefreshCw,
  Save,
  TerminalSquare,
  Trash2,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { useAppStore } from "../app/store";
import type { StudioProject, StudioVariable } from "../domain/models";
import { desktopGateway } from "../infra/gateway";

self.MonacoEnvironment = { getWorker: () => new EditorWorker() };
loader.config({ monaco });

interface NotebookCell {
  cell_type: "code" | "markdown";
  execution_count: number | null;
  metadata: Record<string, unknown>;
  outputs: Array<Record<string, unknown>>;
  source: string | string[];
}

interface NotebookDocument {
  cells: NotebookCell[];
  metadata: Record<string, unknown>;
  nbformat: 4;
  nbformat_minor: number;
}

export function StudioPage() {
  const snapshot = useAppStore((state) => state.snapshot);
  const setSnapshot = useAppStore((state) => state.setSnapshot);
  const [projects, setProjects] = useState<StudioProject[]>([]);
  const [selectedId, setSelectedId] = useState("");
  const [selectedFile, setSelectedFile] = useState("main.py");
  const [content, setContent] = useState("");
  const [projectName, setProjectName] = useState("我的自动化项目");
  const [installedPackageId, setInstalledPackageId] = useState("");
  const [runParameters, setRunParameters] = useState("{}");
  const [notice, setNotice] = useState("工作室已就绪");
  const [busy, setBusy] = useState(false);

  const selected = useMemo(() => projects.find((item) => item.id === selectedId), [projects, selectedId]);
  const packages = snapshot?.packages ?? [];

  const refresh = async () => {
    const next = await desktopGateway.listStudioProjects();
    setProjects(next);
    if (!selectedId && next[0]) setSelectedId(next[0].id);
  };

  useEffect(() => { void refresh(); }, []);
  useEffect(() => {
    if (!installedPackageId && packages[0]) setInstalledPackageId(packages[0].id);
  }, [installedPackageId, packages]);
  useEffect(() => {
    if (!selectedId || !selectedFile) return;
    void desktopGateway.readProjectFile(selectedId, selectedFile).then(setContent).catch((error: unknown) => setNotice(String(error)));
  }, [selectedFile, selectedId]);

  const createProject = async () => {
    if (!projectName.trim()) return setNotice("请输入项目名称");
    setBusy(true);
    try {
      const project = await desktopGateway.createStudioProject(projectName.trim());
      await refresh();
      setSelectedId(project.id);
      setSelectedFile("main.py");
      setNotice(`已创建项目：${project.name}，内部 ID 已自动生成`);
    } catch (error) {
      setNotice(`创建失败：${String(error)}`);
    } finally { setBusy(false); }
  };

  const openInstalled = async () => {
    if (!installedPackageId) return;
    setBusy(true);
    try {
      const project = await desktopGateway.openInstalledPackage(installedPackageId);
      await refresh();
      setSelectedId(project.id);
      setSelectedFile("main.py");
      setNotice(`已从已安装脚本包创建可编辑工作副本：${project.name}`);
    } catch (error) {
      setNotice(`打开失败：${String(error)}`);
    } finally { setBusy(false); }
  };

  const save = async () => {
    if (!selectedId) return;
    setBusy(true);
    try {
      await desktopGateway.writeProjectFile(selectedId, selectedFile, content);
      setNotice(`已保存 ${selectedFile}`);
    } catch (error) { setNotice(`保存失败：${String(error)}`); }
    finally { setBusy(false); }
  };

  const runProject = async () => {
    if (!selectedId) return;
    setBusy(true);
    try {
      const parameters = JSON.parse(runParameters) as Record<string, unknown>;
      await desktopGateway.writeProjectFile(selectedId, selectedFile, content);
      const runId = await desktopGateway.runStudioProject(selectedId, parameters);
      setSnapshot(await desktopGateway.getWorkspaceSnapshot());
      setNotice(`开发态运行完成：${runId}（没有构建或安装）`);
    } catch (error) { setNotice(`运行失败：${String(error)}`); }
    finally { setBusy(false); }
  };

  const exportProject = async () => {
    if (!selectedId) return;
    setBusy(true);
    try {
      await desktopGateway.writeProjectFile(selectedId, selectedFile, content);
      const archivePath = await desktopGateway.buildStudioProject(selectedId);
      setNotice(`RPAZ 已导出：${archivePath}`);
    } catch (error) { setNotice(`导出失败：${String(error)}`); }
    finally { setBusy(false); }
  };

  const language = selectedFile.endsWith(".py") ? "python" : selectedFile.endsWith(".yaml") ? "yaml" : selectedFile.endsWith(".json") ? "json" : "plaintext";
  const notebook = selectedFile.endsWith(".ipynb");

  return (
    <div className="page studio-page">
      <header className="page-header studio-header">
        <div><div className="eyebrow">RPaz + Notebook 集成开发环境</div><h1>开发工作室</h1><p>编辑源码、直接运行项目，并使用持久 Python Kernel 交互调试。</p></div>
        <div className="header-actions">
          <button className="button secondary" type="button" onClick={save} disabled={!selectedId || busy}><Save size={15} /> 保存</button>
          <button className="button secondary" type="button" onClick={exportProject} disabled={!selectedId || busy}><PackageCheck size={15} /> 导出 RPAZ</button>
          <button className="button primary" type="button" onClick={runProject} disabled={!selectedId || busy}><Play size={15} fill="currentColor" /> {busy ? "处理中…" : "直接运行"}</button>
        </div>
      </header>
      <div className="studio-create-bar">
        <input value={projectName} onChange={(event) => setProjectName(event.target.value)} aria-label="项目名称" placeholder="只需输入项目名称" />
        <button className="button secondary" type="button" onClick={createProject} disabled={busy}><Plus size={15} /> 新建项目</button>
        <span className="studio-toolbar-divider" />
        <select aria-label="已安装脚本包" value={installedPackageId} onChange={(event) => setInstalledPackageId(event.target.value)} disabled={packages.length === 0}>
          {packages.length === 0 && <option value="">没有已安装脚本包</option>}
          {packages.map((item) => <option key={item.id} value={item.id}>{item.name} · {item.version}</option>)}
        </select>
        <button className="button secondary" type="button" onClick={openInstalled} disabled={!installedPackageId || busy}><FolderInput size={15} /> 打开已安装包</button>
        <details className="studio-run-config"><summary>运行参数</summary><textarea aria-label="Studio 运行参数 JSON" value={runParameters} onChange={(event) => setRunParameters(event.target.value)} /></details>
      </div>
      <div className={notebook ? "studio-layout notebook-active" : "studio-layout"}>
        <aside className="studio-projects">
          <div className="studio-pane-title"><FolderTree size={15} /> 项目</div>
          {projects.length === 0 && <p className="empty-hint">尚无项目，请在上方新建。</p>}
          {projects.map((project) => (
            <button type="button" key={project.id} className={project.id === selectedId ? "selected" : ""} onClick={() => { setSelectedId(project.id); setSelectedFile(project.files.includes("main.py") ? "main.py" : project.files[0] ?? "main.py"); }}>
              <Box size={14} /><span><strong>{project.name}</strong><small>内部 ID · {project.id.slice(-8)}</small></span>
            </button>
          ))}
        </aside>
        <aside className="studio-files">
          <div className="studio-pane-title"><Code2 size={15} /> 文件</div>
          {selected?.files.map((file) => <button type="button" key={file} className={file === selectedFile ? "selected" : ""} onClick={() => setSelectedFile(file)}>{file.endsWith(".ipynb") ? <BookOpen size={14} /> : <FileCode2 size={14} />} {file}</button>)}
        </aside>
        <section className={notebook ? "studio-editor notebook-editor" : "studio-editor"}>
          {notebook ? (
            <NotebookWorkspace projectId={selectedId} content={content} onChange={setContent} onNotice={setNotice} />
          ) : (
            <><div className="editor-tab"><FileCode2 size={14} /> {selectedFile || "未选择文件"}<span>{language}</span></div><Editor height="100%" language={language} value={content} onChange={(value) => setContent(value ?? "")} theme="vs-dark" options={{ fontSize: 14, minimap: { enabled: false }, automaticLayout: true, tabSize: 4, wordWrap: "on" }} /></>
          )}
        </section>
        <footer className="studio-console"><TerminalSquare size={15} /><strong>任务输出</strong><span>{notice}</span></footer>
      </div>
    </div>
  );
}

function NotebookWorkspace({ projectId, content, onChange, onNotice }: { projectId: string; content: string; onChange: (value: string) => void; onNotice: (value: string) => void }) {
  const [document, setDocument] = useState<NotebookDocument>(() => parseNotebook(content));
  const [executing, setExecuting] = useState<number | null>(null);
  const [variables, setVariables] = useState<StudioVariable[]>([]);

  useEffect(() => { setDocument(parseNotebook(content)); }, [content]);

  const commit = (next: NotebookDocument, persist = false) => {
    const serialized = JSON.stringify(next, null, 2) + "\n";
    setDocument(next);
    onChange(serialized);
    if (persist && projectId) void desktopGateway.writeProjectFile(projectId, "notebook.ipynb", serialized);
  };

  const updateSource = (index: number, source: string) => {
    const next = structuredClone(document);
    next.cells[index].source = source;
    commit(next);
  };

  const execute = async (sourceDocument: NotebookDocument, index: number) => {
    const cell = sourceDocument.cells[index];
    if (!cell || cell.cell_type !== "code") return sourceDocument;
    const result = await desktopGateway.executeStudioCell(projectId, sourceText(cell.source));
    const next = structuredClone(sourceDocument);
    const outputs: Array<Record<string, unknown>> = [];
    if (result.stdout) outputs.push({ output_type: "stream", name: "stdout", text: result.stdout });
    if (result.stderr) outputs.push({ output_type: "stream", name: "stderr", text: result.stderr });
    if (result.result !== undefined) outputs.push({ output_type: "execute_result", execution_count: result.executionCount, data: { "text/plain": result.result }, metadata: {} });
    if (result.error) outputs.push({ output_type: "error", ename: result.error.split(":")[0], evalue: result.error, traceback: result.traceback });
    next.cells[index].execution_count = result.executionCount;
    next.cells[index].outputs = outputs;
    setVariables(result.variables);
    onNotice(result.error ? `单元格执行失败：${result.error}` : `单元格 [${result.executionCount}] 完成 · ${result.durationMs} ms`);
    return next;
  };

  const runCell = async (index: number) => {
    if (!projectId || executing !== null) return;
    setExecuting(index);
    try { commit(await execute(document, index), true); }
    catch (error) { onNotice(`Kernel 执行失败：${String(error)}`); }
    finally { setExecuting(null); }
  };

  const runAll = async () => {
    if (!projectId || executing !== null) return;
    setExecuting(-1);
    try {
      let next = document;
      for (let index = 0; index < next.cells.length; index += 1) next = await execute(next, index);
      commit(next, true);
      onNotice("全部代码单元格执行完成");
    } catch (error) { onNotice(`Kernel 执行失败：${String(error)}`); }
    finally { setExecuting(null); }
  };

  const addCell = (kind: "code" | "markdown") => {
    const next = structuredClone(document);
    next.cells.push({ cell_type: kind, execution_count: null, metadata: {}, outputs: [], source: "" });
    commit(next);
  };

  const deleteCell = (index: number) => {
    const next = structuredClone(document);
    next.cells.splice(index, 1);
    commit(next);
  };

  const restart = async () => {
    await desktopGateway.restartStudioKernel(projectId);
    setVariables([]);
    onNotice("Python Kernel 已重启，内存变量已清空");
  };

  return (
    <div className="notebook-workspace">
      <div className="notebook-toolbar">
        <span><BookOpen size={15} /> notebook.ipynb</span>
        <button type="button" onClick={() => addCell("code")}><Plus size={13} /> 代码</button>
        <button type="button" onClick={() => addCell("markdown")}><Plus size={13} /> Markdown</button>
        <button type="button" onClick={runAll} disabled={executing !== null}><Play size={13} /> 全部运行</button>
        <button type="button" onClick={restart} disabled={!projectId || executing !== null}><RefreshCw size={13} /> 重启 Kernel</button>
        <em>DRPA Python 3.11 · sealed</em>
      </div>
      <div className="notebook-scroll">
        {document.cells.map((cell, index) => (
          <article className="notebook-cell" key={index}>
            <div className="cell-gutter"><button type="button" aria-label={`运行单元格 ${index + 1}`} onClick={() => runCell(index)} disabled={cell.cell_type !== "code" || executing !== null}><Play size={13} fill="currentColor" /></button><span>[{cell.execution_count ?? " "}]</span></div>
            <div className="cell-body">
              {cell.cell_type === "code" ? (
                <Editor height={`${Math.max(92, sourceText(cell.source).split("\n").length * 20 + 30)}px`} language="python" value={sourceText(cell.source)} onChange={(value) => updateSource(index, value ?? "")} theme="vs-dark" options={{ fontSize: 13, minimap: { enabled: false }, automaticLayout: true, lineNumbers: "on", scrollBeyondLastLine: false, folding: false }} />
              ) : <textarea className="markdown-cell" value={sourceText(cell.source)} onChange={(event) => updateSource(index, event.target.value)} placeholder="Markdown 说明…" />}
              {cell.outputs.length > 0 && <div className="cell-output">{cell.outputs.map((output, outputIndex) => <pre className={output.output_type === "error" ? "error" : ""} key={outputIndex}>{outputText(output)}</pre>)}</div>}
            </div>
            <button className="cell-delete" type="button" aria-label={`删除单元格 ${index + 1}`} onClick={() => deleteCell(index)}><Trash2 size={13} /></button>
          </article>
        ))}
        {document.cells.length === 0 && <div className="empty-state"><BookOpen size={26} /><h2>空 Notebook</h2><p>添加代码或 Markdown 单元格开始开发。</p></div>}
        {variables.length > 0 && <section className="variable-explorer"><header>变量资源管理器 <span>{variables.length}</span></header>{variables.map((variable) => <div key={variable.name}><strong>{variable.name}</strong><em>{variable.typeName}</em><code>{variable.preview}</code></div>)}</section>}
      </div>
    </div>
  );
}

function parseNotebook(content: string): NotebookDocument {
  try {
    const parsed = JSON.parse(content) as NotebookDocument;
    if (parsed.nbformat === 4 && Array.isArray(parsed.cells)) return parsed;
  } catch { /* show a recoverable empty notebook while the user edits invalid JSON elsewhere */ }
  return { cells: [], metadata: {}, nbformat: 4, nbformat_minor: 5 };
}

function sourceText(source: string | string[]): string {
  return Array.isArray(source) ? source.join("") : source;
}

function outputText(output: Record<string, unknown>): string {
  if (typeof output.text === "string") return output.text;
  if (Array.isArray(output.text)) return output.text.join("");
  if (Array.isArray(output.traceback)) return output.traceback.join("");
  const data = output.data as Record<string, unknown> | undefined;
  const plainText = data?.["text/plain"];
  return typeof plainText === "string" ? plainText : "";
}
