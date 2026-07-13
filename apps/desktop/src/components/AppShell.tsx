import {
  Blocks,
  Bot,
  Boxes,
  ChevronDown,
  CircleHelp,
  Code2,
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
  { id: "overview", label: "总览", icon: Gauge },
  { id: "library", label: "脚本包", icon: Library },
  { id: "studio", label: "开发工作室", icon: Code2 },
  { id: "workbench", label: "运行工作台", icon: Blocks, shortcut: "⌘1" },
  { id: "runs", label: "运行记录", icon: ListTodo },
  { id: "automations", label: "自动化计划", icon: Zap },
];

const infrastructureNavigation: Array<{
  id: NavigationId;
  label: string;
  icon: typeof Gauge;
}> = [
  { id: "runtimes", label: "运行环境", icon: Boxes },
  { id: "secrets", label: "凭据保险箱", icon: KeyRound },
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
        <button className="workspace-switcher" type="button" disabled title="当前版本使用单一离线工作区">
          <span className="workspace-dot" />
          个人工作区
          <ChevronDown size={14} />
        </button>
        <div className="titlebar-spacer" />
        <button className="sync-state" type="button" aria-label="Host 状态" onClick={() => setActiveNavigation("runtimes")}>
          <span className="pulse-dot" />
          Host 已连接
        </button>
        <div className="window-controls" aria-label="窗口控制">
          <button type="button" aria-label="最小化" onClick={() => controlWindow("minimize")}><Minus size={14} /></button>
          <button type="button" aria-label="最大化" onClick={() => controlWindow("maximize")}><Maximize2 size={13} /></button>
          <button type="button" aria-label="关闭" onClick={() => controlWindow("close")}><X size={14} /></button>
        </div>
      </header>

      <aside className="sidebar">
        <button className="command-trigger" type="button" onClick={() => setCommandOpen(true)}>
          <Search size={15} />
          <span>搜索或运行命令</span>
          <kbd>⌘ K</kbd>
        </button>

        <nav aria-label="主导航">
          <NavigationGroup
            items={primaryNavigation}
            activeId={activeNavigation}
            onSelect={setActiveNavigation}
          />
          <div className="nav-label">基础设施</div>
          <NavigationGroup
            items={infrastructureNavigation}
            activeId={activeNavigation}
            onSelect={setActiveNavigation}
          />
        </nav>

        <div className="sidebar-spacer" />
        <button className="quick-run-card" type="button" onClick={() => setActiveNavigation("library")}>
          <div className="quick-run-icon"><Sparkles size={16} /></div>
          <div>
            <strong>快速安装</strong>
            <span>导入 rpaz 脚本包</span>
          </div>
          <Play size={14} fill="currentColor" />
        </button>
        <div className="sidebar-utility">
          <button type="button" onClick={() => setActiveNavigation("settings")}><CircleHelp size={16} /><span>帮助与诊断</span></button>
          <button type="button" onClick={() => setActiveNavigation("settings")}><Settings size={16} /><span>设置</span></button>
          <button type="button" onClick={toggleCompactMode} aria-pressed={compactMode}>
            <PanelLeftClose size={16} />
            <span>{compactMode ? "舒适布局" : "紧凑布局"}</span>
          </button>
        </div>
        <div className="account-card">
          <div className="avatar">EB</div>
          <div><strong>Ethan Bird</strong><span>管理员</span></div>
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
