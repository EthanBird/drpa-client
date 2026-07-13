import { lazy, Suspense, useEffect } from "react";

import { desktopGateway } from "../infra/gateway";
import { AppShell } from "../components/AppShell";
import { CommandPalette } from "../components/CommandPalette";
import { LibraryPage } from "../pages/LibraryPage";
import { OverviewPage } from "../pages/OverviewPage";
import { PlaceholderPage } from "../pages/PlaceholderPage";
import { RunsPage } from "../pages/RunsPage";
import { RuntimePage } from "../pages/RuntimePage";
import { SettingsPage } from "../pages/SettingsPage";
import { WorkbenchPage } from "../pages/WorkbenchPage";
import { useAppStore } from "./store";

const StudioPage = lazy(() => import("../pages/StudioPage").then((module) => ({ default: module.StudioPage })));

export function App() {
  const activeNavigation = useAppStore((state) => state.activeNavigation);
  const commandOpen = useAppStore((state) => state.commandOpen);
  const compactMode = useAppStore((state) => state.compactMode);
  const dragActive = useAppStore((state) => state.dragActive);
  const operationNotice = useAppStore((state) => state.operationNotice);
  const setCommandOpen = useAppStore((state) => state.setCommandOpen);
  const setActiveNavigation = useAppStore((state) => state.setActiveNavigation);
  const selectPackage = useAppStore((state) => state.selectPackage);
  const setDragActive = useAppStore((state) => state.setDragActive);
  const setOperationNotice = useAppStore((state) => state.setOperationNotice);
  const setSnapshot = useAppStore((state) => state.setSnapshot);

  useEffect(() => {
    void desktopGateway.getWorkspaceSnapshot().then(setSnapshot);
  }, [setSnapshot]);

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
        const archives = event.payload.paths.filter((path) => path.toLowerCase().endsWith(".rpaz"));
        if (archives.length === 0) {
          setOperationNotice("拖入的文件不是 .rpaz 脚本包");
          return;
        }
        try {
          let installed;
          for (const archive of archives) installed = await desktopGateway.installPackage(archive);
          setSnapshot(await desktopGateway.getWorkspaceSnapshot());
          if (installed) selectPackage(installed.id, installed.profiles[0]?.id);
          setActiveNavigation("library");
          setOperationNotice(`已通过拖拽安装 ${archives.length} 个脚本包`);
        } catch (error) {
          setOperationNotice(`拖拽安装失败：${String(error)}`);
        }
      }))
      .then((stop) => { if (disposed) stop(); else unlisten = stop; })
      .catch((error: unknown) => setOperationNotice(`无法启用拖拽安装：${String(error)}`));
    return () => { disposed = true; unlisten?.(); };
  }, [selectPackage, setActiveNavigation, setDragActive, setOperationNotice, setSnapshot]);

  return (
    <div className={compactMode ? "app density-compact" : "app"}>
      <AppShell>
        {activeNavigation === "overview" && <OverviewPage />}
        {activeNavigation === "library" && <LibraryPage />}
        {activeNavigation === "studio" && <Suspense fallback={<div className="page"><div className="empty-state"><h2>正在加载开发工作室…</h2></div></div>}><StudioPage /></Suspense>}
        {activeNavigation === "workbench" && <WorkbenchPage />}
        {activeNavigation === "runs" && <RunsPage />}
        {activeNavigation === "automations" && (
          <PlaceholderPage
            eyebrow="任务编排"
            title="自动化计划"
            description="在本地配置定时、文件触发与 Webhook，同时保持离线可用。"
          />
        )}
        {activeNavigation === "runtimes" && <RuntimePage />}
        {activeNavigation === "secrets" && (
          <PlaceholderPage
            eyebrow="敏感数据保护"
            title="凭据保险箱"
            description="任务只绑定凭据引用；真实值不会进入脚本包、运行历史、日志或命令行。"
          />
        )}
        {activeNavigation === "settings" && <SettingsPage />}
      </AppShell>
      {dragActive && <div className="drop-overlay"><div><strong>释放以安装 RPAZ</strong><span>支持同时拖入多个 `.rpaz` 脚本包</span></div></div>}
      {operationNotice && <button className="global-notice" type="button" onClick={() => setOperationNotice("")}>{operationNotice}<span>×</span></button>}
      {commandOpen && <CommandPalette onClose={() => setCommandOpen(false)} />}
    </div>
  );
}
