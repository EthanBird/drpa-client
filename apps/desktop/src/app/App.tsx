import { lazy, Suspense, useEffect, useRef } from "react";

import { desktopGateway } from "../infra/gateway";
import { AppShell } from "../components/AppShell";
import { CommandPalette } from "../components/CommandPalette";
import { OverviewPage } from "../pages/OverviewPage";
import { useAppStore } from "./store";

const AgentPage = lazy(() => import("../pages/AgentPage").then((module) => ({ default: module.AgentPage })));
const AutomationsPage = lazy(() => import("../pages/AutomationsPage").then((module) => ({ default: module.AutomationsPage })));
const DocsPage = lazy(() => import("../pages/DocsPage").then((module) => ({ default: module.DocsPage })));
const ExtensionToolsPage = lazy(() => import("../pages/ExtensionToolsPage").then((module) => ({ default: module.ExtensionToolsPage })));
const LibraryPage = lazy(() => import("../pages/LibraryPage").then((module) => ({ default: module.LibraryPage })));
const PluginsPage = lazy(() => import("../pages/PluginsPage").then((module) => ({ default: module.PluginsPage })));
const PlaceholderPage = lazy(() => import("../pages/PlaceholderPage").then((module) => ({ default: module.PlaceholderPage })));
const RunsPage = lazy(() => import("../pages/RunsPage").then((module) => ({ default: module.RunsPage })));
const RuntimePage = lazy(() => import("../pages/RuntimePage").then((module) => ({ default: module.RuntimePage })));
const SettingsPage = lazy(() => import("../pages/SettingsPage").then((module) => ({ default: module.SettingsPage })));
const StudioPage = lazy(() => import("../pages/StudioPage").then((module) => ({ default: module.StudioPage })));
const DataPage = lazy(() => import("../pages/DataPage").then((module) => ({ default: module.DataPage })));
const LocalDifyPage = lazy(() => import("../pages/LocalDifyPage").then((module) => ({ default: module.LocalDifyPage })));
const KnowledgeBasePage = lazy(() => import("../pages/KnowledgeBasePage").then((module) => ({ default: module.KnowledgeBasePage })));
const WorkbenchPage = lazy(() => import("../pages/WorkbenchPage").then((module) => ({ default: module.WorkbenchPage })));

function PageFallback() {
  return <div className="page"><div className="empty-state"><h2>正在加载工作台…</h2></div></div>;
}

function waitForTwoPaints(): Promise<void> {
  const schedule = (callback: FrameRequestCallback) => {
    if (typeof window.requestAnimationFrame === "function") return window.requestAnimationFrame(callback);
    return window.setTimeout(() => callback(performance.now()), 0);
  };
  return new Promise((resolve) => {
    let settled = false;
    const finish = () => {
      if (settled) return;
      settled = true;
      window.clearTimeout(fallback);
      resolve();
    };
    // WebView2 can suppress requestAnimationFrame while its native window is hidden.
    // Keep the paint path for visible/dev previews, but never let startup deadlock on it.
    const fallback = window.setTimeout(finish, document.visibilityState === "hidden" ? 80 : 400);
    schedule(() => schedule(finish));
  });
}

export function App() {
  const inputSmokeArmed = useRef(false);
  const activeNavigation = useAppStore((state) => state.activeNavigation);
  const commandOpen = useAppStore((state) => state.commandOpen);
  const theme = useAppStore((state) => state.theme);
  const fontScale = useAppStore((state) => state.fontScale);
  const language = useAppStore((state) => state.language);
  const uiDensity = useAppStore((state) => state.uiDensity);
  const hidePageHeaders = useAppStore((state) => state.hidePageHeaders);
  const dragActive = useAppStore((state) => state.dragActive);
  const operationNotice = useAppStore((state) => state.operationNotice);
  const setCommandOpen = useAppStore((state) => state.setCommandOpen);
  const setActiveNavigation = useAppStore((state) => state.setActiveNavigation);
  const selectPackage = useAppStore((state) => state.selectPackage);
  const setDragActive = useAppStore((state) => state.setDragActive);
  const setOperationNotice = useAppStore((state) => state.setOperationNotice);
  const setSnapshot = useAppStore((state) => state.setSnapshot);

  useEffect(() => {
    let disposed = false;
    void desktopGateway.getWorkspaceSnapshot()
      .then(async (snapshot) => {
        if (disposed) return;
        setSnapshot(snapshot);
        await waitForTwoPaints();
        if (!disposed) {
          await desktopGateway.reportUiReady();
          await desktopGateway.completeStartup();
        }
      })
      .catch((error: unknown) => {
        if (!disposed) {
          setOperationNotice(`桌面初始化失败：${String(error)}`);
          void desktopGateway.completeStartup();
        }
      });
    return () => { disposed = true; };
  }, [setOperationNotice, setSnapshot]);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    document.documentElement.style.colorScheme = theme;
  }, [theme]);

  useEffect(() => {
    document.documentElement.dataset.fontScale = String(fontScale);
    document.documentElement.style.setProperty("--font-scale", String(fontScale / 100));
  }, [fontScale]);

  useEffect(() => {
    document.documentElement.lang = language;
  }, [language]);

  useEffect(() => {
    document.documentElement.dataset.uiDensity = uiDensity;
  }, [uiDensity]);

  useEffect(() => {
    document.documentElement.dataset.hidePageHeaders = String(hidePageHeaders);
  }, [hidePageHeaders]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setCommandOpen(!commandOpen);
      }
      if (event.key === "Escape") {
        setCommandOpen(false);
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [commandOpen, setCommandOpen]);

  useEffect(() => {
    const handleInput = (event: Event) => {
      const target = event.target;
      if (
        event.isTrusted
        && target instanceof HTMLInputElement
        && target.value.length > 0
      ) inputSmokeArmed.current = true;
    };
    const handlePointerDown = (event: PointerEvent) => {
      if (!event.isTrusted || !inputSmokeArmed.current) return;
      inputSmokeArmed.current = false;
      window.setTimeout(() => { void desktopGateway.reportUiInputReady(); }, 300);
    };
    document.addEventListener("input", handleInput);
    document.addEventListener("pointerdown", handlePointerDown, true);
    return () => {
      document.removeEventListener("input", handleInput);
      document.removeEventListener("pointerdown", handlePointerDown, true);
    };
  }, []);

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/window")
      .then(({ getCurrentWindow }) => getCurrentWindow().onDragDropEvent(async (event) => {
        if (event.payload.type === "enter" || event.payload.type === "over") {
          setDragActive(true);
          return;
        }
        if (event.payload.type === "leave") {
          setDragActive(false);
          return;
        }
        setDragActive(false);
        if (activeNavigation === "studio") {
          window.dispatchEvent(new CustomEvent("drpa-studio-file-drop", { detail: { paths: event.payload.paths } }));
          setOperationNotice(`已将 ${event.payload.paths.length} 个文件交给开发工作室`);
          return;
        }
        if (activeNavigation === "docs") {
          window.dispatchEvent(new CustomEvent("drpa-knowledge-file-drop", { detail: { paths: event.payload.paths } }));
          setOperationNotice(`已将 ${event.payload.paths.length} 个文件交给知识文档`);
          return;
        }
        if (activeNavigation === "knowledgeBase") {
          window.dispatchEvent(new CustomEvent("drpa-knowledge-base-file-drop", { detail: { paths: event.payload.paths } }));
          setOperationNotice(`已将 ${event.payload.paths.length} 个文件交给向量知识库`);
          return;
        }
        if (activeNavigation === "agent") {
          window.dispatchEvent(new CustomEvent("drpa-agent-document-drop", { detail: { paths: event.payload.paths } }));
          setOperationNotice(`已将 ${event.payload.paths.length} 个文档交给 AI Agent`);
          return;
        }
        if (activeNavigation === "data") {
          window.dispatchEvent(new CustomEvent("drpa-data-file-drop", { detail: { paths: event.payload.paths } }));
          setOperationNotice(`已将 ${event.payload.paths.length} 个文件交给数据工作台`);
          return;
        }
        const archives = event.payload.paths.filter((path) => path.toLowerCase().endsWith(".rpaz"));
        if (archives.length === 0) {
          setOperationNotice("拖入的文件不是 .rpaz RPAZ 包");
          return;
        }
        try {
          let installed;
          for (const archive of archives) installed = await desktopGateway.installPackage(archive);
          setSnapshot(await desktopGateway.getWorkspaceSnapshot());
          if (installed) selectPackage(installed.id, installed.profiles[0]?.id);
          setActiveNavigation("library");
          setOperationNotice(`已通过拖拽安装 ${archives.length} 个 RPAZ 包`);
        } catch (error) {
          setOperationNotice(`拖拽安装失败：${String(error)}`);
        }
      }))
      .then((stop) => { if (disposed) stop(); else unlisten = stop; })
      .catch((error: unknown) => setOperationNotice(`无法启用拖拽安装：${String(error)}`));
    return () => { disposed = true; unlisten?.(); };
  }, [activeNavigation, selectPackage, setActiveNavigation, setDragActive, setOperationNotice, setSnapshot]);

  return (
    <div className="app">
      <AppShell>
        {activeNavigation === "overview" && <OverviewPage />}
        {activeNavigation === "library" && <Suspense fallback={<PageFallback />}><LibraryPage /></Suspense>}
        {activeNavigation === "studio" && <Suspense fallback={<PageFallback />}><StudioPage /></Suspense>}
        {activeNavigation === "data" && <Suspense fallback={<PageFallback />}><DataPage /></Suspense>}
        {activeNavigation === "workbench" && <Suspense fallback={<PageFallback />}><WorkbenchPage /></Suspense>}
        {activeNavigation === "runs" && <Suspense fallback={<PageFallback />}><RunsPage /></Suspense>}
        {activeNavigation === "automations" && <Suspense fallback={<PageFallback />}><AutomationsPage /></Suspense>}
        {activeNavigation === "localDify" && <Suspense fallback={<PageFallback />}><LocalDifyPage /></Suspense>}
        {activeNavigation === "agent" && <Suspense fallback={<PageFallback />}><AgentPage /></Suspense>}
        {activeNavigation === "extensionTools" && <Suspense fallback={<PageFallback />}><ExtensionToolsPage /></Suspense>}
        {activeNavigation === "plugins" && <Suspense fallback={<PageFallback />}><PluginsPage /></Suspense>}
        {activeNavigation === "docs" && <Suspense fallback={<PageFallback />}><DocsPage /></Suspense>}
        {activeNavigation === "knowledgeBase" && <Suspense fallback={<PageFallback />}><KnowledgeBasePage /></Suspense>}
        {activeNavigation === "runtimes" && <Suspense fallback={<PageFallback />}><RuntimePage /></Suspense>}
        {activeNavigation === "secrets" && (
          <Suspense fallback={<PageFallback />}><PlaceholderPage
            eyebrow="敏感数据保护"
            title="凭据保险箱"
            description="任务只绑定凭据引用；真实值不会进入 RPAZ 包、运行历史、日志或命令行。"
          /></Suspense>
        )}
        {activeNavigation === "settings" && <Suspense fallback={<PageFallback />}><SettingsPage /></Suspense>}
      </AppShell>
      {dragActive && <div className="drop-overlay"><div><strong>{activeNavigation === "docs" ? "释放以导入 Markdown" : activeNavigation === "knowledgeBase" ? "释放以索引到向量知识库" : activeNavigation === "agent" ? "释放以附加到当前对话" : activeNavigation === "studio" ? "释放以添加项目文件" : activeNavigation === "data" ? "释放以创建文件数据源" : "释放以安装 RPAZ"}</strong><span>{activeNavigation === "docs" ? "支持同时导入多个 `.md` / `.markdown` 文档" : activeNavigation === "knowledgeBase" ? "支持 PDF、Word、Excel、PowerPoint 与文本资料" : activeNavigation === "agent" ? "支持 PDF、DOCX、XLSX 与 PPTX" : activeNavigation === "studio" ? "文件将添加到当前项目目录" : activeNavigation === "data" ? "支持 SQLite、XLS、XLSX、XLSB 与 ODS" : "支持同时拖入多个 `.rpaz` RPAZ 包"}</span></div></div>}
      {operationNotice && <button className="global-notice" type="button" onClick={() => setOperationNotice("")}>{operationNotice}<span>×</span></button>}
      {commandOpen && <CommandPalette onClose={() => setCommandOpen(false)} />}
    </div>
  );
}
