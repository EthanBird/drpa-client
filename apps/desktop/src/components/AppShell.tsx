import {
  Blocks,
  Bot,
  Boxes,
  ChevronDown,
  CircleHelp,
  Command,
  Gauge,
  KeyRound,
  Library,
  ListTodo,
  Maximize2,
  Minus,
  PanelLeftClose,
  Play,
  Search,
  Settings,
  Sparkles,
  X,
  Zap,
} from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { PropsWithChildren } from "react";

import type { NavigationId } from "../domain/models";
import { useAppStore } from "../app/store";

const primaryNavigation: Array<{
  id: NavigationId;
  label: string;
  icon: typeof Gauge;
  shortcut?: string;
}> = [
  { id: "overview", label: "Overview", icon: Gauge },
  { id: "library", label: "Library", icon: Library },
  { id: "workbench", label: "Workbench", icon: Blocks, shortcut: "⌘1" },
  { id: "runs", label: "Runs", icon: ListTodo },
  { id: "automations", label: "Automations", icon: Zap },
];

const infrastructureNavigation: Array<{
  id: NavigationId;
  label: string;
  icon: typeof Gauge;
}> = [
  { id: "runtimes", label: "Runtime Center", icon: Boxes },
  { id: "secrets", label: "Secrets", icon: KeyRound },
];

export function AppShell({ children }: PropsWithChildren) {
  const activeNavigation = useAppStore((state) => state.activeNavigation);
  const setActiveNavigation = useAppStore((state) => state.setActiveNavigation);
  const setCommandOpen = useAppStore((state) => state.setCommandOpen);
  const compactMode = useAppStore((state) => state.compactMode);
  const toggleCompactMode = useAppStore((state) => state.toggleCompactMode);
  const inDesktopHost = "__TAURI_INTERNALS__" in window;

  const controlWindow = (action: "minimize" | "maximize" | "close") => {
    if (!inDesktopHost) return;
    const appWindow = getCurrentWindow();
    if (action === "minimize") void appWindow.minimize();
    if (action === "maximize") void appWindow.toggleMaximize();
    if (action === "close") void appWindow.close();
  };

  return (
    <div className="shell">
      <header className="titlebar" data-tauri-drag-region>
        <div className="brand-lockup">
          <div className="brand-mark" aria-hidden="true">
            <Bot size={17} strokeWidth={2.1} />
          </div>
          <span className="brand-name">DRPA</span>
          <span className="release-chip">NEXT</span>
        </div>
        <button className="workspace-switcher" type="button">
          <span className="workspace-dot" />
          Personal workspace
          <ChevronDown size={14} />
        </button>
        <div className="titlebar-spacer" />
        <button className="sync-state" type="button" aria-label="Runtime status">
          <span className="pulse-dot" />
          Runtime healthy
        </button>
        <div className="window-controls" aria-label="Window controls">
          <button type="button" aria-label="Minimize" onClick={() => controlWindow("minimize")}><Minus size={14} /></button>
          <button type="button" aria-label="Maximize" onClick={() => controlWindow("maximize")}><Maximize2 size={13} /></button>
          <button type="button" aria-label="Close" onClick={() => controlWindow("close")}><X size={14} /></button>
        </div>
      </header>

      <aside className="sidebar">
        <button className="command-trigger" type="button" onClick={() => setCommandOpen(true)}>
          <Search size={15} />
          <span>Search or run a command</span>
          <kbd>⌘ K</kbd>
        </button>

        <nav aria-label="Primary navigation">
          <NavigationGroup
            items={primaryNavigation}
            activeId={activeNavigation}
            onSelect={setActiveNavigation}
          />
          <div className="nav-label">Infrastructure</div>
          <NavigationGroup
            items={infrastructureNavigation}
            activeId={activeNavigation}
            onSelect={setActiveNavigation}
          />
        </nav>

        <div className="sidebar-spacer" />
        <div className="quick-run-card">
          <div className="quick-run-icon"><Sparkles size={16} /></div>
          <div>
            <strong>Quick run</strong>
            <span>Drop a package or Python file</span>
          </div>
          <Play size={14} fill="currentColor" />
        </div>
        <div className="sidebar-utility">
          <button type="button"><CircleHelp size={16} /><span>Help & diagnostics</span></button>
          <button type="button"><Settings size={16} /><span>Settings</span></button>
          <button type="button" onClick={toggleCompactMode} aria-pressed={compactMode}>
            <PanelLeftClose size={16} />
            <span>{compactMode ? "Comfortable density" : "Compact density"}</span>
          </button>
        </div>
        <div className="account-card">
          <div className="avatar">EB</div>
          <div><strong>Ethan Bird</strong><span>Administrator</span></div>
          <ChevronDown size={14} />
        </div>
      </aside>

      <main className="main-content">{children}</main>
    </div>
  );
}

interface NavigationGroupProps {
  items: Array<{ id: NavigationId; label: string; icon: typeof Gauge; shortcut?: string }>;
  activeId: NavigationId;
  onSelect: (id: NavigationId) => void;
}

function NavigationGroup({ items, activeId, onSelect }: NavigationGroupProps) {
  return (
    <div className="nav-group">
      {items.map((item) => {
        const Icon = item.icon;
        return (
          <button
            key={item.id}
            className={activeId === item.id ? "nav-item active" : "nav-item"}
            type="button"
            onClick={() => onSelect(item.id)}
          >
            <Icon size={16} strokeWidth={1.9} />
            <span>{item.label}</span>
            {item.shortcut && <kbd>{item.shortcut}</kbd>}
          </button>
        );
      })}
    </div>
  );
}
