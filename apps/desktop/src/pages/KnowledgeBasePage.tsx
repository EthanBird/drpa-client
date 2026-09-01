import { open } from "@tauri-apps/plugin-dialog";
import {
  BookOpen,
  CheckCircle2,
  ChevronDown,
  CircleAlert,
  Clock3,
  Database,
  FileText,
  Files,
  FolderOpen,
  Globe2,
  Layers3,
  Link2,
  LoaderCircle,
  Plus,
  RefreshCw,
  Search,
  Sparkles,
  Trash2,
  Upload,
  X,
} from "lucide-react";
import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type DragEvent,
  type FormEvent,
  type ReactNode,
} from "react";

import { useAppStore } from "../app/store";
import { SidebarToggle, useSidebarCollapsed } from "../components/SidebarToggle";
import { desktopGateway } from "../infra/gateway";
import "../styles/knowledge-base.css";

type DialogMode = "create" | "text" | "url" | null;

/**
 * These structural contracts intentionally live beside the page until the
 * shared desktop gateway exposes the knowledge-engine API. Keeping the page
 * structurally typed lets the UI compile independently while the native
 * implementation is connected.
 */
interface KnowledgeBaseSummary {
  id: string;
  name: string;
  description: string;
  sourceCount: number;
  chunkCount: number;
  updatedAt: number | string;
}

interface KnowledgeBaseSource {
  id: string;
  knowledgeBaseId: string;
  name: string;
  kind: string;
  status: string;
  chunkCount: number;
  sizeBytes: number;
  uri: string;
  lastError: string;
  updatedAt: number | string;
}

interface KnowledgeBaseSearchResult {
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

interface KnowledgeBaseGateway {
  listKnowledgeBases(): Promise<KnowledgeBaseSummary[]>;
  createKnowledgeBase(name: string, description: string): Promise<KnowledgeBaseSummary>;
  deleteKnowledgeBase(knowledgeBaseId: string): Promise<void>;
  listKnowledgeBaseSources(knowledgeBaseId: string): Promise<KnowledgeBaseSource[]>;
  importKnowledgeBaseFiles(knowledgeBaseId: string, paths: string[]): Promise<KnowledgeBaseSource[]>;
  importKnowledgeBaseDirectory(knowledgeBaseId: string, directoryPath: string): Promise<KnowledgeBaseSource[]>;
  addKnowledgeBaseText(knowledgeBaseId: string, title: string, content: string): Promise<KnowledgeBaseSource>;
  addKnowledgeBaseUrl(knowledgeBaseId: string, url: string): Promise<KnowledgeBaseSource>;
  deleteKnowledgeBaseSource(knowledgeBaseId: string, sourceId: string): Promise<void>;
  searchKnowledgeBase(knowledgeBaseIds: string[], query: string, limit: number): Promise<KnowledgeBaseSearchResult[]>;
}

const knowledgeBaseGateway = desktopGateway as typeof desktopGateway & KnowledgeBaseGateway;

const supportedFilePattern =
  /\.(md|markdown|txt|pdf|doc|docx|xls|xlsx|csv|ppt|pptx|html?|json)$/i;

export function KnowledgeBasePage() {
  const setActiveNavigation = useAppStore((state) => state.setActiveNavigation);
  const librarySidebarCollapsed = useSidebarCollapsed("vector-knowledge-libraries");
  const searchSidebarCollapsed = useSidebarCollapsed("vector-knowledge-search");
  const [libraries, setLibraries] = useState<KnowledgeBaseSummary[]>([]);
  const [activeLibraryId, setActiveLibraryId] = useState("");
  const [searchLibraryIds, setSearchLibraryIds] = useState<Set<string>>(new Set());
  const [sources, setSources] = useState<KnowledgeBaseSource[]>([]);
  const [loadingLibraries, setLoadingLibraries] = useState(true);
  const [loadingSources, setLoadingSources] = useState(false);
  const [libraryLoadError, setLibraryLoadError] = useState("");
  const [sourceLoadError, setSourceLoadError] = useState("");
  const [busy, setBusy] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [notice, setNotice] = useState("正在读取当前工作区的向量知识库…");
  const [noticeTone, setNoticeTone] = useState<"neutral" | "success" | "error">("neutral");
  const [dialog, setDialog] = useState<DialogMode>(null);
  const [dialogError, setDialogError] = useState("");
  const [newLibraryName, setNewLibraryName] = useState("");
  const [newLibraryDescription, setNewLibraryDescription] = useState("");
  const [textTitle, setTextTitle] = useState("");
  const [textContent, setTextContent] = useState("");
  const [sourceUrl, setSourceUrl] = useState("");
  const [query, setQuery] = useState("");
  const [searchLimit, setSearchLimit] = useState(6);
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState("");
  const [searchResults, setSearchResults] = useState<KnowledgeBaseSearchResult[]>([]);
  const [searchHasRun, setSearchHasRun] = useState(false);

  const activeLibrary = useMemo(
    () => libraries.find((library) => library.id === activeLibraryId) ?? null,
    [activeLibraryId, libraries],
  );

  const showDialog = (mode: Exclude<DialogMode, null>) => {
    setDialogError("");
    setDialog(mode);
  };

  const closeDialog = () => {
    if (busy) return;
    setDialogError("");
    setDialog(null);
  };

  const refreshLibraries = useCallback(async (preferredId?: string) => {
    setLoadingLibraries(true);
    setLibraryLoadError("");
    try {
      const next = await knowledgeBaseGateway.listKnowledgeBases();
      setLibraries(next);
      setActiveLibraryId((current) => {
        const retained = preferredId || current;
        return next.some((library) => library.id === retained) ? retained : (next[0]?.id ?? "");
      });
      setSearchLibraryIds((current) => {
        const valid = new Set([...current].filter((id) => next.some((library) => library.id === id)));
        if (valid.size === 0 && next[0]) valid.add(next[0].id);
        return valid;
      });
      setNotice(next.length === 0 ? "尚未创建向量知识库" : `${next.length} 个知识库 · 数据按当前工作区隔离`);
      setNoticeTone("neutral");
      return next;
    } catch (error) {
      const message = `读取知识库失败：${String(error)}`;
      setLibraryLoadError(message);
      setNotice(message);
      setNoticeTone("error");
      return [];
    } finally {
      setLoadingLibraries(false);
    }
  }, []);

  const refreshSources = useCallback(async (libraryId: string) => {
    if (!libraryId) {
      setSources([]);
      return;
    }
    setLoadingSources(true);
    setSourceLoadError("");
    try {
      setSources(await knowledgeBaseGateway.listKnowledgeBaseSources(libraryId));
    } catch (error) {
      const message = `读取知识来源失败：${String(error)}`;
      setSources([]);
      setSourceLoadError(message);
      setNotice(message);
      setNoticeTone("error");
    } finally {
      setLoadingSources(false);
    }
  }, []);

  useEffect(() => {
    void refreshLibraries();
  }, [refreshLibraries]);

  useEffect(() => {
    void refreshSources(activeLibraryId);
  }, [activeLibraryId, refreshSources]);

  useEffect(() => {
    if (!activeLibraryId || !sources.some((source) => ["pending", "indexing"].includes(normalizeStatus(source.status)))) return;
    const timer = window.setTimeout(() => {
      void Promise.all([refreshSources(activeLibraryId), refreshLibraries(activeLibraryId)]);
    }, 2200);
    return () => window.clearTimeout(timer);
  }, [activeLibraryId, refreshLibraries, refreshSources, sources]);

  useEffect(() => {
    if (!dialog) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !busy) closeDialog();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [busy, dialog]);

  const importPaths = useCallback(async (paths: string[], includeDirectories = false) => {
    if (!activeLibraryId || busy) return;
    const acceptedFiles = paths.filter((path) => supportedFilePattern.test(path));
    const possibleDirectories = includeDirectories
      ? paths.filter((path) => !supportedFilePattern.test(path))
      : [];
    if (acceptedFiles.length === 0 && possibleDirectories.length === 0) {
      setNotice("未发现可索引文件；支持 PDF、Word、Excel、PowerPoint、Markdown、文本、CSV、HTML 与 JSON");
      setNoticeTone("error");
      return;
    }
    setBusy(true);
    setNotice(`正在扫描并解析 ${acceptedFiles.length + possibleDirectories.length} 个导入项…`);
    setNoticeTone("neutral");
    try {
      if (acceptedFiles.length > 0) {
        await knowledgeBaseGateway.importKnowledgeBaseFiles(activeLibraryId, acceptedFiles);
      }
      for (const directoryPath of possibleDirectories) {
        await knowledgeBaseGateway.importKnowledgeBaseDirectory(activeLibraryId, directoryPath);
      }
      await Promise.all([refreshSources(activeLibraryId), refreshLibraries(activeLibraryId)]);
      const summary = [
        acceptedFiles.length > 0 ? `${acceptedFiles.length} 个文件` : "",
        possibleDirectories.length > 0 ? `${possibleDirectories.length} 个目录` : "",
      ].filter(Boolean).join("和");
      setNotice(`已导入${summary}，索引任务已提交`);
      setNoticeTone("success");
    } catch (error) {
      setNotice(`导入失败：${String(error)}`);
      setNoticeTone("error");
    } finally {
      setBusy(false);
      setDragging(false);
    }
  }, [activeLibraryId, busy, refreshLibraries, refreshSources]);

  useEffect(() => {
    const handleNativeDrop = (event: Event) => {
      const paths = (event as CustomEvent<{ paths?: string[] }>).detail?.paths ?? [];
      void importPaths(paths, true);
    };
    window.addEventListener("drpa-knowledge-base-file-drop", handleNativeDrop);
    return () => window.removeEventListener("drpa-knowledge-base-file-drop", handleNativeDrop);
  }, [importPaths]);

  const pickFiles = async () => {
    if (!activeLibraryId) return;
    if (!("__TAURI_INTERNALS__" in window)) {
      setNotice("浏览器预览无法取得本地路径；请将文本文件直接拖入，或在桌面应用中选择文件");
      setNoticeTone("neutral");
      return;
    }
    const selected = await open({
      multiple: true,
      directory: false,
      filters: [{
        name: "知识库数据源",
        extensions: [
          "md", "markdown", "txt", "pdf", "doc", "docx", "xls", "xlsx",
          "csv", "ppt", "pptx", "html", "htm", "json",
        ],
      }],
    });
    if (!selected) return;
    await importPaths(Array.isArray(selected) ? selected : [selected]);
  };

  const pickDirectory = async () => {
    if (!activeLibraryId) return;
    if (!("__TAURI_INTERNALS__" in window)) {
      setNotice("目录导入需要桌面应用访问本地文件系统");
      setNoticeTone("neutral");
      return;
    }
    const selected = await open({
      multiple: false,
      directory: true,
      title: "选择需要递归导入的知识目录",
    });
    if (!selected || Array.isArray(selected)) return;
    setBusy(true);
    setNotice(`正在扫描目录 ${fileNameFromPath(selected)}…`);
    setNoticeTone("neutral");
    try {
      await knowledgeBaseGateway.importKnowledgeBaseDirectory(activeLibraryId, selected);
      await Promise.all([refreshSources(activeLibraryId), refreshLibraries(activeLibraryId)]);
      setNotice(`目录“${fileNameFromPath(selected)}”已提交递归索引`);
      setNoticeTone("success");
    } catch (error) {
      setNotice(`目录导入失败：${String(error)}`);
      setNoticeTone("error");
    } finally {
      setBusy(false);
    }
  };

  const dropBrowserFiles = async (event: DragEvent<HTMLElement>) => {
    event.preventDefault();
    setDragging(false);
    if (!activeLibraryId || busy) return;
    const files = Array.from(event.dataTransfer.files).filter((file) => supportedFilePattern.test(file.name));
    const paths = files
      .map((file) => (file as File & { path?: string }).path ?? "")
      .filter(Boolean);
    if (paths.length > 0) {
      await importPaths(paths);
      return;
    }
    const readable = files.filter((file) => /\.(md|markdown|txt|csv|html?|json)$/i.test(file.name));
    if (readable.length === 0) {
      setNotice("浏览器预览只能直接读取文本类文件；PDF 与 Office 文件请在桌面应用中导入");
      setNoticeTone("error");
      return;
    }
    setBusy(true);
    setNotice(`正在导入 ${readable.length} 个文本数据源…`);
    setNoticeTone("neutral");
    try {
      for (const file of readable) {
        await knowledgeBaseGateway.addKnowledgeBaseText(activeLibraryId, file.name, await file.text());
      }
      await Promise.all([refreshSources(activeLibraryId), refreshLibraries(activeLibraryId)]);
      setNotice(`已导入 ${readable.length} 个文本数据源`);
      setNoticeTone("success");
    } catch (error) {
      setNotice(`拖拽导入失败：${String(error)}`);
      setNoticeTone("error");
    } finally {
      setBusy(false);
    }
  };

  const createLibrary = async (event: FormEvent) => {
    event.preventDefault();
    const name = newLibraryName.trim();
    if (!name || busy) return;
    setBusy(true);
    setDialogError("");
    try {
      const created = await knowledgeBaseGateway.createKnowledgeBase(name, newLibraryDescription.trim());
      await refreshLibraries(created.id);
      setSearchLibraryIds((current) => new Set(current).add(created.id));
      setNewLibraryName("");
      setNewLibraryDescription("");
      setDialog(null);
      setNotice(`已创建知识库“${created.name}”`);
      setNoticeTone("success");
    } catch (error) {
      const message = `创建知识库失败：${String(error)}`;
      setDialogError(message);
      setNotice(message);
      setNoticeTone("error");
    } finally {
      setBusy(false);
    }
  };

  const addText = async (event: FormEvent) => {
    event.preventDefault();
    if (!activeLibraryId || !textTitle.trim() || !textContent.trim() || busy) return;
    setBusy(true);
    setDialogError("");
    try {
      await knowledgeBaseGateway.addKnowledgeBaseText(activeLibraryId, textTitle.trim(), textContent);
      await Promise.all([refreshSources(activeLibraryId), refreshLibraries(activeLibraryId)]);
      setTextTitle("");
      setTextContent("");
      setDialog(null);
      setNotice("文本已写入知识库并提交向量索引");
      setNoticeTone("success");
    } catch (error) {
      const message = `添加文本失败：${String(error)}`;
      setDialogError(message);
      setNotice(message);
      setNoticeTone("error");
    } finally {
      setBusy(false);
    }
  };

  const addUrl = async (event: FormEvent) => {
    event.preventDefault();
    if (!activeLibraryId || !sourceUrl.trim() || busy) return;
    setBusy(true);
    setDialogError("");
    try {
      await knowledgeBaseGateway.addKnowledgeBaseUrl(activeLibraryId, sourceUrl.trim());
      await Promise.all([refreshSources(activeLibraryId), refreshLibraries(activeLibraryId)]);
      setSourceUrl("");
      setDialog(null);
      setNotice("URL 内容抓取任务已提交");
      setNoticeTone("success");
    } catch (error) {
      const message = `添加 URL 失败：${String(error)}`;
      setDialogError(message);
      setNotice(message);
      setNoticeTone("error");
    } finally {
      setBusy(false);
    }
  };

  const deleteLibrary = async (library: KnowledgeBaseSummary) => {
    if (busy || !window.confirm(`删除知识库“${library.name}”及其全部向量索引？此操作不会删除原始外部文件。`)) return;
    setBusy(true);
    try {
      await knowledgeBaseGateway.deleteKnowledgeBase(library.id);
      setSearchResults((current) => current.filter((result) => result.knowledgeBaseId !== library.id));
      await refreshLibraries();
      setNotice(`已删除知识库“${library.name}”`);
      setNoticeTone("success");
    } catch (error) {
      setNotice(`删除知识库失败：${String(error)}`);
      setNoticeTone("error");
    } finally {
      setBusy(false);
    }
  };

  const deleteSource = async (source: KnowledgeBaseSource) => {
    if (!activeLibraryId || busy || !window.confirm(`从知识库移除数据源“${source.name}”及其向量片段？`)) return;
    setBusy(true);
    try {
      await knowledgeBaseGateway.deleteKnowledgeBaseSource(activeLibraryId, source.id);
      await Promise.all([refreshSources(activeLibraryId), refreshLibraries(activeLibraryId)]);
      setNotice(`已移除数据源“${source.name}”`);
      setNoticeTone("success");
    } catch (error) {
      setNotice(`移除数据源失败：${String(error)}`);
      setNoticeTone("error");
    } finally {
      setBusy(false);
    }
  };

  const runSearch = async (event: FormEvent) => {
    event.preventDefault();
    const normalized = query.trim();
    if (!normalized || searching || libraries.length === 0) return;
    const ids = searchLibraryIds.size > 0
      ? [...searchLibraryIds]
      : activeLibraryId
        ? [activeLibraryId]
        : [];
    if (ids.length === 0) return;
    setSearching(true);
    setSearchHasRun(true);
    setSearchError("");
    try {
      setSearchResults(await knowledgeBaseGateway.searchKnowledgeBase(ids, normalized, searchLimit));
    } catch (error) {
      const message = `知识查询失败：${String(error)}`;
      setSearchResults([]);
      setSearchError(message);
      setNotice(message);
      setNoticeTone("error");
    } finally {
      setSearching(false);
    }
  };

  const toggleSearchLibrary = (libraryId: string) => {
    setSearchLibraryIds((current) => {
      const next = new Set(current);
      if (next.has(libraryId)) next.delete(libraryId);
      else next.add(libraryId);
      return next;
    });
  };

  const sourceStats = useMemo(() => ({
    ready: sources.filter((source) => normalizeStatus(source.status) === "ready").length,
    processing: sources.filter((source) => ["pending", "indexing"].includes(normalizeStatus(source.status))).length,
    failed: sources.filter((source) => normalizeStatus(source.status) === "failed").length,
    chunks: sources.reduce((total, source) => total + (source.chunkCount || 0), 0),
  }), [sources]);

  return (
    <div
      className="page knowledge-base-page"
      onDragEnter={(event) => { event.preventDefault(); if (activeLibraryId) setDragging(true); }}
      onDragOver={(event) => event.preventDefault()}
      onDragLeave={(event) => {
        const nextTarget = event.relatedTarget;
        if (!(nextTarget instanceof Node) || !event.currentTarget.contains(nextTarget)) setDragging(false);
      }}
      onDrop={(event) => void dropBrowserFiles(event)}
    >
      <header className="page-header knowledge-base-header">
        <div>
          <div className="eyebrow">VECTOR KNOWLEDGE ENGINE</div>
          <h1>知识库</h1>
          <p>将多种数据源切分并向量化，为 AI Agent 提供带引用的本地知识查询。</p>
        </div>
        <button className="button primary" type="button" onClick={() => showDialog("create")}>
          <Plus size={15} /> 新建知识库
        </button>
      </header>

      <div className="knowledge-base-distinction">
        <div className="knowledge-base-distinction-item vectorized">
          <span><Database size={15} /></span>
          <div><strong>知识库</strong><small>多源数据 · 分块 · 向量化 · 供 Agent 检索</small></div>
        </div>
        <div className="knowledge-base-distinction-divider" aria-hidden="true" />
        <div className="knowledge-base-distinction-item documents">
          <span><BookOpen size={15} /></span>
          <div><strong>知识文档</strong><small>可编辑 Markdown 原文 · 不参与向量化</small></div>
          <button className="button ghost small" type="button" onClick={() => setActiveNavigation("docs")}>打开知识文档</button>
        </div>
      </div>

      <div className={`knowledge-base-notice ${noticeTone}`} role="status" aria-live="polite">
        {busy ? <LoaderCircle className="spin" size={13} /> : noticeTone === "error" ? <CircleAlert size={13} /> : <Sparkles size={13} />}
        <span>{notice}</span>
      </div>

      <div className={[
        "knowledge-base-layout",
        librarySidebarCollapsed ? "library-collapsed" : "",
        searchSidebarCollapsed ? "search-collapsed" : "",
      ].filter(Boolean).join(" ")}>
        {librarySidebarCollapsed ? (
          <SidebarToggle id="vector-knowledge-libraries" side="left" label="知识库列表" restore />
        ) : (
          <aside className="knowledge-base-libraries collapsible-sidebar" aria-label="知识库列表">
            <SidebarToggle id="vector-knowledge-libraries" side="left" label="知识库列表" />
            <div className="knowledge-base-sidebar-title">
              <div><Layers3 size={14} /><strong>知识库</strong><span>{libraries.length}</span></div>
              <button type="button" aria-label="刷新知识库" title="刷新知识库" onClick={() => void refreshLibraries(activeLibraryId)} disabled={loadingLibraries}>
                <RefreshCw className={loadingLibraries ? "spin" : ""} size={13} />
              </button>
            </div>
            <div className="knowledge-base-library-list">
              {loadingLibraries && libraries.length === 0 && (
                <div className="knowledge-base-loading-state" aria-label="正在加载知识库">
                  <LoaderCircle className="spin" size={18} />
                  <span>正在加载知识库…</span>
                </div>
              )}
              {libraryLoadError && libraries.length === 0 && (
                <div className="knowledge-base-inline-error">
                  <CircleAlert size={19} />
                  <strong>知识库列表不可用</strong>
                  <p>{libraryLoadError}</p>
                  <button className="button secondary small" type="button" onClick={() => void refreshLibraries()}>
                    <RefreshCw size={12} /> 重试
                  </button>
                </div>
              )}
              {libraries.map((library) => {
                const selectedForSearch = searchLibraryIds.has(library.id);
                return (
                  <article className={`knowledge-base-library-card${library.id === activeLibraryId ? " active" : ""}`} key={library.id}>
                    <button className="knowledge-base-library-main" type="button" onClick={() => setActiveLibraryId(library.id)}>
                      <span className="knowledge-base-library-icon"><Database size={14} /></span>
                      <span>
                        <strong>{library.name}</strong>
                        <small>{library.sourceCount} 个来源 · {library.chunkCount} 个片段</small>
                      </span>
                    </button>
                    <div className="knowledge-base-library-actions">
                      <label title={selectedForSearch ? "从联合查询中移除" : "加入联合查询"}>
                        <input
                          type="checkbox"
                          checked={selectedForSearch}
                          onChange={() => toggleSearchLibrary(library.id)}
                          aria-label={`${selectedForSearch ? "取消查询" : "查询"}知识库 ${library.name}`}
                        />
                        <span>查询</span>
                      </label>
                      <button
                        type="button"
                        aria-label={`删除知识库 ${library.name}`}
                        title="删除知识库"
                        onClick={() => void deleteLibrary(library)}
                        disabled={busy}
                      >
                        <Trash2 size={12} />
                      </button>
                    </div>
                  </article>
                );
              })}
              {!loadingLibraries && !libraryLoadError && libraries.length === 0 && (
                <div className="knowledge-base-sidebar-empty">
                  <Database size={22} />
                  <strong>还没有知识库</strong>
                  <p>创建后即可导入文件、文本或网页。</p>
                </div>
              )}
            </div>
            <button className="knowledge-base-create-shortcut" type="button" onClick={() => showDialog("create")}>
              <Plus size={13} /> 创建知识库
            </button>
          </aside>
        )}

        <main className="knowledge-base-main">
          {activeLibrary ? (
            <>
              <div className="knowledge-base-main-heading">
                <div>
                  <div className="knowledge-base-main-title">
                    <span><Database size={17} /></span>
                    <div>
                      <h2>{activeLibrary.name}</h2>
                      <p>{activeLibrary.description || "当前知识库还没有描述"}</p>
                    </div>
                  </div>
                </div>
                <div className="knowledge-base-source-actions">
                  <button className="button secondary small" type="button" onClick={() => void pickFiles()} disabled={busy}>
                    <Upload size={13} /> 导入文件
                  </button>
                  <button className="button secondary small" type="button" onClick={() => void pickDirectory()} disabled={busy}>
                    <FolderOpen size={13} /> 导入目录
                  </button>
                  <button className="button secondary small" type="button" onClick={() => showDialog("text")} disabled={busy}>
                    <FileText size={13} /> 粘贴文本
                  </button>
                  <button className="button secondary small" type="button" onClick={() => showDialog("url")} disabled={busy}>
                    <Link2 size={13} /> 添加 URL
                  </button>
                </div>
              </div>

              <div className="knowledge-base-stats" aria-label="索引统计">
                <div><span className="ready"><CheckCircle2 size={14} /></span><strong>{sourceStats.ready}</strong><small>已就绪</small></div>
                <div><span className="processing"><Clock3 size={14} /></span><strong>{sourceStats.processing}</strong><small>处理中</small></div>
                <div><span className="failed"><CircleAlert size={14} /></span><strong>{sourceStats.failed}</strong><small>异常</small></div>
                <div><span className="chunks"><Layers3 size={14} /></span><strong>{sourceStats.chunks}</strong><small>向量片段</small></div>
              </div>

              <section className={`knowledge-base-drop-zone${dragging ? " dragging" : ""}`} aria-label="知识库文件导入区">
                <div className="knowledge-base-source-list-heading">
                  <div><Files size={14} /><strong>数据来源</strong><span>{sources.length}</span></div>
                  <button type="button" aria-label="刷新数据来源" title="刷新数据来源" onClick={() => void refreshSources(activeLibraryId)} disabled={loadingSources}>
                    <RefreshCw className={loadingSources ? "spin" : ""} size={13} />
                  </button>
                </div>
                {dragging && (
                  <div className="knowledge-base-drop-prompt">
                    <Upload size={24} />
                    <strong>释放文件或目录以加入“{activeLibrary.name}”</strong>
                    <span>目录会递归扫描；受支持的文件将被解析、分块并建立向量索引</span>
                  </div>
                )}
                {!dragging && (
                  <div className="knowledge-base-source-list">
                    {loadingSources && sources.length === 0 && (
                      <div className="knowledge-base-loading-state source-loading" aria-label="正在加载数据来源">
                        <LoaderCircle className="spin" size={19} />
                        <span>正在读取数据来源与索引状态…</span>
                      </div>
                    )}
                    {sourceLoadError && (
                      <div className="knowledge-base-inline-error source-error">
                        <CircleAlert size={20} />
                        <strong>无法读取数据来源</strong>
                        <p>{sourceLoadError}</p>
                        <button className="button secondary small" type="button" onClick={() => void refreshSources(activeLibraryId)}>
                          <RefreshCw size={12} /> 重试
                        </button>
                      </div>
                    )}
                    {sources.map((source) => (
                      <SourceRow key={source.id} source={source} busy={busy} onDelete={deleteSource} />
                    ))}
                    {!loadingSources && !sourceLoadError && sources.length === 0 && (
                      <div className="knowledge-base-source-empty">
                        <span><Upload size={21} /></span>
                        <h3>拖入第一批知识数据</h3>
                        <p>支持递归导入目录，以及 PDF、Word、Excel、PowerPoint、Markdown、纯文本、CSV、HTML 与 JSON。</p>
                        <div>
                          <button className="button primary small" type="button" onClick={() => void pickFiles()}><Upload size={13} /> 选择文件</button>
                          <button className="button secondary small" type="button" onClick={() => void pickDirectory()}><FolderOpen size={13} /> 选择目录</button>
                          <button className="button secondary small" type="button" onClick={() => showDialog("text")}>粘贴文本</button>
                        </div>
                      </div>
                    )}
                  </div>
                )}
              </section>
            </>
          ) : libraryLoadError ? (
            <div className="knowledge-base-welcome error">
              <div className="knowledge-base-welcome-art"><CircleAlert size={30} /></div>
              <h2>暂时无法打开向量知识库</h2>
              <p>{libraryLoadError}</p>
              <button className="button secondary" type="button" onClick={() => void refreshLibraries()}><RefreshCw size={14} /> 重新加载</button>
            </div>
          ) : (
            <div className="knowledge-base-welcome">
              <div className="knowledge-base-welcome-art" aria-hidden="true">
                <span /><span /><span /><Database size={30} />
              </div>
              <h2>建立 Agent 可查询的向量知识库</h2>
              <p>知识库属于当前工作区。创建后导入多种数据源，DRPA 会保留引用信息并建立混合检索索引。</p>
              <button className="button primary" type="button" onClick={() => showDialog("create")}><Plus size={14} /> 新建知识库</button>
            </div>
          )}
        </main>

        {searchSidebarCollapsed ? (
          <SidebarToggle id="vector-knowledge-search" side="right" label="查询测试" restore />
        ) : (
          <aside className="knowledge-base-search collapsible-sidebar" aria-label="知识库查询测试">
            <SidebarToggle id="vector-knowledge-search" side="right" label="查询测试" />
            <div className="knowledge-base-search-heading">
              <span><Search size={14} /></span>
              <div><strong>查询测试</strong><small>模拟 AI Agent 的知识检索</small></div>
            </div>
            <form className="knowledge-base-search-form" onSubmit={runSearch}>
              <label htmlFor="knowledge-base-query">查询问题</label>
              <textarea
                id="knowledge-base-query"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder="例如：报销流程需要哪些材料？"
                rows={4}
              />
              <div className="knowledge-base-search-scope">
                <span>查询范围</span>
                <strong>{searchLibraryIds.size || (activeLibraryId ? 1 : 0)} 个知识库</strong>
              </div>
              <div className="knowledge-base-search-options">
                <label htmlFor="knowledge-base-result-limit">返回条数</label>
                <select
                  id="knowledge-base-result-limit"
                  value={searchLimit}
                  onChange={(event) => setSearchLimit(Number(event.target.value))}
                >
                  {[3, 6, 10, 15, 20].map((limit) => <option key={limit} value={limit}>{limit} 条</option>)}
                </select>
              </div>
              <button className="button primary" type="submit" disabled={searching || !query.trim() || libraries.length === 0}>
                {searching ? <LoaderCircle className="spin" size={14} /> : <Sparkles size={14} />}
                {searching ? "正在检索…" : "运行查询"}
              </button>
            </form>

            <div className="knowledge-base-search-results" aria-live="polite">
              {searchError && (
                <div className="knowledge-base-inline-error search-error">
                  <CircleAlert size={18} />
                  <strong>查询失败</strong>
                  <p>{searchError}</p>
                </div>
              )}
              {searchResults.map((result, index) => (
                <article className="knowledge-base-search-result" key={`${result.chunkId}-${index}`}>
                  <header>
                    <span className="knowledge-base-result-rank">{index + 1}</span>
                    <div>
                      <strong title={result.sourceName}>{result.sourceName}</strong>
                      <small>{result.knowledgeBaseName}</small>
                    </div>
                    <span className="knowledge-base-result-score" title={`向量 ${formatScore(result.vectorScore)} · 关键词 ${formatScore(result.keywordScore)}`}>
                      {formatScore(result.score)}
                    </span>
                  </header>
                  <p>{result.content}</p>
                  <footer>
                    <span title={result.citation}><FileText size={11} /> {result.citation || `片段 ${result.chunkId}`}</span>
                  </footer>
                  <details className="knowledge-base-citation-detail">
                    <summary><ChevronDown size={12} /> 引用详情</summary>
                    <div>
                      <dl>
                        <div><dt>知识库</dt><dd>{result.knowledgeBaseName}</dd></div>
                        <div><dt>数据来源</dt><dd>{result.sourceName}</dd></div>
                        <div><dt>片段 ID</dt><dd>{result.chunkId}</dd></div>
                        <div><dt>综合相关度</dt><dd>{formatScore(result.score)}</dd></div>
                        <div><dt>向量相关度</dt><dd>{formatScore(result.vectorScore)}</dd></div>
                        <div><dt>关键词相关度</dt><dd>{formatScore(result.keywordScore)}</dd></div>
                      </dl>
                      <label>引用标识</label>
                      <code>{result.citation || `${result.sourceId}#${result.chunkId}`}</code>
                      <label>完整召回片段</label>
                      <p>{result.content}</p>
                    </div>
                  </details>
                </article>
              ))}
              {searchHasRun && !searching && searchResults.length === 0 && (
                <div className="knowledge-base-search-empty">
                  <Search size={20} />
                  <strong>没有找到相关片段</strong>
                  <p>尝试换一种问法，或检查数据源是否已经完成索引。</p>
                </div>
              )}
              {!searchHasRun && (
                <div className="knowledge-base-search-tip">
                  <Sparkles size={18} />
                  <p>勾选左侧一个或多个知识库，输入问题即可检查召回内容、相关度与引用来源。</p>
                </div>
              )}
            </div>
          </aside>
        )}
      </div>

      {dialog === "create" && (
        <Dialog title="新建向量知识库" description="知识库及索引数据将保存在当前工作区。" error={dialogError} busy={busy} onClose={closeDialog}>
          <form className="knowledge-base-dialog-form" onSubmit={createLibrary}>
            <label>
              <span>知识库名称</span>
              <input autoFocus aria-label="知识库名称" value={newLibraryName} onChange={(event) => setNewLibraryName(event.target.value)} placeholder="例如：产品与售后知识" maxLength={80} />
            </label>
            <label>
              <span>用途描述 <small>可选</small></span>
              <textarea aria-label="知识库用途描述" value={newLibraryDescription} onChange={(event) => setNewLibraryDescription(event.target.value)} placeholder="说明内容范围，便于 Agent 选择合适的知识库。" rows={4} maxLength={500} />
            </label>
            <div className="knowledge-base-dialog-actions">
              <button className="button ghost" type="button" onClick={closeDialog} disabled={busy}>取消</button>
              <button className="button primary" type="submit" disabled={busy || !newLibraryName.trim()}>{busy && <LoaderCircle className="spin" size={13} />} 创建知识库</button>
            </div>
          </form>
        </Dialog>
      )}

      {dialog === "text" && (
        <Dialog title="粘贴文本" description={`文本将写入“${activeLibrary?.name ?? ""}”并建立向量索引。`} error={dialogError} busy={busy} onClose={closeDialog}>
          <form className="knowledge-base-dialog-form" onSubmit={addText}>
            <label>
              <span>来源标题</span>
              <input autoFocus aria-label="文本来源标题" value={textTitle} onChange={(event) => setTextTitle(event.target.value)} placeholder="例如：退款政策 2026" maxLength={160} />
            </label>
            <label>
              <span>文本内容</span>
              <textarea className="knowledge-base-content-input" aria-label="知识文本内容" value={textContent} onChange={(event) => setTextContent(event.target.value)} placeholder="粘贴需要提供给 AI Agent 查询的内容…" rows={10} />
            </label>
            <div className="knowledge-base-dialog-actions">
              <button className="button ghost" type="button" onClick={closeDialog} disabled={busy}>取消</button>
              <button className="button primary" type="submit" disabled={busy || !textTitle.trim() || !textContent.trim()}>{busy && <LoaderCircle className="spin" size={13} />} 添加并索引</button>
            </div>
          </form>
        </Dialog>
      )}

      {dialog === "url" && (
        <Dialog title="添加网页 URL" description={`抓取网页正文并写入“${activeLibrary?.name ?? ""}”。`} error={dialogError} busy={busy} onClose={closeDialog}>
          <form className="knowledge-base-dialog-form" onSubmit={addUrl}>
            <label>
              <span>网页地址</span>
              <div className="knowledge-base-url-input"><Globe2 size={14} /><input autoFocus type="url" aria-label="知识来源 URL" value={sourceUrl} onChange={(event) => setSourceUrl(event.target.value)} placeholder="https://example.com/docs" /></div>
            </label>
            <div className="knowledge-base-dialog-hint"><CircleAlert size={13} /><span>仅抓取可公开访问的静态网页内容；登录态页面请先导出为文件。</span></div>
            <div className="knowledge-base-dialog-actions">
              <button className="button ghost" type="button" onClick={closeDialog} disabled={busy}>取消</button>
              <button className="button primary" type="submit" disabled={busy || !sourceUrl.trim()}>{busy && <LoaderCircle className="spin" size={13} />} 抓取并索引</button>
            </div>
          </form>
        </Dialog>
      )}
    </div>
  );
}

function SourceRow({
  source,
  busy,
  onDelete,
}: {
  source: KnowledgeBaseSource;
  busy: boolean;
  onDelete: (source: KnowledgeBaseSource) => Promise<void>;
}) {
  const status = normalizeStatus(source.status);
  const kind = normalizeKind(source.kind);
  const SourceIcon = kind === "url" ? Globe2 : FileText;
  return (
    <article className="knowledge-base-source-row">
      <span className={`knowledge-base-source-icon ${kind}`}><SourceIcon size={16} /></span>
      <div className="knowledge-base-source-identity">
        <strong title={source.uri || source.name}>{source.name}</strong>
        <span>
          <small>{sourceKindLabel(kind)}</small>
          {source.sizeBytes > 0 && <small>{formatBytes(source.sizeBytes)}</small>}
          <small>{formatDate(source.updatedAt)}</small>
        </span>
        {source.lastError && <em title={source.lastError}>{source.lastError}</em>}
      </div>
      <div className="knowledge-base-source-index">
        <span className={`knowledge-base-index-status ${status}`}>
          {status === "ready" ? <CheckCircle2 size={11} /> : status === "failed" ? <CircleAlert size={11} /> : <LoaderCircle className={status === "indexing" ? "spin" : ""} size={11} />}
          {sourceStatusLabel(status)}
        </span>
        {(status === "pending" || status === "indexing") && (
          <span className={`knowledge-base-index-progress ${status}`} role="progressbar" aria-label={`${source.name} ${sourceStatusLabel(status)}`}>
            <i />
          </span>
        )}
        <small>{source.chunkCount || 0} 个片段</small>
      </div>
      <button className="knowledge-base-source-delete" type="button" aria-label={`移除数据源 ${source.name}`} title="移除数据源" disabled={busy} onClick={() => void onDelete(source)}>
        <Trash2 size={13} />
      </button>
    </article>
  );
}

function Dialog({
  title,
  description,
  error,
  busy,
  onClose,
  children,
}: {
  title: string;
  description: string;
  error?: string;
  busy: boolean;
  onClose: () => void;
  children: ReactNode;
}) {
  return (
    <div className="knowledge-base-dialog-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget && !busy) onClose(); }}>
      <section className="knowledge-base-dialog" role="dialog" aria-modal="true" aria-labelledby="knowledge-base-dialog-title">
        <header>
          <div><h2 id="knowledge-base-dialog-title">{title}</h2><p>{description}</p></div>
          <button type="button" aria-label="关闭对话框" onClick={onClose} disabled={busy}><X size={15} /></button>
        </header>
        {error && <div className="knowledge-base-dialog-error" role="alert"><CircleAlert size={14} /><span>{error}</span></div>}
        {children}
      </section>
    </div>
  );
}

function normalizeStatus(status: KnowledgeBaseSource["status"]): "pending" | "indexing" | "ready" | "failed" {
  const normalized = String(status).toLowerCase();
  if (["ready", "completed", "indexed", "success"].includes(normalized)) return "ready";
  if (["failed", "error"].includes(normalized)) return "failed";
  if (["indexing", "processing", "running"].includes(normalized)) return "indexing";
  return "pending";
}

function normalizeKind(kind: KnowledgeBaseSource["kind"]): "file" | "text" | "url" {
  const normalized = String(kind).toLowerCase();
  if (normalized === "url" || normalized === "web" || normalized === "website") return "url";
  if (normalized === "text" || normalized === "pasted-text") return "text";
  return "file";
}

function sourceStatusLabel(status: ReturnType<typeof normalizeStatus>) {
  if (status === "ready") return "已就绪";
  if (status === "indexing") return "索引中";
  if (status === "failed") return "索引失败";
  return "等待处理";
}

function sourceKindLabel(kind: ReturnType<typeof normalizeKind>) {
  if (kind === "url") return "网页";
  if (kind === "text") return "粘贴文本";
  return "文件";
}

function formatBytes(bytes: number) {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const rank = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  return `${(bytes / (1024 ** rank)).toFixed(rank === 0 ? 0 : 1)} ${units[rank]}`;
}

function formatDate(value: number | string) {
  if (!value) return "刚刚更新";
  const date = typeof value === "number"
    ? new Date(value < 10_000_000_000 ? value * 1000 : value)
    : new Date(value);
  if (Number.isNaN(date.getTime())) return "刚刚更新";
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}

function formatScore(score: number) {
  if (!Number.isFinite(score)) return "—";
  const percent = score > 1 ? score : score * 100;
  return `${Math.max(0, Math.min(100, percent)).toFixed(0)}%`;
}

function fileNameFromPath(path: string) {
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}
