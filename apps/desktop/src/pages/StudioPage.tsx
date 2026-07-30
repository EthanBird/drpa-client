import Editor, { loader } from "@monaco-editor/react";
import { open } from "@tauri-apps/plugin-dialog";
import * as monaco from "monaco-editor/esm/vs/editor/editor.api.js";
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker.js?worker";
import "monaco-editor/esm/vs/basic-languages/python/python.contribution.js";
import "monaco-editor/esm/vs/basic-languages/yaml/yaml.contribution.js";
import {
  BookOpen,
  Box,
  Code2,
  FileCode2,
  FilePlus2,
  Folder,
  FolderInput,
  FolderPlus,
  FolderTree,
  LoaderCircle,
  PackageCheck,
  PanelRightClose,
  PanelRightOpen,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  Save,
  TerminalSquare,
  Trash2,
  Upload,
} from "lucide-react";
import type { DragEvent } from "react";
import { useCallback, useEffect, useMemo, useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { useAppStore } from "../app/store";
import { SidebarToggle, useSidebarCollapsed } from "../components/SidebarToggle";
import type { StudioProject, StudioVariable } from "../domain/models";
import { desktopGateway } from "../infra/gateway";
import { AgentPage } from "./AgentPage";

self.MonacoEnvironment = { getWorker: () => new EditorWorker() };
loader.config({ monaco });

let pythonCompletionRegistered = false;

function ensurePythonCompletionProvider() {
  if (pythonCompletionRegistered || !monaco.languages?.registerCompletionItemProvider) return;
  pythonCompletionRegistered = true;
  monaco.languages.registerCompletionItemProvider("python", {
    triggerCharacters: [".", "_"],
    provideCompletionItems: async (model, position, context, token) => {
      if (model.uri.scheme !== "drpa-python") return { suggestions: [] };
      const projectId = decodeURIComponent(model.uri.path.split("/").filter(Boolean)[0] ?? "");
      if (!projectId) return { suggestions: [] };
      const code = model.getValue();
      const utf16Cursor = model.getOffsetAt(position);
      const cursorPos = Array.from(code.slice(0, utf16Cursor)).length;
      try {
        const result = await desktopGateway.completeStudioPython(projectId, code, cursorPos);
        if (token.isCancellationRequested || result.status !== "ok") return { suggestions: [] };
        const utf16Start = codePointOffsetToUtf16(code, result.cursorStart);
        const utf16End = codePointOffsetToUtf16(code, result.cursorEnd);
        const start = model.getPositionAt(utf16Start);
        const end = model.getPositionAt(utf16End);
        const range = new monaco.Range(start.lineNumber, start.column, end.lineNumber, end.column);
        const typeMetadata = completionTypeMetadata(result.metadata);
        const suggestions = [...new Set(result.matches)].slice(0, 400).map((match, index) => ({
          label: match,
          kind: completionKind(match, context.triggerCharacter, typeMetadata.get(match)?.type),
          insertText: match,
          range,
          detail: typeMetadata.get(match)?.type,
          documentation: typeMetadata.get(match)?.signature || undefined,
          sortText: String(index).padStart(4, "0"),
        }));
        return { suggestions };
      } catch {
        return { suggestions: [] };
      }
    },
  });
  if (monaco.languages.registerHoverProvider) {
    monaco.languages.registerHoverProvider("python", {
      provideHover: async (model, position, token) => {
        const target = pythonInspectionTarget(model, position);
        if (!target) return null;
        try {
          const result = await desktopGateway.inspectStudioPython(target.projectId, target.code, target.cursorPos, 0);
          const text = inspectionText(result.data);
          if (token.isCancellationRequested || result.status !== "ok" || !result.found || !text) return null;
          const word = model.getWordAtPosition(position);
          return {
            contents: [{ value: `\`\`\`text\n${text.replaceAll("```", "'''")}\n\`\`\`` }],
            range: word ? new monaco.Range(position.lineNumber, word.startColumn, position.lineNumber, word.endColumn) : undefined,
          };
        } catch {
          return null;
        }
      },
    });
  }
  if (monaco.languages.registerSignatureHelpProvider) {
    monaco.languages.registerSignatureHelpProvider("python", {
      signatureHelpTriggerCharacters: ["(", ","],
      signatureHelpRetriggerCharacters: [","],
      provideSignatureHelp: async (model, position, token) => {
        const target = pythonInspectionTarget(model, position);
        if (!target) return null;
        try {
          const result = await desktopGateway.inspectStudioPython(target.projectId, target.code, target.cursorPos, 0);
          const text = inspectionText(result.data);
          const parsed = parsePythonSignature(text);
          if (token.isCancellationRequested || result.status !== "ok" || !result.found || !parsed) return null;
          return {
            value: {
              signatures: [{
                label: parsed.label,
                documentation: parsed.documentation,
                parameters: parsed.parameters.map((label) => ({ label })),
              }],
              activeSignature: 0,
              activeParameter: Math.min(activeCallParameter(target.code, target.cursorPos), Math.max(0, parsed.parameters.length - 1)),
            },
            dispose: () => undefined,
          };
        } catch {
          return null;
        }
      },
    });
  }
}

function pythonInspectionTarget(model: monaco.editor.ITextModel, position: monaco.Position) {
  if (model.uri.scheme !== "drpa-python") return null;
  const projectId = decodeURIComponent(model.uri.path.split("/").filter(Boolean)[0] ?? "");
  if (!projectId) return null;
  const code = model.getValue();
  const utf16Cursor = model.getOffsetAt(position);
  return { projectId, code, cursorPos: Array.from(code.slice(0, utf16Cursor)).length };
}

function inspectionText(data: Record<string, string>): string {
  return (data["text/plain"] ?? Object.values(data)[0] ?? "")
    .replace(/\u001b\[[0-9;]*m/g, "")
    .trim()
    .slice(0, 60_000);
}

export function parsePythonSignature(text: string): { label: string; parameters: string[]; documentation: string } | null {
  const clean = text.replace(/\u001b\[[0-9;]*m/g, "").trim();
  const signatureLine = clean.split(/\r?\n/).find((line) => /^Signature:\s*.+\(.*\)/.test(line.trim()))?.trim();
  if (!signatureLine) return null;
  const label = signatureLine.replace(/^Signature:\s*/, "");
  const opening = label.indexOf("(");
  const closing = label.lastIndexOf(")");
  const parameters = opening >= 0 && closing > opening
    ? splitPythonParameters(label.slice(opening + 1, closing)).filter((parameter) => parameter !== "/" && parameter !== "*")
    : [];
  const documentation = clean.split(/\r?\n/).filter((line) => !line.trim().startsWith("Signature:")).join("\n").trim();
  return { label, parameters, documentation };
}

function splitPythonParameters(value: string): string[] {
  const parameters: string[] = [];
  let start = 0;
  let depth = 0;
  let quote = "";
  for (let index = 0; index < value.length; index += 1) {
    const character = value[index];
    if (quote) {
      if (character === quote && value[index - 1] !== "\\") quote = "";
      continue;
    }
    if (character === "'" || character === '"') { quote = character; continue; }
    if ("([{<".includes(character)) depth += 1;
    if (")]}>".includes(character)) depth = Math.max(0, depth - 1);
    if (character === "," && depth === 0) {
      parameters.push(value.slice(start, index).trim());
      start = index + 1;
    }
  }
  const final = value.slice(start).trim();
  if (final) parameters.push(final);
  return parameters;
}

function activeCallParameter(code: string, cursorPos: number): number {
  const prefix = Array.from(code).slice(0, cursorPos).join("");
  let depth = 0;
  let commas = 0;
  for (let index = prefix.length - 1; index >= 0; index -= 1) {
    const character = prefix[index];
    if (character === ")") depth += 1;
    else if (character === "(") {
      if (depth === 0) return commas;
      depth -= 1;
    } else if (character === "," && depth === 0) commas += 1;
  }
  return 0;
}

function codePointOffsetToUtf16(value: string, offset: number): number {
  return Array.from(value).slice(0, Math.max(0, offset)).join("").length;
}

function completionKind(match: string, triggerCharacter?: string, typeName?: string): monaco.languages.CompletionItemKind {
  const normalized = typeName?.toLowerCase();
  if (normalized === "function" || normalized === "method") return monaco.languages.CompletionItemKind.Function;
  if (normalized === "class" || normalized === "type") return monaco.languages.CompletionItemKind.Class;
  if (normalized === "module") return monaco.languages.CompletionItemKind.Module;
  if (normalized === "keyword") return monaco.languages.CompletionItemKind.Keyword;
  if (normalized === "property" || normalized === "field" || normalized === "statement") return monaco.languages.CompletionItemKind.Property;
  if (triggerCharacter === "." || /^[a-z_][a-zA-Z0-9_]*$/.test(match)) {
    return monaco.languages.CompletionItemKind.Property;
  }
  return monaco.languages.CompletionItemKind.Variable;
}

function completionTypeMetadata(metadata: unknown): Map<string, { type: string; signature: string }> {
  if (!metadata || typeof metadata !== "object") return new Map();
  const raw = (metadata as Record<string, unknown>)._jupyter_types_experimental;
  if (!Array.isArray(raw)) return new Map();
  const entries = raw.flatMap((item) => {
    if (!item || typeof item !== "object") return [];
    const value = item as Record<string, unknown>;
    if (typeof value.text !== "string") return [];
    return [[value.text, {
      type: typeof value.type === "string" ? value.type : "",
      signature: typeof value.signature === "string" ? value.signature : "",
    }] as const];
  });
  return new Map(entries);
}

function pythonModelPath(projectId: string, filePath: string): string {
  return `drpa-python://studio/${encodeURIComponent(projectId)}/${encodeURIComponent(filePath)}`;
}

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

interface FileContextMenu {
  x: number;
  y: number;
  target?: string;
}

interface ProjectContextMenu {
  x: number;
  y: number;
  project: StudioProject;
}

type InlineDraft =
  | { mode: "create"; kind: "file" | "folder"; value: string }
  | { mode: "rename"; source: string; value: string };

type PendingDelete =
  | { kind: "project"; project: StudioProject }
  | { kind: "entry"; path: string };

export function StudioPage() {
  const projectsCollapsed = useSidebarCollapsed("studio-projects");
  const filesCollapsed = useSidebarCollapsed("studio-files");
  const snapshot = useAppStore((state) => state.snapshot);
  const setSnapshot = useAppStore((state) => state.setSnapshot);
  const theme = useAppStore((state) => state.theme);
  const [projects, setProjects] = useState<StudioProject[]>([]);
  const [selectedId, setSelectedId] = useState("");
  const [selectedFile, setSelectedFile] = useState("main.py");
  const [selectedEntry, setSelectedEntry] = useState("main.py");
  const [content, setContent] = useState("");
  const [projectName, setProjectName] = useState("我的自动化项目");
  const [installedPackageId, setInstalledPackageId] = useState("");
  const [runParameters, setRunParameters] = useState("{}");
  const [notice, setNotice] = useState("工作室已就绪");
  const [busy, setBusy] = useState(false);
  const [fileMenu, setFileMenu] = useState<FileContextMenu | null>(null);
  const [projectMenu, setProjectMenu] = useState<ProjectContextMenu | null>(null);
  const [inlineDraft, setInlineDraft] = useState<InlineDraft | null>(null);
  const [pendingDelete, setPendingDelete] = useState<PendingDelete | null>(null);
  const [agentOpen, setAgentOpen] = useState(true);

  const selected = useMemo(() => projects.find((item) => item.id === selectedId), [projects, selectedId]);
  const packages = snapshot?.packages ?? [];
  const files = selected?.files ?? [];
  const normalizedEntry = selectedEntry.replace(/\/$/, "");
  const currentDirectory = selectedEntry.endsWith("/")
    ? normalizedEntry
    : normalizedEntry.includes("/")
      ? normalizedEntry.slice(0, normalizedEntry.lastIndexOf("/"))
      : "";

  const pickInitialFile = (project: StudioProject | undefined) => {
    if (!project) return "main.py";
    if (project.files.includes("main.py")) return "main.py";
    return project.files.find((file) => !file.endsWith("/")) ?? "main.py";
  };

  const refresh = async (preferredFile?: string, preferredProjectId = selectedId) => {
    const next = await desktopGateway.listStudioProjects();
    setProjects(next);
    const project = next.find((item) => item.id === preferredProjectId) ?? next[0];
    if (project) {
      setSelectedId(project.id);
      const nextFile = preferredFile && project.files.includes(preferredFile) ? preferredFile : pickInitialFile(project);
      setSelectedFile(nextFile);
      setSelectedEntry(nextFile);
    } else {
      setSelectedId("");
      setSelectedFile("");
      setSelectedEntry("");
      setContent("");
    }
  };

  useEffect(() => { void refresh(); }, []);
  useEffect(() => {
    if (!installedPackageId && packages[0]) setInstalledPackageId(packages[0].id);
  }, [installedPackageId, packages]);
  useEffect(() => {
    if (!selectedId || !selectedFile || selectedFile.endsWith("/")) return;
    void desktopGateway.readProjectFile(selectedId, selectedFile).then(setContent).catch((error: unknown) => setNotice(String(error)));
  }, [selectedFile, selectedId]);

  const createProject = async () => {
    if (!projectName.trim()) return setNotice("请输入项目名称");
    setBusy(true);
    try {
      const project = await desktopGateway.createStudioProject(projectName.trim());
      await refresh("main.py", project.id);
      setNotice(`已创建项目：${project.name}，内部 ID 已自动生成`);
    } catch (error) {
      setNotice(`失败：${String(error)}`);
    } finally { setBusy(false); }
  };

  const openInstalled = async () => {
    if (!installedPackageId) return;
    setBusy(true);
    try {
      const project = await desktopGateway.openInstalledPackage(installedPackageId);
      await refresh("main.py", project.id);
      setNotice(`已从已安装 RPAZ 包创建可编辑工作副本：${project.name}`);
    } catch (error) {
      setNotice(`打开失败：${String(error)}`);
    } finally { setBusy(false); }
  };

  const save = async () => {
    if (!selectedId || !selectedFile || selectedFile.endsWith("/")) return;
    setBusy(true);
    try {
      await desktopGateway.writeProjectFile(selectedId, selectedFile, content);
      await refresh(selectedFile);
      setNotice(`已保存 ${selectedFile}`);
    } catch (error) { setNotice(`失败：${String(error)}`); }
    finally { setBusy(false); }
  };

  const startCreate = (kind: "file" | "folder") => {
    if (!selectedId) return;
    setFileMenu(null);
    setInlineDraft({ mode: "create", kind, value: kind === "file" ? "new_file.py" : "new_folder" });
  };

  const startRename = (source: string) => {
    const normalized = source.replace(/\/$/, "");
    setFileMenu(null);
    setSelectedEntry(source);
    setInlineDraft({ mode: "rename", source, value: normalized.split("/").pop() ?? normalized });
  };

  const commitInlineDraft = async () => {
    if (!selectedId || !inlineDraft) return;
    const name = inlineDraft.value.trim();
    if (!name || name === "." || name === ".." || /[\\/]/.test(name)) {
      setNotice("名称不能为空，也不能包含路径分隔符");
      return;
    }
    setBusy(true);
    try {
      if (inlineDraft.mode === "create") {
        const relativePath = currentDirectory ? `${currentDirectory}/${name}` : name;
        if (inlineDraft.kind === "file") {
          await desktopGateway.writeProjectFile(selectedId, relativePath, "");
          await refresh(relativePath);
          setNotice(`已新建文件 ${relativePath}`);
        } else {
          await desktopGateway.createProjectDirectory(selectedId, relativePath);
          await refresh(selectedFile);
          setSelectedEntry(`${relativePath}/`);
          setNotice(`已新建文件夹 ${relativePath}`);
        }
      } else {
        const source = inlineDraft.source.replace(/\/$/, "");
        const parent = source.includes("/") ? source.slice(0, source.lastIndexOf("/")) : "";
        const target = parent ? `${parent}/${name}` : name;
        await desktopGateway.renameProjectEntry(selectedId, source, target);
        const directory = inlineDraft.source.endsWith("/");
        const nextFile = directory && selectedFile.startsWith(`${source}/`)
          ? `${target}${selectedFile.slice(source.length)}`
          : selectedFile === source ? target : selectedFile;
        await refresh(nextFile);
        setSelectedEntry(directory ? `${target}/` : target);
        setNotice(`已重命名为 ${target}`);
      }
      setInlineDraft(null);
    } catch (error) {
      setNotice(`文件操作失败：${String(error)}`);
    } finally { setBusy(false); }
  };

  const confirmDelete = async () => {
    if (!pendingDelete) return;
    setBusy(true);
    try {
      if (pendingDelete.kind === "project") {
        await desktopGateway.deleteStudioProject(pendingDelete.project.id);
        await refresh(undefined, "");
        setNotice(`已删除开发项目：${pendingDelete.project.name}`);
      } else if (selectedId) {
        await desktopGateway.deleteProjectEntry(selectedId, pendingDelete.path.replace(/\/$/, ""));
        await refresh();
        setNotice(`已删除 ${pendingDelete.path}`);
      }
      setPendingDelete(null);
    } catch (error) {
      setNotice(`删除失败：${String(error)}`);
    } finally { setBusy(false); }
  };

  const importPaths = useCallback(async (paths: string[]) => {
    if (!selectedId || paths.length === 0) return;
    setFileMenu(null);
    setBusy(true);
    try {
      let lastImported = selectedFile;
      for (const sourcePath of paths) {
        lastImported = await desktopGateway.importProjectFile(selectedId, sourcePath, currentDirectory);
      }
      await refresh(lastImported);
      setNotice(`已导入 ${paths.length} 个文件${currentDirectory ? ` 到 ${currentDirectory}` : ""}`);
    } catch (error) { setNotice(`导入失败：${String(error)}`); }
    finally { setBusy(false); }
  }, [currentDirectory, selectedFile, selectedId]);

  useEffect(() => {
    const handleNativeDrop = (event: Event) => {
      const paths = (event as CustomEvent<{ paths?: string[] }>).detail?.paths ?? [];
      void importPaths(paths);
    };
    window.addEventListener("drpa-studio-file-drop", handleNativeDrop);
    return () => window.removeEventListener("drpa-studio-file-drop", handleNativeDrop);
  }, [importPaths]);

  const pickAndImport = async () => {
    if (!selectedId || !("__TAURI_INTERNALS__" in window)) return;
    const selectedPaths = await open({ multiple: true });
    if (!selectedPaths) return;
    await importPaths(Array.isArray(selectedPaths) ? selectedPaths : [selectedPaths]);
  };

  const dropBrowserFiles = async (event: DragEvent<HTMLElement>) => {
    event.preventDefault();
    if (!selectedId) return;
    const dropped = Array.from(event.dataTransfer.files);
    if (dropped.length === 0) return;
    setBusy(true);
    try {
      let lastFile = selectedFile;
      for (const file of dropped) {
        const relativePath = currentDirectory ? `${currentDirectory}/${file.name}` : file.name;
        await desktopGateway.writeProjectFile(selectedId, relativePath, await file.text());
        lastFile = relativePath;
      }
      await refresh(lastFile);
      setNotice(`已拖拽添加 ${dropped.length} 个文件${currentDirectory ? ` 到 ${currentDirectory}` : ""}`);
    } catch (error) { setNotice(`拖拽添加失败：${String(error)}`); }
    finally { setBusy(false); }
  };

  const runProject = async () => {
    if (!selectedId) return;
    setBusy(true);
    try {
      const parameters = JSON.parse(runParameters) as Record<string, unknown>;
      if (selectedFile && !selectedFile.endsWith("/")) await desktopGateway.writeProjectFile(selectedId, selectedFile, content);
      const runId = await desktopGateway.runStudioProject(selectedId, parameters);
      setSnapshot(await desktopGateway.getWorkspaceSnapshot());
      setNotice(`开发态运行已启动：${runId}（没有构建或安装，可在运行工作台查看实时日志）`);
    } catch (error) { setNotice(`失败：${String(error)}`); }
    finally { setBusy(false); }
  };

  const exportProject = async () => {
    if (!selectedId) return;
    setBusy(true);
    try {
      if (selectedFile && !selectedFile.endsWith("/")) await desktopGateway.writeProjectFile(selectedId, selectedFile, content);
      const archivePath = await desktopGateway.buildStudioProject(selectedId);
      try {
        await desktopGateway.openBuildOutputDirectory();
        setNotice(`RPAZ 已导出并打开所在目录：${archivePath}`);
      } catch (openError) {
        setNotice(`RPAZ 已导出：${archivePath}；打开目录失败：${String(openError)}`);
      }
    } catch (error) { setNotice(`失败：${String(error)}`); }
    finally { setBusy(false); }
  };

  const saveToPackageLibrary = async () => {
    if (!selectedId) return;
    setBusy(true);
    try {
      if (selectedFile && !selectedFile.endsWith("/")) {
        await desktopGateway.writeProjectFile(selectedId, selectedFile, content);
      }
      const installed = await desktopGateway.installStudioProject(selectedId);
      setSnapshot(await desktopGateway.getWorkspaceSnapshot());
      setInstalledPackageId(installed.id);
      setNotice(`已保存到 RPAZ 包库：${installed.name} · v${installed.version}`);
    } catch (error) {
      setNotice(`保存到 RPAZ 包失败：${String(error)}`);
    } finally {
      setBusy(false);
    }
  };

  const language = selectedFile.endsWith(".py") ? "python" : selectedFile.endsWith(".yaml") ? "yaml" : selectedFile.endsWith(".json") ? "json" : "plaintext";
  const notebook = selectedFile.endsWith(".ipynb");

  return (
    <div className="page studio-page" onClick={() => { setFileMenu(null); setProjectMenu(null); }}>
      <header className="page-header studio-header">
        <div><div className="eyebrow">RPaz + Notebook 集成开发环境</div><h1>开发工作室</h1><p>编辑源码、直接运行项目，并使用持久 Python Kernel 交互调试。</p></div>
        <div className="header-actions">
          <button className="button secondary" type="button" onClick={() => setAgentOpen((current) => !current)} aria-pressed={agentOpen}>{agentOpen ? <PanelRightClose size={15} /> : <PanelRightOpen size={15} />} {agentOpen ? "收起 Agent" : "打开 Agent"}</button>
          <button className="button secondary" type="button" onClick={save} disabled={!selectedId || busy || selectedFile.endsWith("/")}><Save size={15} /> 保存</button>
          <button className="button secondary" type="button" onClick={() => void saveToPackageLibrary()} disabled={!selectedId || busy}><PackageCheck size={15} /> 保存到 RPAZ 包</button>
          <button className="button secondary" type="button" onClick={exportProject} disabled={!selectedId || busy}><PackageCheck size={15} /> 导出 RPAZ</button>
          <button className="button primary" type="button" onClick={runProject} disabled={!selectedId || busy}><Play size={15} fill="currentColor" /> {busy ? "处理中…" : "直接运行"}</button>
        </div>
      </header>
      <div className="studio-create-bar">
        <input value={projectName} onChange={(event) => setProjectName(event.target.value)} aria-label="项目名称" placeholder="只需输入项目名称" />
        <button className="button secondary" type="button" onClick={createProject} disabled={busy}><Plus size={15} /> 新建项目</button>
        <span className="studio-toolbar-divider" />
        <select aria-label="已安装 RPAZ 包" value={installedPackageId} onChange={(event) => setInstalledPackageId(event.target.value)} disabled={packages.length === 0}>
          {packages.length === 0 && <option value="">没有已安装 RPAZ 包</option>}
          {packages.map((item) => <option key={item.id} value={item.id}>{item.name} · {item.version}</option>)}
        </select>
        <button className="button secondary" type="button" onClick={openInstalled} disabled={!installedPackageId || busy}><FolderInput size={15} /> 打开已安装包</button>
        <details className="studio-run-config"><summary>运行参数</summary><textarea aria-label="Studio 运行参数 JSON" value={runParameters} onChange={(event) => setRunParameters(event.target.value)} /></details>
      </div>
      <div className={`${notebook ? "studio-layout notebook-active" : "studio-layout"} ${agentOpen ? "agent-open" : ""}${projectsCollapsed ? " projects-collapsed" : ""}${filesCollapsed ? " files-collapsed" : ""}`}>
        {projectsCollapsed ? <SidebarToggle id="studio-projects" side="left" label="项目侧边栏" restore /> : <aside className="studio-projects collapsible-sidebar">
          <SidebarToggle id="studio-projects" side="left" label="项目侧边栏" />
          <div className="studio-pane-title"><FolderTree size={15} /> 项目</div>
          {projects.length === 0 && <p className="empty-hint">尚无项目，请在上方新建。</p>}
          {projects.map((project) => (
            <button
              type="button"
              key={project.id}
              className={project.id === selectedId ? "selected" : ""}
              onClick={() => { const file = pickInitialFile(project); setSelectedId(project.id); setSelectedFile(file); setSelectedEntry(file); }}
              onContextMenu={(event) => { event.preventDefault(); event.stopPropagation(); setSelectedId(project.id); setProjectMenu({ x: event.clientX, y: event.clientY, project }); }}
            >
              <Box size={14} /><span><strong>{project.name}</strong><small>内部 ID · {project.id.slice(-8)}</small></span>
            </button>
          ))}
          {projectMenu && (
            <div className="studio-context-menu" style={{ left: projectMenu.x, top: projectMenu.y }} onClick={(event) => event.stopPropagation()}>
              <button type="button" className="danger" onClick={() => { setPendingDelete({ kind: "project", project: projectMenu.project }); setProjectMenu(null); }}><Trash2 size={14} /> 删除开发项目</button>
            </div>
          )}
        </aside>}
        {filesCollapsed ? <SidebarToggle id="studio-files" side="left" label="文件侧边栏" restore /> : <aside
          className="studio-files collapsible-sidebar"
          onContextMenu={(event) => { event.preventDefault(); setFileMenu({ x: event.clientX, y: event.clientY }); }}
          onDragOver={(event) => event.preventDefault()}
          onDrop={dropBrowserFiles}
        >
          <SidebarToggle id="studio-files" side="left" label="文件侧边栏" />
          <div className="studio-pane-title studio-pane-title-actions">
            <span><Code2 size={15} /> 文件</span>
            <span className="studio-file-actions">
              <button type="button" title="新建文件" onClick={() => startCreate("file")} disabled={!selectedId || busy}><FilePlus2 size={13} /></button>
              <button type="button" title="新建文件夹" onClick={() => startCreate("folder")} disabled={!selectedId || busy}><FolderPlus size={13} /></button>
              <button type="button" title="导入文件" onClick={pickAndImport} disabled={!selectedId || busy || !("__TAURI_INTERNALS__" in window)}><Upload size={13} /></button>
            </span>
          </div>
          {files.length === 0 && !inlineDraft && <p className="empty-hint">当前项目还没有文件，可右键新建或拖拽导入。</p>}
          {inlineDraft?.mode === "create" && (
            <div className="studio-inline-entry">
              {inlineDraft.kind === "folder" ? <Folder size={14} /> : <FileCode2 size={14} />}
              {currentDirectory && <span>{currentDirectory}/</span>}
              <input
                autoFocus
                aria-label={inlineDraft.kind === "folder" ? "新文件夹名称" : "新文件名称"}
                value={inlineDraft.value}
                onChange={(event) => setInlineDraft({ ...inlineDraft, value: event.target.value })}
                onFocus={(event) => event.currentTarget.select()}
                onBlur={() => { if (!busy) setInlineDraft(null); }}
                onKeyDown={(event) => { if (event.key === "Enter") void commitInlineDraft(); if (event.key === "Escape") setInlineDraft(null); }}
              />
            </div>
          )}
          {files.map((file) => {
            const directory = file.endsWith("/");
            if (inlineDraft?.mode === "rename" && inlineDraft.source === file) {
              return (
                <div className="studio-inline-entry" key={file}>
                  {directory ? <Folder size={14} /> : <FileCode2 size={14} />}
                  <input
                    autoFocus
                    aria-label="重命名"
                    value={inlineDraft.value}
                    onChange={(event) => setInlineDraft({ ...inlineDraft, value: event.target.value })}
                    onFocus={(event) => event.currentTarget.select()}
                    onBlur={() => { if (!busy) setInlineDraft(null); }}
                    onKeyDown={(event) => { if (event.key === "Enter") void commitInlineDraft(); if (event.key === "Escape") setInlineDraft(null); }}
                  />
                </div>
              );
            }
            return (
              <button
                type="button"
                key={file}
                className={file === selectedEntry ? "selected" : ""}
                onClick={() => { setSelectedEntry(file); if (!directory) setSelectedFile(file); }}
                onDoubleClick={() => startRename(file)}
                onContextMenu={(event) => { event.preventDefault(); event.stopPropagation(); setSelectedEntry(file); setFileMenu({ x: event.clientX, y: event.clientY, target: file }); }}
              >
                {directory ? <Folder size={14} /> : file.endsWith(".ipynb") ? <BookOpen size={14} /> : <FileCode2 size={14} />} {file}
              </button>
            );
          })}
          {fileMenu && (
            <div className="studio-context-menu" style={{ left: fileMenu.x, top: fileMenu.y }} onClick={(event) => event.stopPropagation()}>
              <button type="button" onClick={() => startCreate("file")}><FilePlus2 size={14} /> 新建文件</button>
              <button type="button" onClick={() => startCreate("folder")}><FolderPlus size={14} /> 新建文件夹</button>
              <button type="button" onClick={() => fileMenu.target && startRename(fileMenu.target)} disabled={!fileMenu.target}><Pencil size={14} /> 重命名</button>
              <button type="button" onClick={pickAndImport} disabled={!("__TAURI_INTERNALS__" in window)}><Upload size={14} /> 导入文件</button>
              <button type="button" className="danger" onClick={() => { if (fileMenu.target) setPendingDelete({ kind: "entry", path: fileMenu.target }); setFileMenu(null); }} disabled={!fileMenu.target}><Trash2 size={14} /> 删除</button>
            </div>
          )}
        </aside>}
        <section className={notebook ? "studio-editor notebook-editor" : "studio-editor"}>
          {notebook ? (
            <NotebookWorkspace projectId={selectedId} content={content} onChange={setContent} onNotice={setNotice} theme={theme} />
          ) : (
            <><div className="editor-tab"><FileCode2 size={14} /> {selectedFile || "未选择文件"}<span>{language}</span></div><Editor beforeMount={ensurePythonCompletionProvider} path={language === "python" && selectedId ? pythonModelPath(selectedId, selectedFile) : undefined} height="100%" language={language} value={content} onChange={(value) => setContent(value ?? "")} theme={theme === "dark" ? "vs-dark" : "light"} options={{ fontSize: 14, minimap: { enabled: false }, automaticLayout: true, tabSize: 4, wordWrap: "on", quickSuggestions: { other: true, comments: false, strings: false }, suggestOnTriggerCharacters: true }} /></>
          )}
        </section>
        {agentOpen && <aside className="studio-agent-pane"><AgentPage embedded embeddedProjectId={selected?.id ?? ""} embeddedProjectName={selected?.name ?? ""} /></aside>}
        <footer className="studio-console"><TerminalSquare size={15} /><strong>任务输出</strong><span>{notice}</span></footer>
      </div>
      {pendingDelete && (
        <div className="studio-confirm-overlay" role="dialog" aria-modal="true" aria-label="确认删除" onClick={(event) => event.stopPropagation()}>
          <section className="studio-confirm-dialog">
            <Trash2 size={22} />
            <div><h2>{pendingDelete.kind === "project" ? "删除开发项目？" : "删除文件？"}</h2><p>{pendingDelete.kind === "project" ? `项目“${pendingDelete.project.name}”及其全部文件将被删除。` : `“${pendingDelete.path}”将从当前项目中删除。`}</p></div>
            <footer><button className="button ghost" type="button" onClick={() => setPendingDelete(null)} disabled={busy}>取消</button><button className="button danger" type="button" onClick={() => void confirmDelete()} disabled={busy}><Trash2 size={14} /> {busy ? "删除中…" : "删除"}</button></footer>
          </section>
        </div>
      )}
    </div>
  );
}

function NotebookWorkspace({ projectId, content, onChange, onNotice, theme }: { projectId: string; content: string; onChange: (value: string) => void; onNotice: (value: string) => void; theme: "light" | "dark" }) {
  const [document, setDocument] = useState<NotebookDocument>(() => parseNotebook(content));
  const [executing, setExecuting] = useState<number | null>(null);
  const [variables, setVariables] = useState<StudioVariable[]>([]);
  const [kernelStatus, setKernelStatus] = useState<"preparing" | "ready" | "error">("preparing");
  const [editingMarkdownCells, setEditingMarkdownCells] = useState<Set<number>>(() => new Set());

  useEffect(() => { setDocument(parseNotebook(content)); }, [content]);

  useEffect(() => {
    let active = true;
    setKernelStatus("preparing");
    void desktopGateway.prepareStudioKernel(projectId).then(() => {
      if (active) setKernelStatus("ready");
    }).catch((error: unknown) => {
      if (!active) return;
      setKernelStatus("error");
      onNotice(`Kernel 预热失败，首次执行时将重试：${String(error)}`);
    });
    return () => { active = false; };
  }, [onNotice, projectId]);

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
    const outputs = result.outputs.length > 0 ? result.outputs : [];
    if (result.outputs.length === 0) {
      if (result.stdout) outputs.push({ output_type: "stream", name: "stdout", text: result.stdout });
      if (result.stderr) outputs.push({ output_type: "stream", name: "stderr", text: result.stderr });
      if (result.result !== undefined) outputs.push({ output_type: "execute_result", execution_count: result.executionCount, data: { "text/plain": result.result }, metadata: {} });
      if (result.error) outputs.push({ output_type: "error", ename: result.error.split(":")[0], evalue: result.error, traceback: result.traceback });
    }
    next.cells[index].execution_count = result.executionCount;
    next.cells[index].outputs = outputs;
    setVariables(result.variables);
    onNotice(result.error ? `单元格执行失败：${result.error}` : `单元格 [${result.executionCount}] 完成 · ${result.durationMs} ms`);
    return next;
  };

  const runCell = async (index: number) => {
    if (!projectId || executing !== null) return;
    if (document.cells[index]?.cell_type === "markdown") {
      setEditingMarkdownCells((current) => {
        const next = new Set(current);
        next.delete(index);
        return next;
      });
      commit(document, true);
      onNotice(`Markdown 单元格 ${index + 1} 已渲染并保存`);
      return;
    }
    setExecuting(index);
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    try { commit(await execute(document, index), true); setKernelStatus("ready"); }
    catch (error) { onNotice(`Kernel 执行失败：${String(error)}`); }
    finally { setExecuting(null); }
  };

  const runAll = async () => {
    if (!projectId || executing !== null) return;
    setExecuting(-1);
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    try {
      let next = document;
      for (let index = 0; index < next.cells.length; index += 1) next = await execute(next, index);
      commit(next, true);
      setEditingMarkdownCells(new Set());
      setKernelStatus("ready");
      onNotice("全部单元格执行完成，Markdown 已渲染");
    } catch (error) { onNotice(`Kernel 执行失败：${String(error)}`); }
    finally { setExecuting(null); }
  };

  const addCell = (kind: "code" | "markdown") => {
    const next = structuredClone(document);
    next.cells.push({ cell_type: kind, execution_count: null, metadata: {}, outputs: [], source: "" });
    commit(next);
    if (kind === "markdown") setEditingMarkdownCells((current) => new Set(current).add(next.cells.length - 1));
  };

  const deleteCell = (index: number) => {
    const next = structuredClone(document);
    next.cells.splice(index, 1);
    commit(next);
  };

  const restart = async () => {
    await desktopGateway.restartStudioKernel(projectId);
    setKernelStatus("preparing");
    await desktopGateway.prepareStudioKernel(projectId);
    setKernelStatus("ready");
    setVariables([]);
    onNotice("Python Kernel 已重启，内存变量已清空");
  };

  return (
    <div className="notebook-workspace" aria-busy={executing !== null}>
      <div className="notebook-toolbar">
        <span><BookOpen size={15} /> notebook.ipynb</span>
        <button type="button" onClick={() => addCell("code")}><Plus size={13} /> 代码</button>
        <button type="button" onClick={() => addCell("markdown")}><Plus size={13} /> Markdown</button>
        <button type="button" onClick={runAll} disabled={executing !== null}><Play size={13} /> 全部运行</button>
        <button type="button" onClick={restart} disabled={!projectId || executing !== null}><RefreshCw size={13} /> 重启 Kernel</button>
        <em className={`kernel-state ${kernelStatus}`}>{(kernelStatus === "preparing" || executing !== null) && <LoaderCircle className="spin" size={12} />}{executing !== null ? "正在运行" : kernelStatus === "preparing" ? "Kernel 准备中" : kernelStatus === "error" ? "按运行重试" : "Kernel 就绪"}</em>
      </div>
      <div className="notebook-scroll">
        {document.cells.map((cell, index) => (
          <article className="notebook-cell" key={index}>
            <div className="cell-gutter"><button type="button" aria-label={`运行单元格 ${index + 1}`} onClick={() => runCell(index)} disabled={executing !== null}><Play size={13} fill="currentColor" /></button><span>{cell.cell_type === "markdown" ? "MD" : `[${cell.execution_count ?? " "}]`}</span></div>
            <div className="cell-body">
              {cell.cell_type === "code" ? (
                <Editor beforeMount={ensurePythonCompletionProvider} path={projectId ? pythonModelPath(projectId, `notebook.ipynb#cell-${index}`) : undefined} height={`${Math.min(420, Math.max(92, sourceText(cell.source).split("\n").length * 20 + 30))}px`} language="python" value={sourceText(cell.source)} onChange={(value) => updateSource(index, value ?? "")} theme={theme === "dark" ? "vs-dark" : "light"} options={{ fontSize: 13, minimap: { enabled: false }, automaticLayout: true, lineNumbers: "on", scrollBeyondLastLine: false, folding: false, quickSuggestions: { other: true, comments: false, strings: false }, suggestOnTriggerCharacters: true, scrollbar: { vertical: "auto", horizontal: "auto" } }} />
              ) : editingMarkdownCells.has(index) ? (
                <textarea autoFocus className="markdown-cell" aria-label={`编辑 Markdown 单元格 ${index + 1}`} value={sourceText(cell.source)} onChange={(event) => updateSource(index, event.target.value)} placeholder="Markdown 说明…" />
              ) : (
                <div className="markdown-cell-preview" onDoubleClick={() => setEditingMarkdownCells((current) => new Set(current).add(index))}>
                  <button type="button" className="markdown-cell-edit" aria-label={`编辑 Markdown 单元格 ${index + 1}`} onClick={() => setEditingMarkdownCells((current) => new Set(current).add(index))}><Pencil size={12} /> 编辑</button>
                  <Markdown remarkPlugins={[remarkGfm]}>{sourceText(cell.source) || "*双击或点击编辑开始编写 Markdown*"}</Markdown>
                </div>
              )}
              {cell.outputs.length > 0 && <div className="cell-output">{cell.outputs.map((output, outputIndex) => <NotebookOutput output={output} key={outputIndex} />)}</div>}
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
  if (typeof plainText === "string") return plainText;
  const json = data?.["application/json"];
  return json === undefined ? "" : JSON.stringify(json, null, 2);
}

function NotebookOutput({ output }: { output: Record<string, unknown> }) {
  const data = output.data as Record<string, unknown> | undefined;
  const image = data?.["image/png"];
  if (typeof image === "string") return <img className="notebook-output-image" src={`data:image/png;base64,${image.replace(/\s/g, "")}`} alt="Notebook 输出" />;
  const html = data?.["text/html"];
  if (typeof html === "string") return <iframe className="notebook-output-html" title="Notebook HTML 输出" sandbox="" srcDoc={html} />;
  return <pre className={output.output_type === "error" ? "error" : ""}>{outputText(output)}</pre>;
}
