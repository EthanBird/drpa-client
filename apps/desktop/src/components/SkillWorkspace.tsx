import Editor, { loader } from "@monaco-editor/react";
import * as monaco from "monaco-editor/esm/vs/editor/editor.api.js";
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker.js?worker";
import "monaco-editor/esm/vs/basic-languages/python/python.contribution.js";
import "monaco-editor/esm/vs/basic-languages/yaml/yaml.contribution.js";
import {
  ChevronDown,
  ChevronRight,
  CircleAlert,
  FileCode2,
  FilePlus2,
  FileText,
  Folder,
  FolderOpen,
  FolderPlus,
  Pencil,
  Plus,
  RefreshCw,
  Save,
  Trash2,
  X,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { useAppStore } from "../app/store";
import type { AgentSkillEntry, AgentSkillPackage, AgentWorkspaceConfig } from "../domain/models";
import { desktopGateway } from "../infra/gateway";
import { SidebarToggle, useSidebarCollapsed } from "./SidebarToggle";

self.MonacoEnvironment = { getWorker: () => new EditorWorker() };
loader.config({ monaco });

interface SkillTreeNode extends AgentSkillEntry {
  name: string;
  children: SkillTreeNode[];
}

const protectedPaths = new Set(["skill.yaml", "instructions.md"]);

function skillManifestTemplate(name: string) {
  return `schema: 2\nid: ${name}\nname: ${name}\nversion: 1.0.0\ndescription: 说明这个 Skill 解决什么问题，以及应该在何时使用。\nactivation:\n  intents: []\n  file_patterns: []\npermissions:\n  workspace_read: true\n  workspace_write: false\n  network: false\ntools: []\nlibraries: []\n`;
}

function skillInstructionsTemplate(name: string) {
  return `# ${name}\n\n## 工作流\n\n1. 读取必要上下文。\n2. 执行可验证的最小步骤。\n3. 运行检查并报告结果。\n`;
}

function fileTemplate(path: string) {
  if (path.endsWith(".py")) return "def run(arguments, context):\n    return {\"arguments\": arguments}\n";
  if (path.endsWith(".json")) return "{}\n";
  if (path.endsWith(".yaml") || path.endsWith(".yml")) return "# YAML\n";
  if (path.endsWith(".md")) return `# ${fileName(path).replace(/\.md$/i, "")}\n`;
  return "";
}

export function SkillWorkspace() {
  const packagesCollapsed = useSidebarCollapsed("skills-packages");
  const filesCollapsed = useSidebarCollapsed("skills-files");
  const theme = useAppStore((state) => state.theme);
  const [workspace, setWorkspace] = useState<AgentWorkspaceConfig | null>(null);
  const [skillPackage, setSkillPackage] = useState<AgentSkillPackage | null>(null);
  const [selectedSkill, setSelectedSkill] = useState("");
  const [selectedPath, setSelectedPath] = useState("");
  const [content, setContent] = useState("");
  const [savedContent, setSavedContent] = useState("");
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [newSkillName, setNewSkillName] = useState("");
  const [newPath, setNewPath] = useState("");
  const [createKind, setCreateKind] = useState<"file" | "directory">("file");
  const [renamingPath, setRenamingPath] = useState("");
  const [renameValue, setRenameValue] = useState("");
  const [pendingDeletePath, setPendingDeletePath] = useState("");
  const [pendingDeleteSkill, setPendingDeleteSkill] = useState("");
  const [notice, setNotice] = useState("正在初始化 Skills 工作区…");
  const [busy, setBusy] = useState(false);
  const renameRef = useRef<HTMLInputElement>(null);

  const entries = skillPackage?.entries ?? skillPackage?.files.map((path) => ({ path, kind: "file" as const })) ?? [];
  const tree = useMemo(() => buildSkillTree(entries), [entries]);
  const selectedEntry = entries.find((entry) => entry.path === selectedPath);
  const dirty = Boolean(selectedEntry?.kind === "file" && content !== savedContent);
  const parent = selectedEntry?.kind === "directory" ? selectedEntry.path : parentPath(selectedPath);

  const loadFile = async (skill: string, path: string) => {
    const source = await desktopGateway.readAgentSkillFile(skill, path);
    setSelectedPath(path);
    setContent(source);
    setSavedContent(source);
  };

  const loadSkill = async (name: string, preferredPath = "skill.yaml") => {
    const packageData = await desktopGateway.readAgentSkillPackage(name);
    setSelectedSkill(name);
    setSkillPackage(packageData);
    const available = packageData.entries ?? packageData.files.map((path) => ({ path, kind: "file" as const }));
    setExpanded(new Set(available.filter((entry) => entry.kind === "directory").map((entry) => entry.path)));
    const target = available.find((entry) => entry.path === preferredPath && entry.kind === "file")
      ?? available.find((entry) => entry.kind === "file");
    if (target) await loadFile(name, target.path);
    else {
      setSelectedPath("");
      setContent("");
      setSavedContent("");
    }
  };

  const refresh = async (preferredSkill?: string, preferredPath?: string) => {
    const next = await desktopGateway.getAgentWorkspaceConfig();
    setWorkspace(next);
    const skill = preferredSkill && next.skills.some((item) => item.name === preferredSkill)
      ? preferredSkill
      : selectedSkill && next.skills.some((item) => item.name === selectedSkill)
        ? selectedSkill
        : next.skills[0]?.name ?? "";
    if (skill) await loadSkill(skill, preferredPath ?? (skill === selectedSkill ? selectedPath : "skill.yaml"));
    else {
      setSelectedSkill("");
      setSkillPackage(null);
    }
    setNotice(`${next.skills.length} 个能力包 · 本地 ToolRegistry`);
  };

  useEffect(() => {
    void refresh().catch((error: unknown) => setNotice(`读取 Skills 失败：${String(error)}`));
  }, []);

  useEffect(() => {
    if (renamingPath) window.setTimeout(() => renameRef.current?.focus(), 0);
  }, [renamingPath]);

  const createSkill = async () => {
    const name = newSkillName.trim().toLowerCase().replace(/\s+/g, "-");
    if (!name) return;
    setBusy(true);
    try {
      await desktopGateway.writeAgentSkillPackage(name, skillManifestTemplate(name), skillInstructionsTemplate(name));
      setNewSkillName("");
      await refresh(name, "skill.yaml");
      setNotice(`Skill 已创建：${name}`);
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    if (!selectedSkill || !selectedPath || selectedEntry?.kind !== "file") return;
    setBusy(true);
    try {
      await desktopGateway.writeAgentSkillFile(selectedSkill, selectedPath, content);
      setSavedContent(content);
      await refresh(selectedSkill, selectedPath);
      setNotice(`已保存 ${selectedPath}`);
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy(false);
    }
  };

  const createPath = async () => {
    const value = newPath.trim().replace(/\\/g, "/").replace(/^\/+|\/+$/g, "");
    if (!selectedSkill || !value) return;
    const target = joinPath(parent, value);
    setBusy(true);
    try {
      if (createKind === "directory") await desktopGateway.createAgentSkillDirectory(selectedSkill, target);
      else await desktopGateway.writeAgentSkillFile(selectedSkill, target, fileTemplate(target));
      setNewPath("");
      await refresh(selectedSkill, createKind === "file" ? target : selectedPath);
      if (createKind === "directory") setExpanded((current) => new Set(current).add(target));
      setNotice(`已创建${createKind === "directory" ? "目录" : "文件"}：${target}`);
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy(false);
    }
  };

  const beginRename = (path: string) => {
    if (protectedPaths.has(path)) return;
    setRenamingPath(path);
    setRenameValue(fileName(path));
  };

  const commitRename = async () => {
    const name = renameValue.trim().replace(/[\\/]/g, "");
    if (!selectedSkill || !renamingPath || !name) return;
    const target = joinPath(parentPath(renamingPath), name);
    setBusy(true);
    try {
      await desktopGateway.renameAgentSkillPath(selectedSkill, renamingPath, target);
      const selectedWasRenamed = selectedPath === renamingPath || selectedPath.startsWith(`${renamingPath}/`);
      const nextSelected = selectedWasRenamed ? `${target}${selectedPath.slice(renamingPath.length)}` : selectedPath;
      setRenamingPath("");
      await refresh(selectedSkill, nextSelected);
      setNotice(`已重命名为 ${target}`);
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy(false);
    }
  };

  const deletePath = async () => {
    if (!selectedSkill || !pendingDeletePath) return;
    setBusy(true);
    try {
      await desktopGateway.deleteAgentSkillPath(selectedSkill, pendingDeletePath);
      const nextPath = selectedPath === pendingDeletePath || selectedPath.startsWith(`${pendingDeletePath}/`) ? "skill.yaml" : selectedPath;
      setPendingDeletePath("");
      await refresh(selectedSkill, nextPath);
      setNotice(`已删除 ${pendingDeletePath}`);
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy(false);
    }
  };

  const deleteSkill = async () => {
    if (!pendingDeleteSkill) return;
    setBusy(true);
    try {
      await desktopGateway.deleteAgentSkill(pendingDeleteSkill);
      setPendingDeleteSkill("");
      setSelectedSkill("");
      await refresh();
      setNotice("Skill 能力包已删除");
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="settings-card settings-card-wide skills-library-card skills-workspace-card">
      <header><FileCode2 size={18} /><div><h2>Skills 2.0 能力工作区</h2><p>使用目录树和 Monaco 编辑清单、指令、工作流、工具代码、资源及可调用代码库。</p></div><button className="button ghost small" type="button" onClick={() => void refresh(selectedSkill, selectedPath)} disabled={busy}><RefreshCw size={12} /> 刷新</button></header>
      <div className={`skills-workspace-layout${packagesCollapsed ? " packages-collapsed" : ""}${filesCollapsed ? " files-collapsed" : ""}`}>
        {packagesCollapsed ? <SidebarToggle id="skills-packages" side="left" label="Skills 列表侧边栏" restore /> : <aside className="skills-package-list collapsible-sidebar">
          <SidebarToggle id="skills-packages" side="left" label="Skills 列表侧边栏" />
          <div className="skill-create-row"><input aria-label="新 Skill 名称" value={newSkillName} onChange={(event) => setNewSkillName(event.target.value.toLowerCase())} onKeyDown={(event) => { if (event.key === "Enter") void createSkill(); }} placeholder="new-skill" /><button type="button" aria-label="创建 Skill" onClick={() => void createSkill()} disabled={busy || !newSkillName.trim()}><Plus size={14} /></button></div>
          <div className="skill-list">{workspace?.skills.map((skill) => <button type="button" className={selectedSkill === skill.name ? "active" : ""} onClick={() => void loadSkill(skill.name)} key={skill.name}><strong>{skill.displayName || skill.name}</strong><span>{skill.description}</span><small>v{skill.version || "1.0.0"} · {skill.toolCount || 0} tools · {skill.libraryCount || 0} libs</small></button>)}</div>
          <button className="button ghost small danger-text skills-delete-package" type="button" disabled={!selectedSkill || busy} onClick={() => setPendingDeleteSkill(selectedSkill)}><Trash2 size={12} /> 删除能力包</button>
        </aside>}

        {filesCollapsed ? <SidebarToggle id="skills-files" side="left" label="Skill 文件侧边栏" restore /> : <aside className="skill-file-explorer collapsible-sidebar">
          <SidebarToggle id="skills-files" side="left" label="Skill 文件侧边栏" />
          <header><span><FolderOpen size={13} /> 能力包文件</span><code>{selectedSkill || "-"}</code></header>
          <div className="skill-path-create">
            <div><button type="button" className={createKind === "file" ? "active" : ""} onClick={() => setCreateKind("file")} title="新建文件"><FilePlus2 size={12} /></button><button type="button" className={createKind === "directory" ? "active" : ""} onClick={() => setCreateKind("directory")} title="新建目录"><FolderPlus size={12} /></button></div>
            <input aria-label="新能力包路径" value={newPath} onChange={(event) => setNewPath(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") void createPath(); }} placeholder={createKind === "file" ? "tools/example.py" : "lib"} />
            <button type="button" onClick={() => void createPath()} disabled={!selectedSkill || !newPath.trim() || busy}><Plus size={12} /></button>
          </div>
          <div className="skill-file-tree" role="tree" aria-label="Skill 文件目录">
            {tree.map((node) => <SkillTreeRow key={node.path} node={node} depth={0} selectedPath={selectedPath} expanded={expanded} renamingPath={renamingPath} renameValue={renameValue} renameRef={renameRef} onRenameValue={setRenameValue} onCommitRename={() => void commitRename()} onCancelRename={() => setRenamingPath("")} onSelect={(entry) => { if (entry.kind === "directory") { setSelectedPath(entry.path); setExpanded((current) => toggleSet(current, entry.path)); } else void loadFile(selectedSkill, entry.path); }} onToggle={(path) => setExpanded((current) => toggleSet(current, path))} onRename={beginRename} onDelete={setPendingDeletePath} />)}
          </div>
        </aside>}

        <div className="skill-monaco-pane">
          <header><div><strong>{selectedPath || "选择一个文件"}</strong><span>{notice}</span></div>{dirty && <em>未保存</em>}<button className="button secondary small" type="button" disabled={!selectedPath || selectedEntry?.kind !== "file" || busy || !dirty} onClick={() => void save()}><Save size={12} /> 保存</button></header>
          {selectedEntry?.kind === "file" ? <Editor path={`drpa-skill:///${encodeURIComponent(selectedSkill)}/${selectedPath}`} height="100%" language={editorLanguage(selectedPath)} value={content} onChange={(value) => setContent(value ?? "")} theme={theme === "dark" ? "vs-dark" : "light"} options={{ fontSize: 13, automaticLayout: true, minimap: { enabled: false }, tabSize: 2, insertSpaces: true, wordWrap: "on", quickSuggestions: true, suggestOnTriggerCharacters: true, scrollBeyondLastLine: false }} /> : <div className="skill-editor-empty"><Folder size={28} /><strong>{selectedPath || "选择能力包文件"}</strong><span>选择文件开始编辑，或在当前目录创建工具、工作流和代码库。</span></div>}
        </div>
      </div>

      {pendingDeletePath && <div className="knowledge-confirm-overlay" role="dialog" aria-modal="true" aria-label="确认删除 Skill 路径"><section className="knowledge-confirm"><div className="knowledge-confirm-icon"><CircleAlert size={18} /></div><div><h2>删除能力包内容？</h2><p><strong>{pendingDeletePath}</strong> 及其子文件将被删除。</p></div><footer><button className="button ghost" type="button" onClick={() => setPendingDeletePath("")}><X size={13} /> 取消</button><button className="button danger" type="button" onClick={() => void deletePath()}><Trash2 size={13} /> 删除</button></footer></section></div>}
      {pendingDeleteSkill && <div className="knowledge-confirm-overlay" role="dialog" aria-modal="true" aria-label="确认删除 Skill"><section className="knowledge-confirm"><div className="knowledge-confirm-icon"><CircleAlert size={18} /></div><div><h2>删除 Skill？</h2><p>“{pendingDeleteSkill}”的清单、指令、工具和代码库将全部删除。</p></div><footer><button className="button ghost" type="button" onClick={() => setPendingDeleteSkill("")}><X size={13} /> 取消</button><button className="button danger" type="button" onClick={() => void deleteSkill()}><Trash2 size={13} /> 删除</button></footer></section></div>}
    </section>
  );
}

function SkillTreeRow({ node, depth, selectedPath, expanded, renamingPath, renameValue, renameRef, onRenameValue, onCommitRename, onCancelRename, onSelect, onToggle, onRename, onDelete }: {
  node: SkillTreeNode;
  depth: number;
  selectedPath: string;
  expanded: Set<string>;
  renamingPath: string;
  renameValue: string;
  renameRef: React.RefObject<HTMLInputElement | null>;
  onRenameValue: (value: string) => void;
  onCommitRename: () => void;
  onCancelRename: () => void;
  onSelect: (entry: AgentSkillEntry) => void;
  onToggle: (path: string) => void;
  onRename: (path: string) => void;
  onDelete: (path: string) => void;
}) {
  const directory = node.kind === "directory";
  const opened = expanded.has(node.path);
  return <div role="treeitem" aria-expanded={directory ? opened : undefined}>
    <div className={`skill-tree-row ${selectedPath === node.path ? "selected" : ""}`} style={{ paddingLeft: 6 + depth * 14 }} onClick={() => onSelect(node)}>
      <button type="button" className="skill-tree-chevron" onClick={(event) => { event.stopPropagation(); if (directory) onToggle(node.path); }}>{directory ? opened ? <ChevronDown size={11} /> : <ChevronRight size={11} /> : null}</button>
      {directory ? opened ? <FolderOpen size={13} /> : <Folder size={13} /> : node.path.endsWith(".md") ? <FileText size={13} /> : <FileCode2 size={13} />}
      {renamingPath === node.path ? <input ref={renameRef} value={renameValue} onChange={(event) => onRenameValue(event.target.value)} onClick={(event) => event.stopPropagation()} onBlur={onCommitRename} onKeyDown={(event) => { if (event.key === "Enter") onCommitRename(); if (event.key === "Escape") onCancelRename(); }} /> : <span title={node.path}>{node.name}</span>}
      {!protectedPaths.has(node.path) && renamingPath !== node.path && <div className="skill-tree-actions"><button type="button" title="重命名" onClick={(event) => { event.stopPropagation(); onRename(node.path); }}><Pencil size={11} /></button><button type="button" title="删除" onClick={(event) => { event.stopPropagation(); onDelete(node.path); }}><Trash2 size={11} /></button></div>}
    </div>
    {directory && opened && node.children.map((child) => <SkillTreeRow key={child.path} node={child} depth={depth + 1} selectedPath={selectedPath} expanded={expanded} renamingPath={renamingPath} renameValue={renameValue} renameRef={renameRef} onRenameValue={onRenameValue} onCommitRename={onCommitRename} onCancelRename={onCancelRename} onSelect={onSelect} onToggle={onToggle} onRename={onRename} onDelete={onDelete} />)}
  </div>;
}

function buildSkillTree(entries: AgentSkillEntry[]): SkillTreeNode[] {
  const nodes = new Map<string, SkillTreeNode>();
  const ensureDirectory = (path: string) => {
    if (!path || nodes.has(path)) return;
    ensureDirectory(parentPath(path));
    nodes.set(path, { path, name: fileName(path), kind: "directory", children: [] });
  };
  for (const entry of entries) {
    ensureDirectory(parentPath(entry.path));
    nodes.set(entry.path, { ...entry, name: fileName(entry.path), children: [] });
  }
  const roots: SkillTreeNode[] = [];
  for (const node of nodes.values()) {
    const parent = nodes.get(parentPath(node.path));
    if (parent?.kind === "directory") parent.children.push(node); else roots.push(node);
  }
  const sort = (items: SkillTreeNode[]) => {
    items.sort((left, right) => left.kind === right.kind ? left.name.localeCompare(right.name, "zh-CN", { numeric: true }) : left.kind === "directory" ? -1 : 1);
    items.forEach((item) => sort(item.children));
  };
  sort(roots);
  return roots;
}

function toggleSet(current: Set<string>, value: string) {
  const next = new Set(current);
  if (next.has(value)) next.delete(value); else next.add(value);
  return next;
}

function parentPath(path: string) { return path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : ""; }
function fileName(path: string) { return path.split("/").at(-1) ?? path; }
function joinPath(parent: string, child: string) { return parent ? `${parent}/${child}` : child; }
function editorLanguage(path: string) {
  const extension = path.split(".").at(-1)?.toLowerCase();
  if (extension === "py") return "python";
  if (extension === "yaml" || extension === "yml") return "yaml";
  if (extension === "json") return "json";
  if (extension === "md" || extension === "markdown") return "markdown";
  if (extension === "toml") return "ini";
  return "plaintext";
}
