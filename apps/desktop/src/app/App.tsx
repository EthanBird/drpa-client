import { useEffect } from "react";

import { desktopGateway } from "../infra/gateway";
import { AppShell } from "../components/AppShell";
import { CommandPalette } from "../components/CommandPalette";
import { LibraryPage } from "../pages/LibraryPage";
import { OverviewPage } from "../pages/OverviewPage";
import { PlaceholderPage } from "../pages/PlaceholderPage";
import { RunsPage } from "../pages/RunsPage";
import { WorkbenchPage } from "../pages/WorkbenchPage";
import { useAppStore } from "./store";

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
        {activeNavigation === "workbench" && <WorkbenchPage />}
        {activeNavigation === "runs" && <RunsPage />}
        {activeNavigation === "automations" && (
          <PlaceholderPage
            eyebrow="Orchestration"
            title="Automations"
            description="Schedules, file triggers and webhooks will live here without turning the desktop client into a server dependency."
          />
        )}
        {activeNavigation === "runtimes" && (
          <PlaceholderPage
            eyebrow="Runtime infrastructure"
            title="Runtime Center"
            description="Inspect immutable environments, runtime health, cache use and package compatibility from one place."
          />
        )}
        {activeNavigation === "secrets" && (
          <PlaceholderPage
            eyebrow="Protected data"
            title="Secrets"
            description="Profiles bind to secret references. Values stay out of package manifests, run history, logs and command arguments."
          />
        )}
      </AppShell>
      {commandOpen && <CommandPalette onClose={() => setCommandOpen(false)} />}
    </div>
  );
}
