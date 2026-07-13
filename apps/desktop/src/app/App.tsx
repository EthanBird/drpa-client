import { lazy, Suspense, useEffect } from "react";

import { desktopGateway } from "../infra/gateway";
import { AppShell } from "../components/AppShell";
import { CommandPalette } from "../components/CommandPalette";
import { LibraryPage } from "../pages/LibraryPage";
import { OverviewPage } from "../pages/OverviewPage";
import { PlaceholderPage } from "../pages/PlaceholderPage";
import { RunsPage } from "../pages/RunsPage";
import { SettingsPage } from "../pages/SettingsPage";
import { WorkbenchPage } from "../pages/WorkbenchPage";
import { useAppStore } from "./store";

const StudioPage = lazy(() => import("../pages/StudioPage").then((module) => ({ default: module.StudioPage })));

export function App() {
  const activeNavigation = useAppStore((state) => state.activeNavigation);
  const commandOpen = useAppStore((state) => state.commandOpen);
  const compactMode = useAppStore((state) => state.compactMode);
  const setCommandOpen = useAppStore((state) => state.setCommandOpen);
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
        {activeNavigation === "runtimes" && (
          <PlaceholderPage
            eyebrow="运行基础设施"
            title="运行环境"
            description="集中查看不可变 Python 环境、健康状态、缓存占用和脚本包兼容性。"
          />
        )}
        {activeNavigation === "secrets" && (
          <PlaceholderPage
            eyebrow="敏感数据保护"
            title="凭据保险箱"
            description="任务只绑定凭据引用；真实值不会进入脚本包、运行历史、日志或命令行。"
          />
        )}
        {activeNavigation === "settings" && <SettingsPage />}
      </AppShell>
      {commandOpen && <CommandPalette onClose={() => setCommandOpen(false)} />}
    </div>
  );
}
