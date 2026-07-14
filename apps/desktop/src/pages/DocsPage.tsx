import { open, save as saveDialog } from "@tauri-apps/plugin-dialog";
import {
  BookOpen,
  Check,
  ChevronDown,
  ChevronRight,
  Download,
  Eye,
  FilePenLine,
  FilePlus2,
  FileText,
  Folder,
  FolderOpen,
  FolderPlus,
  MoreHorizontal,
  Pencil,
  RefreshCw,
  Save,
  Search,
  Trash2,
  Upload,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, type DragEvent, type MouseEvent } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";

import type { KnowledgeEntry } from "../domain/models";
import { desktopGateway } from "../infra/gateway";

type ReaderMode = "preview" | "edit" | "split";
type EntryKind = KnowledgeEntry["kind"];

interface TreeNode {
  entry: KnowledgeEntry;
  children: TreeNode[];
}

interface EntryMenu {
  entry: KnowledgeEntry;
  x: number;
  y: number;
}

type InlineDraft =
  | { mode: "create"; kind: EntryKind; parent: string; value: string }
  | { mode: "rename"; source: KnowledgeEntry; parent: string; value: string };

export function DocsPage() {
  const [entries, setEntries] = useState<KnowledgeEntry[]>([]);
  const [selectedPath, setSelectedPath] = useState("");
  const [selectedEntryPath, setSelectedEntryPath] = useState("");
  const [content, setContent] = useState("");
  const [savedContent, setSavedContent] = useState("");
  const [mode, setMode] = useState<ReaderMode>("preview");
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [query, setQuery] = useState("");
  const [notice, setNotice] = useState("正在初始化本地知识库…");
  const [busy, setBusy] = useState(false);
  const [menu, setMenu] = useState<EntryMenu | null>(null);
  const [draft, setDraft] = useState<InlineDraft | null>(null);
  const [pendingDelete, setPendingDelete] = useState<KnowledgeEntry | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const dirty = selectedPath !== "" && content !== savedContent;
  const entryMap = useMemo(() => new Map(entries.map((entry) => [entry.path, entry])), [entries]);
  const tree = useMemo(() => buildTree(entries), [entries]);
  const selectedEntry = entryMap.get(selectedEntryPath);
  const currentDirectory = selectedEntry?.kind === "directory"
    ? selectedEntry.path
    : selectedEntryPath.includes("/")
      ? selectedEntryPath.slice(0, selectedEntryPath.lastIndexOf("/"))
      : "";

  const persist = useCallback(async (path: string, value: string, quiet = false) => {
    if (!path) return;
    await desktopGateway.writeKnowledgeFile(path, value);
    setSavedContent(value);
    if (!quiet) setNotice(`已保存 ${path}`);
  }, []);

  const refresh = useCallback(async (preferredPath?: string) => {
    const next = await desktopGateway.listKnowledgeEntries();
    setEntries(next);
    setExpanded((current) => {
      if (current.size > 0) return current;
      return new Set(next.filter((entry) => entry.kind === "directory").map((entry) => entry.path));
    });
    const preferred = preferredPath && next.some((entry) => entry.path === preferredPath) ? preferredPath : "";
    const firstFile = next.find((entry) => entry.path === "RPAZ 开发指南/00_阅读指南.md" && entry.kind === "file")
      ?? next.find((entry) => entry.kind === "file");
    const retained = selectedPath && next.some((entry) => entry.path === selectedPath && entry.kind === "file") ? selectedPath : "";
    const target = preferred || retained || firstFile?.path || "";
    if (target && next.some((entry) => entry.path === target && entry.kind === "file")) {
      setSelectedEntryPath(target);
      setSelectedPath(target);
    }
    setNotice(`${next.filter((entry) => entry.kind === "file").length} 篇 Markdown · 本地离线知识库`);
  }, [selectedPath]);

  useEffect(() => {
    void refresh().catch((error: unknown) => setNotice(`读取知识库失败：${String(error)}`));
  }, []);

  useEffect(() => {
    if (!selectedPath) {
      setContent("");
      setSavedContent("");
      return;
    }
    let active = true;
    setBusy(true);
    void desktopGateway.readKnowledgeFile(selectedPath)
      .then((source) => {
        if (!active) return;
        setContent(source);
        setSavedContent(source);
        setNotice(`正在查看 ${selectedPath}`);
      })
      .catch((error: unknown) => active && setNotice(`打开文档失败：${String(error)}`))
      .finally(() => active && setBusy(false));
    return () => { active = false; };
  }, [selectedPath]);

  useEffect(() => {
    if (!dirty || busy) return;
    const timer = window.setTimeout(() => {
      void persist(selectedPath, content, true).catch((error: unknown) => setNotice(`自动保存失败：${String(error)}`));
    }, 900);
    return () => window.clearTimeout(timer);
  }, [busy, content, dirty, persist, selectedPath]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
        event.preventDefault();
        if (dirty) void persist(selectedPath, content).catch((error: unknown) => setNotice(`保存失败：${String(error)}`));
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [content, dirty, persist, selectedPath]);

  useEffect(() => {
    if (draft) window.setTimeout(() => inputRef.current?.select(), 0);
  }, [draft]);

  const openDocument = useCallback(async (path: string) => {
    if (path === selectedPath) return;
    if (dirty) await persist(selectedPath, content, true);
    setSelectedEntryPath(path);
    setSelectedPath(path);
  }, [content, dirty, persist, selectedPath]);

  const toggleDirectory = (path: string) => {
    setSelectedEntryPath(path);
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(path)) next.delete(path); else next.add(path);
      return next;
    });
  };

  const startCreate = (kind: EntryKind, parent = currentDirectory) => {
    setMenu(null);
    setExpanded((current) => new Set(current).add(parent));
    setDraft({ mode: "create", kind, parent, value: kind === "file" ? "新文档.md" : "新文件夹" });
  };

  const startRename = (entry: KnowledgeEntry) => {
    const parent = parentPath(entry.path);
    setMenu(null);
    setSelectedEntryPath(entry.path);
    setDraft({ mode: "rename", source: entry, parent, value: entry.name });
  };

  const commitDraft = async () => {
    if (!draft) return;
    let name = draft.value.trim();
    if (!name || name === "." || name === ".." || /[\\/:*?"<>|]/.test(name)) {
      setNotice("名称不能为空，也不能包含路径分隔符或 Windows 保留字符");
      return;
    }
    const kind = draft.mode === "create" ? draft.kind : draft.source.kind;
    if (kind === "file" && !/\.(md|markdown)$/i.test(name)) name += ".md";
    const target = joinPath(draft.parent, name);
    setBusy(true);
    try {
      if (dirty) await persist(selectedPath, content, true);
      if (draft.mode === "create") {
        await desktopGateway.createKnowledgeEntry(target, kind);
        await refresh(target);
        if (kind === "file") await openDocument(target);
        else setSelectedEntryPath(target);
        setNotice(`已创建${kind === "file" ? "文档" : "文件夹"} ${target}`);
      } else {
        await desktopGateway.renameKnowledgeEntry(draft.source.path, target);
        const nextSelected = selectedPath === draft.source.path
          ? target
          : selectedPath.startsWith(`${draft.source.path}/`)
            ? `${target}${selectedPath.slice(draft.source.path.length)}`
            : selectedPath;
        if (selectedPath === draft.source.path) setSelectedPath("");
        await refresh(nextSelected || target);
        setSelectedEntryPath(target);
        if (nextSelected && entryMap.get(draft.source.path)?.kind === "file") setSelectedPath(target);
        setNotice(`已重命名为 ${target}`);
      }
      setDraft(null);
    } catch (error) {
      setNotice(`知识条目操作失败：${String(error)}`);
    } finally {
      setBusy(false);
    }
  };

  const confirmDelete = async () => {
    if (!pendingDelete) return;
    setBusy(true);
    try {
      await desktopGateway.deleteKnowledgeEntry(pendingDelete.path);
      const removedSelected = selectedPath === pendingDelete.path || selectedPath.startsWith(`${pendingDelete.path}/`);
      if (removedSelected) {
        setSelectedPath("");
        setSelectedEntryPath("");
      }
      setPendingDelete(null);
      await refresh();
      setNotice(`已删除 ${pendingDelete.path}`);
    } catch (error) {
      setNotice(`删除失败：${String(error)}`);
    } finally {
      setBusy(false);
    }
  };

  const importPaths = useCallback(async (paths: string[]) => {
    const markdownPaths = paths.filter((path) => /\.(md|markdown)$/i.test(path));
    if (markdownPaths.length === 0) {
      setNotice("请选择 UTF-8 编码的 .md 或 .markdown 文件");
      return;
    }
    setBusy(true);
    try {
      if (dirty) await persist(selectedPath, content, true);
      const imported = await desktopGateway.importKnowledgeFiles(markdownPaths, currentDirectory);
      await refresh(imported.at(-1));
      if (imported.at(-1)) await openDocument(imported.at(-1) ?? "");
      setNotice(`已导入 ${imported.length} 篇 Markdown 文档`);
    } catch (error) {
      setNotice(`导入失败：${String(error)}`);
    } finally {
      setBusy(false);
    }
  }, [content, currentDirectory, dirty, openDocument, persist, refresh, selectedPath]);

  useEffect(() => {
    const handleNativeDrop = (event: Event) => {
      const paths = (event as CustomEvent<{ paths?: string[] }>).detail?.paths ?? [];
      void importPaths(paths);
    };
    window.addEventListener("drpa-knowledge-file-drop", handleNativeDrop);
    return () => window.removeEventListener("drpa-knowledge-file-drop", handleNativeDrop);
  }, [importPaths]);

  const pickAndImport = async () => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const selected = await open({ multiple: true, filters: [{ name: "Markdown", extensions: ["md", "markdown"] }] });
    if (!selected) return;
    await importPaths(Array.isArray(selected) ? selected : [selected]);
  };

  const exportCurrent = async () => {
    if (!selectedPath || !("__TAURI_INTERNALS__" in window)) return;
    if (dirty) await persist(selectedPath, content, true);
    const target = await saveDialog({ defaultPath: fileName(selectedPath), filters: [{ name: "Markdown", extensions: ["md", "markdown"] }] });
    if (!target) return;
    try {
      const exported = await desktopGateway.exportKnowledgeFile(selectedPath, target);
      setNotice(`已导出 ${exported}`);
    } catch (error) {
      setNotice(`导出失败：${String(error)}`);
    }
  };

  const dropBrowserFiles = async (event: DragEvent<HTMLElement>) => {
    event.preventDefault();
    const files = Array.from(event.dataTransfer.files).filter((file) => /\.(md|markdown)$/i.test(file.name));
    if (files.length === 0) return setNotice("拖入的文件不是 Markdown 文档");
    setBusy(true);
    try {
      if (dirty) await persist(selectedPath, content, true);
      let last = "";
      for (const file of files) {
        last = uniqueBrowserPath(entries, currentDirectory, file.name);
        await desktopGateway.createKnowledgeEntry(last, "file");
        await desktopGateway.writeKnowledgeFile(last, await file.text());
      }
      await refresh(last);
      await openDocument(last);
      setNotice(`已拖拽导入 ${files.length} 篇 Markdown 文档`);
    } catch (error) {
      setNotice(`拖拽导入失败：${String(error)}`);
    } finally {
      setBusy(false);
    }
  };

  const followLink = (href: string) => {
    if (!selectedPath || !href || href.startsWith("#")) return false;
    if (/^[a-z]+:/i.test(href) || href.startsWith("//")) return false;
    const target = resolveMarkdownLink(selectedPath, href);
    if (!target || !entryMap.has(target)) {
      setNotice(`本地文档链接不存在：${target || href}`);
      return true;
    }
    void openDocument(target);
    return true;
  };

  const filteredEntries = query.trim()
    ? entries.filter((entry) => entry.kind === "file" && entry.path.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()))
    : [];

  return (
    <div className="page knowledge-page" onClick={() => setMenu(null)} onDragOver={(event) => event.preventDefault()} onDrop={(event) => void dropBrowserFiles(event)}>
      <header className="knowledge-header">
        <div>
          <div className="eyebrow">LOCAL MARKDOWN WORKSPACE</div>
          <h1>知识文档</h1>
          <p>离线 Markdown 知识库 · 文件树、编辑、渲染、跳转与导入导出全部内置</p>
        </div>
        <div className="knowledge-header-actions">
          <button className="button ghost small" type="button" onClick={() => startCreate("file")} disabled={busy}><FilePlus2 size={13} /> 新建文档</button>
          <button className="button ghost small" type="button" onClick={() => startCreate("directory")} disabled={busy}><FolderPlus size={13} /> 新建目录</button>
          <button className="button ghost small" type="button" onClick={() => void pickAndImport()} disabled={busy || !("__TAURI_INTERNALS__" in window)}><Upload size={13} /> 导入</button>
          <button className="button ghost small" type="button" onClick={() => void exportCurrent()} disabled={busy || !selectedPath || !("__TAURI_INTERNALS__" in window)}><Download size={13} /> 导出</button>
        </div>
      </header>

      <div className="knowledge-layout">
        <aside className="knowledge-sidebar">
          <div className="knowledge-search"><Search size={13} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索文档名称…" /></div>
          <div className="knowledge-tree-title"><span><BookOpen size={13} /> 本地知识库</span><button type="button" title="刷新目录" onClick={() => void refresh(selectedPath)}><RefreshCw size={12} /></button></div>
          <div className="knowledge-tree" role="tree" aria-label="知识库目录">
            {draft && (
              <div className="knowledge-inline-entry">
                {draft.mode === "create" && draft.kind === "directory" ? <FolderPlus size={13} /> : <FilePenLine size={13} />}
                <span title={draft.parent}>{draft.parent ? `${draft.parent}/` : ""}</span>
                <input ref={inputRef} aria-label={draft.mode === "rename" ? "重命名知识条目" : draft.kind === "file" ? "新文档名称" : "新目录名称"} value={draft.value} onChange={(event) => setDraft({ ...draft, value: event.target.value })} onKeyDown={(event) => { if (event.key === "Enter") { event.preventDefault(); void commitDraft(); } if (event.key === "Escape") setDraft(null); }} />
              </div>
            )}
            {query.trim() ? filteredEntries.map((entry) => (
              <button key={entry.path} className={entry.path === selectedEntryPath ? "knowledge-search-result selected" : "knowledge-search-result"} type="button" onClick={() => void openDocument(entry.path)}><FileText size={13} /><span><strong>{entry.name}</strong><small>{parentPath(entry.path) || "根目录"}</small></span></button>
            )) : tree.map((node) => (
              <KnowledgeTreeNode key={node.entry.path} node={node} depth={0} expanded={expanded} selectedPath={selectedEntryPath} onOpen={openDocument} onToggle={toggleDirectory} onMenu={setMenu} />
            ))}
            {entries.length === 0 && <div className="knowledge-tree-empty">知识库为空<br />新建或导入一篇 Markdown</div>}
          </div>
          <footer className="knowledge-sidebar-footer"><span>{entries.filter((entry) => entry.kind === "file").length} 篇文档</span><span>{entries.filter((entry) => entry.kind === "directory").length} 个目录</span></footer>
        </aside>

        <section className="knowledge-workspace">
          <div className="knowledge-toolbar">
            <div className="knowledge-breadcrumbs">
              <BookOpen size={13} />
              {selectedPath ? selectedPath.split("/").map((part, index, all) => <span key={`${part}-${index}`}>{index > 0 && <ChevronRight size={10} />}{part}</span>) : <span>选择一篇文档</span>}
            </div>
            <div className="knowledge-save-state">{dirty ? <><span className="unsaved" /> 正在保存</> : <><Check size={11} /> 已保存</>}</div>
            <div className="knowledge-mode-switch" aria-label="阅读模式">
              <button type="button" className={mode === "preview" ? "active" : ""} onClick={() => setMode("preview")} title="阅读"><Eye size={13} /></button>
              <button type="button" className={mode === "edit" ? "active" : ""} onClick={() => setMode("edit")} title="编辑"><Pencil size={13} /></button>
              <button type="button" className={mode === "split" ? "active" : ""} onClick={() => setMode("split")} title="分栏"><FilePenLine size={13} /></button>
            </div>
            <button className="knowledge-save-button" type="button" onClick={() => void persist(selectedPath, content)} disabled={!dirty || busy} title="保存 (Ctrl+S)"><Save size={13} /></button>
          </div>

          {!selectedPath ? (
            <div className="knowledge-welcome"><div><BookOpen size={28} /><h2>建立你的本地知识库</h2><p>选择左侧默认 RPAZ 指南，或新建、导入 Markdown 文档。所有内容保存在当前安装的数据目录中。</p><button className="button primary" type="button" onClick={() => startCreate("file", "")}><FilePlus2 size={14} /> 新建第一篇文档</button></div></div>
          ) : (
            <div className={`knowledge-document mode-${mode}`}>
              {(mode === "edit" || mode === "split") && <textarea className="knowledge-editor" aria-label="Markdown 编辑器" spellCheck={false} value={content} onChange={(event) => setContent(event.target.value)} />}
              {(mode === "preview" || mode === "split") && (
                <article className="knowledge-preview">
                  <Markdown
                    remarkPlugins={[remarkGfm]}
                    components={{
                      a: ({ href = "", children, ...props }) => <a {...props} href={href} onClick={(event) => { if (followLink(href)) event.preventDefault(); }} target={/^(https?:)?\/\//i.test(href) ? "_blank" : undefined} rel={/^(https?:)?\/\//i.test(href) ? "noreferrer" : undefined}>{children}</a>,
                    }}
                  >{content}</Markdown>
                </article>
              )}
            </div>
          )}
          <footer className="knowledge-status"><span>{notice}</span>{selectedPath && <><span>UTF-8</span><span>Markdown</span><span>{content.length.toLocaleString()} 字符</span></>}</footer>
        </section>
      </div>

      {menu && (
        <div className="context-menu knowledge-context-menu" style={{ left: Math.min(menu.x, window.innerWidth - 210), top: Math.min(menu.y, window.innerHeight - 190) }} onClick={(event) => event.stopPropagation()}>
          {menu.entry.kind === "directory" && <><button type="button" onClick={() => startCreate("file", menu.entry.path)}><FilePlus2 size={13} /> 新建子文档</button><button type="button" onClick={() => startCreate("directory", menu.entry.path)}><FolderPlus size={13} /> 新建子目录</button><span /></>}
          <button type="button" onClick={() => startRename(menu.entry)}><Pencil size={13} /> 重命名</button>
          <button type="button" className="danger" onClick={() => { setPendingDelete(menu.entry); setMenu(null); }}><Trash2 size={13} /> 删除</button>
        </div>
      )}

      {pendingDelete && (
        <div className="knowledge-confirm-overlay" onMouseDown={() => setPendingDelete(null)}>
          <section className="knowledge-confirm" role="dialog" aria-modal="true" onMouseDown={(event) => event.stopPropagation()}>
            <div className="knowledge-confirm-icon"><Trash2 size={17} /></div>
            <div><h2>删除{pendingDelete.kind === "directory" ? "目录" : "文档"}？</h2><p><strong>{pendingDelete.path}</strong>{pendingDelete.kind === "directory" && " 及其中的全部文档"}将从本地知识库移除。</p></div>
            <footer><button className="button ghost" type="button" onClick={() => setPendingDelete(null)}><X size={13} /> 取消</button><button className="button danger" type="button" onClick={() => void confirmDelete()}><Trash2 size={13} /> 删除</button></footer>
          </section>
        </div>
      )}
    </div>
  );
}

function KnowledgeTreeNode({ node, depth, expanded, selectedPath, onOpen, onToggle, onMenu }: {
  node: TreeNode;
  depth: number;
  expanded: Set<string>;
  selectedPath: string;
  onOpen: (path: string) => Promise<void>;
  onToggle: (path: string) => void;
  onMenu: (menu: EntryMenu) => void;
}) {
  const directory = node.entry.kind === "directory";
  const opened = expanded.has(node.entry.path);
  const select = () => directory ? onToggle(node.entry.path) : void onOpen(node.entry.path);
  const showMenu = (event: MouseEvent) => {
    event.preventDefault();
    event.stopPropagation();
    onMenu({ entry: node.entry, x: event.clientX, y: event.clientY });
  };
  return (
    <div role="treeitem" aria-expanded={directory ? opened : undefined}>
      <div className={node.entry.path === selectedPath ? "knowledge-tree-row selected" : "knowledge-tree-row"} style={{ paddingLeft: 7 + depth * 14 }} role="button" tabIndex={0} onClick={select} onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); select(); } }} onContextMenu={showMenu}>
        {directory ? opened ? <ChevronDown className="tree-chevron" size={11} /> : <ChevronRight className="tree-chevron" size={11} /> : <span className="tree-chevron" />}
        {directory ? opened ? <FolderOpen size={14} /> : <Folder size={14} /> : <FileText size={13} />}
        <span title={node.entry.path}>{node.entry.name}</span>
        <button className="knowledge-row-menu" type="button" aria-label={`${node.entry.name} 菜单`} onClick={showMenu}><MoreHorizontal size={13} /></button>
      </div>
      {directory && opened && node.children.map((child) => <KnowledgeTreeNode key={child.entry.path} node={child} depth={depth + 1} expanded={expanded} selectedPath={selectedPath} onOpen={onOpen} onToggle={onToggle} onMenu={onMenu} />)}
    </div>
  );
}

function buildTree(entries: KnowledgeEntry[]): TreeNode[] {
  const nodes = new Map(entries.map((entry) => [entry.path, { entry, children: [] as TreeNode[] }]));
  const roots: TreeNode[] = [];
  for (const node of nodes.values()) {
    const parent = nodes.get(parentPath(node.entry.path));
    if (parent?.entry.kind === "directory") parent.children.push(node); else roots.push(node);
  }
  const sort = (items: TreeNode[]) => {
    items.sort((left, right) => left.entry.kind === right.entry.kind
      ? left.entry.name.localeCompare(right.entry.name, "zh-CN", { numeric: true })
      : left.entry.kind === "directory" ? -1 : 1);
    items.forEach((item) => sort(item.children));
  };
  sort(roots);
  return roots;
}

function parentPath(path: string): string {
  return path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "";
}

function fileName(path: string): string {
  return path.split("/").at(-1) ?? path;
}

function joinPath(parent: string, name: string): string {
  return parent ? `${parent}/${name}` : name;
}

function resolveMarkdownLink(current: string, href: string): string {
  const pathPart = decodeURIComponent(href.split(/[?#]/, 1)[0] ?? "").replace(/\\/g, "/");
  const parts = pathPart.startsWith("/") ? [] : parentPath(current).split("/").filter(Boolean);
  for (const part of pathPart.replace(/^\/+/, "").split("/")) {
    if (!part || part === ".") continue;
    if (part === "..") parts.pop(); else parts.push(part);
  }
  return parts.join("/");
}

function uniqueBrowserPath(entries: KnowledgeEntry[], directory: string, originalName: string): string {
  const existing = new Set(entries.map((entry) => entry.path.toLocaleLowerCase()));
  const extension = originalName.toLocaleLowerCase().endsWith(".markdown") ? ".markdown" : ".md";
  const stem = originalName.slice(0, -extension.length);
  for (let index = 0; index < 10_000; index += 1) {
    const name = index === 0 ? originalName : `${stem} (${index})${extension}`;
    const candidate = joinPath(directory, name);
    if (!existing.has(candidate.toLocaleLowerCase())) return candidate;
  }
  return joinPath(directory, `${stem}-${Date.now()}${extension}`);
}
