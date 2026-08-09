import { lazy, Suspense, useEffect, useRef } from "react";

import { desktopGateway } from "../infra/gateway";
import { AppShell } from "../components/AppShell";
import { CommandPalette } from "../components/CommandPalette";
import type { NavigationId } from "../domain/models";
import { OverviewPage } from "../pages/OverviewPage";
import { NavigationSurface } from "./NavigationSurface";
import { useAppStore } from "./store";

const AgentPage = lazy(() => import("../pages/AgentPage").then((module) => ({ default: module.AgentPage })));
const AutomationsPage = lazy(() => import("../pages/AutomationsPage").then((module) => ({ default: module.AutomationsPage })));
const DocsPage = lazy(() => import("../pages/DocsPage").then((module) => ({ default: module.DocsPage })));
const ExtensionToolsPage = lazy(() => import("../pages/ExtensionToolsPage").then((module) => ({ default: module.ExtensionToolsPage })));
const LibraryPage = lazy(() => import("../pages/LibraryPage").then((module) => ({ default: module.LibraryPage })));
const PluginsPage = lazy(() => import("../pages/PluginsPage").then((module) => ({ default: module.PluginsPage })));
const RunsPage = lazy(() => import("../pages/RunsPage").then((module) => ({ default: module.RunsPage })));
const RuntimePage = lazy(() => import("../pages/RuntimePage").then((module) => ({ default: module.RuntimePage })));
const SecretsPage = lazy(() => import("../pages/SecretsPage").then((module) => ({ default: module.SecretsPage })));
const SettingsPage = lazy(() => import("../pages/SettingsPage").then((module) => ({ default: module.SettingsPage })));
const StudioPage = lazy(() => import("../pages/StudioPage").then((module) => ({ default: module.StudioPage })));
const DataPage = lazy(() => import("../pages/DataPage").then((module) => ({ default: module.DataPage })));
const LocalDifyPage = lazy(() => import("../pages/LocalDifyPage").then((module) => ({ default: module.LocalDifyPage })));
const KnowledgeBasePage = lazy(() => import("../pages/KnowledgeBasePage").then((module) => ({ default: module.KnowledgeBasePage })));
const WorkbenchPage = lazy(() => import("../pages/WorkbenchPage").then((module) => ({ default: module.WorkbenchPage })));

// These workspaces own drafts that are intentionally kept in React memory. List/status
// pages are remounted on demand so WebKitGTK does not accumulate a hidden application
// tree after every navigation. This matters on the UOS software-rendering profile.
const RETAINED_NAVIGATIONS = new Set<NavigationId>(["overview", "studio", "docs"]);

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
  const retainedNavigations = useRef<Set<NavigationId>>(new Set());
  if (RETAINED_NAVIGATIONS.has(activeNavigation)) retainedNavigations.current.add(activeNavigation);
  const shouldMountNavigation = (id: NavigationId) => id === activeNavigation || retainedNavigations.current.has(id);
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
    let disposed = false;
    void desktopGateway.getPlatformCapabilities()
      .then((capabilities) => {
        if (!disposed) {
          document.documentElement.dataset.reducedVisualEffects = String(capabilities.reducedVisualEffects);
        }
      })
      .catch(() => {
        if (!disposed) document.documentElement.dataset.reducedVisualEffects = "false";
      });
    return () => { disposed = true; };
  }, []);

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
        {shouldMountNavigation("overview") && <NavigationSurface id="overview" activeId={activeNavigation}><OverviewPage /></NavigationSurface>}
        {shouldMountNavigation("library") && <NavigationSurface id="library" activeId={activeNavigation}><Suspense fallback={<PageFallback />}><LibraryPage /></Suspense></NavigationSurface>}
        {shouldMountNavigation("studio") && <NavigationSurface id="studio" activeId={activeNavigation}><Suspense fallback={<PageFallback />}><StudioPage /></Suspense></NavigationSurface>}
        {shouldMountNavigation("data") && <NavigationSurface id="data" activeId={activeNavigation}><Suspense fallback={<PageFallback />}><DataPage /></Suspense></NavigationSurface>}
        {shouldMountNavigation("workbench") && <NavigationSurface id="workbench" activeId={activeNavigation}><Suspense fallback={<PageFallback />}><WorkbenchPage /></Suspense></NavigationSurface>}
        {shouldMountNavigation("runs") && <NavigationSurface id="runs" activeId={activeNavigation}><Suspense fallback={<PageFallback />}><RunsPage /></Suspense></NavigationSurface>}
        {shouldMountNavigation("automations") && <NavigationSurface id="automations" activeId={activeNavigation}><Suspense fallback={<PageFallback />}><AutomationsPage /></Suspense></NavigationSurface>}
        {shouldMountNavigation("localDify") && <NavigationSurface id="localDify" activeId={activeNavigation}><Suspense fallback={<PageFallback />}><LocalDifyPage /></Suspense></NavigationSurface>}
        {shouldMountNavigation("agent") && <NavigationSurface id="agent" activeId={activeNavigation}><Suspense fallback={<PageFallback />}><AgentPage /></Suspense></NavigationSurface>}
        {shouldMountNavigation("extensionTools") && <NavigationSurface id="extensionTools" activeId={activeNavigation}><Suspense fallback={<PageFallback />}><ExtensionToolsPage /></Suspense></NavigationSurface>}
        {shouldMountNavigation("plugins") && <NavigationSurface id="plugins" activeId={activeNavigation}><Suspense fallback={<PageFallback />}><PluginsPage /></Suspense></NavigationSurface>}
        {shouldMountNavigation("docs") && <NavigationSurface id="docs" activeId={activeNavigation}><Suspense fallback={<PageFallback />}><DocsPage /></Suspense></NavigationSurface>}
        {shouldMountNavigation("knowledgeBase") && <NavigationSurface id="knowledgeBase" activeId={activeNavigation}><Suspense fallback={<PageFallback />}><KnowledgeBasePage /></Suspense></NavigationSurface>}
        {shouldMountNavigation("runtimes") && <NavigationSurface id="runtimes" activeId={activeNavigation}><Suspense fallback={<PageFallback />}><RuntimePage /></Suspense></NavigationSurface>}
        {shouldMountNavigation("secrets") && <NavigationSurface id="secrets" activeId={activeNavigation}><Suspense fallback={<PageFallback />}><SecretsPage /></Suspense></NavigationSurface>}
        {shouldMountNavigation("settings") && <NavigationSurface id="settings" activeId={activeNavigation}><Suspense fallback={<PageFallback />}><SettingsPage /></Suspense></NavigationSurface>}
      </AppShell>
      {dragActive && <div className="drop-overlay"><div><strong>{activeNavigation === "docs" ? "释放以导入 Markdown" : activeNavigation === "knowledgeBase" ? "释放以索引到向量知识库" : activeNavigation === "agent" ? "释放以附加到当前对话" : activeNavigation === "studio" ? "释放以添加项目文件" : activeNavigation === "data" ? "释放以创建文件数据源" : "释放以安装 RPAZ"}</strong><span>{activeNavigation === "docs" ? "支持同时导入多个 `.md` / `.markdown` 文档" : activeNavigation === "knowledgeBase" ? "支持 PDF、Word、Excel、PowerPoint 与文本资料" : activeNavigation === "agent" ? "支持 PDF、DOCX、XLSX 与 PPTX" : activeNavigation === "studio" ? "文件将添加到当前项目目录" : activeNavigation === "data" ? "支持 SQLite、XLS、XLSX、XLSB 与 ODS" : "支持同时拖入多个 `.rpaz` RPAZ 包"}</span></div></div>}
      {operationNotice && <button className="global-notice" type="button" onClick={() => setOperationNotice("")}>{operationNotice}<span>×</span></button>}
      {commandOpen && <CommandPalette onClose={() => setCommandOpen(false)} />}
    </div>
  );
}
