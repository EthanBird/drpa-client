import {
  Blocks,
  Bot,
  BookOpenCheck,
  Boxes,
  ChevronDown,
  CircleHelp,
  Code2,
  Command,
  Database,
  Gauge,
  KeyRound,
  Library,
  ListTodo,
  LoaderCircle,
  Maximize2,
  Minimize2,
  Minus,
  Play,
  Plus,
  PlugZap,
  Search,
  Settings,
  Sparkles,
  Workflow,
  Wrench,
  X,
  Zap,
} from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useRef, useState, type FormEvent, type MouseEvent, type PropsWithChildren } from "react";

import type { NavigationId, WorkspaceInfo } from "../domain/models";
import { useAppStore } from "../app/store";
import { desktopGateway } from "../infra/gateway";
import { useI18n, type TranslationKey } from "../i18n";
import { SidebarToggle, useSidebarCollapsed } from "./SidebarToggle";

const primaryNavigation: Array<{
  id: NavigationId;
  label: string;
  icon: typeof Gauge;
  shortcut?: string;
}> = [
  { id: "overview", label: "BI 主页", icon: Gauge },
  { id: "library", label: "RPAZ 包", icon: Library },
  { id: "studio", label: "开发工作室", icon: Code2 },
  { id: "data", label: "数据工作台", icon: Database },
  { id: "workbench", label: "运行工作台", icon: Blocks, shortcut: "⌘1" },
  { id: "runs", label: "运行记录", icon: ListTodo },
  { id: "automations", label: "自动化计划", icon: Zap },
];

const infrastructureNavigation: Array<{
  id: NavigationId;
  label: string;
  icon: typeof Gauge;
}> = [
  { id: "localDify", label: "流程设计", icon: Workflow },
  { id: "agent", label: "AI Agent", icon: Bot },
  { id: "knowledgeBase", label: "知识库", icon: BookOpenCheck },
  { id: "extensionTools", label: "扩展工具", icon: Wrench },
  { id: "plugins", label: "插件", icon: PlugZap },
  { id: "runtimes", label: "运行环境", icon: Boxes },
  { id: "secrets", label: "凭据保险箱", icon: KeyRound },
];

const navigationTranslationKeys: Partial<Record<NavigationId, TranslationKey>> = {
  overview: "nav.overview",
  library: "nav.library",
  studio: "nav.studio",
  data: "nav.data",
  workbench: "nav.workbench",
  runs: "nav.runs",
  automations: "nav.automations",
  localDify: "nav.localDify",
  agent: "nav.agent",
  knowledgeBase: "nav.knowledgeBase",
  extensionTools: "nav.extensionTools",
  plugins: "nav.plugins",
  runtimes: "nav.runtimes",
  secrets: "nav.secrets",
};

export function AppShell({ children }: PropsWithChildren) {
  const { t } = useI18n();
  const activeNavigation = useAppStore((state) => state.activeNavigation);
  const setActiveNavigation = useAppStore((state) => state.setActiveNavigation);
  const setCommandOpen = useAppStore((state) => state.setCommandOpen);
  const setWorkspaceScope = useAppStore((state) => state.setWorkspaceScope);
  const navigationCollapsed = useSidebarCollapsed("app-navigation");
  const inDesktopHost = "__TAURI_INTERNALS__" in window;
  const [maximized, setMaximized] = useState(false);
  const [currentUser, setCurrentUser] = useState({ displayName: "本地用户", accountName: "local", initials: "本地" });
  const [workspaces, setWorkspaces] = useState<WorkspaceInfo[]>([]);
  const [workspaceMenuOpen, setWorkspaceMenuOpen] = useState(false);
  const [creatingWorkspace, setCreatingWorkspace] = useState(false);
  const [workspaceName, setWorkspaceName] = useState("");
  const [workspaceBusy, setWorkspaceBusy] = useState(false);
  const [workspaceError, setWorkspaceError] = useState("");
  const workspaceControlRef = useRef<HTMLDivElement>(null);
  const currentWorkspace = workspaces.find((workspace) => workspace.active)
    ?? workspaces[0]
    ?? { id: "personal", name: "个人工作区", path: "", active: true, createdAt: 0 };

  useEffect(() => {
    if (!inDesktopHost) return;
    void getCurrentWindow().isMaximized().then(setMaximized).catch(() => setMaximized(false));
  }, [inDesktopHost]);

  useEffect(() => {
    void desktopGateway.getCurrentUser().then(setCurrentUser).catch(() => undefined);
  }, []);

  useEffect(() => {
    let active = true;
    void desktopGateway.listWorkspaces()
      .then((items) => {
        if (!active) return;
        setWorkspaces(items);
        const selected = items.find((workspace) => workspace.active) ?? items[0];
        if (selected) setWorkspaceScope(selected.id);
      })
      .catch((reason: unknown) => { if (active) setWorkspaceError(String(reason)); });
    return () => { active = false; };
  }, [setWorkspaceScope]);

  useEffect(() => {
    if (!workspaceMenuOpen) return;
    const closeOnOutsideClick = (event: PointerEvent) => {
      if (!workspaceControlRef.current?.contains(event.target as Node)) setWorkspaceMenuOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setWorkspaceMenuOpen(false);
    };
    window.addEventListener("pointerdown", closeOnOutsideClick);
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      window.removeEventListener("pointerdown", closeOnOutsideClick);
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [workspaceMenuOpen]);

  const switchWorkspace = async (workspace: WorkspaceInfo) => {
    if (workspace.active || workspaceBusy) {
      setWorkspaceMenuOpen(false);
      return;
    }
    const previous = currentWorkspace;
    setWorkspaceBusy(true);
    setWorkspaceError("");
    let switched = false;
    try {
      await desktopGateway.switchWorkspace(workspace.id);
      switched = true;
      if (!inDesktopHost) {
        setWorkspaceScope(workspace.id);
        const items = await desktopGateway.listWorkspaces();
        setWorkspaces(items);
      }
      setWorkspaceMenuOpen(false);
    } catch (reason) {
      if (switched) {
        await desktopGateway.switchWorkspace(previous.id).catch(() => undefined);
      }
      if (!inDesktopHost && switched) setWorkspaceScope(previous.id);
      setWorkspaceError(String(reason));
    } finally {
      setWorkspaceBusy(false);
    }
  };

  const createWorkspace = async (event: FormEvent) => {
    event.preventDefault();
    if (!workspaceName.trim() || workspaceBusy) return;
    const previous = currentWorkspace;
    setWorkspaceBusy(true);
    setWorkspaceError("");
    let switched = false;
    try {
      const created = await desktopGateway.createWorkspace(workspaceName);
      setWorkspaces((items) => [...items, created]);
      setWorkspaceName("");
      setCreatingWorkspace(false);
      await desktopGateway.switchWorkspace(created.id);
      switched = true;
      if (!inDesktopHost) {
        setWorkspaceScope(created.id);
        const items = await desktopGateway.listWorkspaces();
        setWorkspaces(items);
      }
      setWorkspaceMenuOpen(false);
    } catch (reason) {
      if (switched) {
        await desktopGateway.switchWorkspace(previous.id).catch(() => undefined);
      }
      if (!inDesktopHost && switched) setWorkspaceScope(previous.id);
      setWorkspaceError(String(reason));
    } finally {
      setWorkspaceBusy(false);
    }
  };

  const controlWindow = async (action: "minimize" | "maximize" | "close") => {
    if (!inDesktopHost) return;
    const appWindow = getCurrentWindow();
    if (action === "minimize") await appWindow.minimize();
    if (action === "maximize") {
      await appWindow.toggleMaximize();
      setMaximized(await appWindow.isMaximized());
    }
    if (action === "close") await appWindow.close();
  };

  const startWindowDrag = (event: MouseEvent<HTMLElement>) => {
    if (!inDesktopHost || event.button !== 0 || event.detail !== 1) return;
    const target = event.target as HTMLElement;
    if (target.closest("button, input, select, textarea, summary, a")) return;
    void getCurrentWindow().startDragging();
  };

  return (
    <div className={navigationCollapsed ? "shell navigation-collapsed" : "shell"}>
      <header className="titlebar" data-tauri-drag-region onMouseDown={startWindowDrag} onDoubleClick={(event) => { if (!(event.target as HTMLElement).closest("button, input, select, textarea, summary, a")) void controlWindow("maximize"); }}>
        <div className="brand-lockup" data-tauri-drag-region>
          <div className="brand-mark" aria-hidden="true">
            <Bot size={17} strokeWidth={2.1} />
          </div>
          <span className="brand-name">DRPA</span>
          <span className="release-chip">NEXT</span>
          {!navigationCollapsed && <SidebarToggle id="app-navigation" side="left" label="主侧边栏" />}
        </div>
        <div className="workspace-control" ref={workspaceControlRef}>
          <button
            className={workspaceMenuOpen ? "workspace-switcher active" : "workspace-switcher"}
            type="button"
            title={currentWorkspace.path || currentWorkspace.name}
            aria-haspopup="dialog"
            aria-expanded={workspaceMenuOpen}
            onClick={() => { setWorkspaceMenuOpen((open) => !open); setWorkspaceError(""); }}
          >
            <span className="workspace-dot" />
            <span>{currentWorkspace.name}</span>
            <ChevronDown size={14} />
          </button>
          {workspaceMenuOpen && (
            <section className="workspace-menu" role="dialog" aria-label="切换工作区" onMouseDown={(event) => event.stopPropagation()}>
              <header>
                <div><strong>工作区</strong><span>数据、项目与任务完全隔离</span></div>
                <button type="button" aria-label="新建工作区" onClick={() => setCreatingWorkspace(true)}><Plus size={15} /></button>
              </header>
              <div className="workspace-list" role="listbox" aria-label="工作区列表">
                {workspaces.map((workspace) => (
                  <button
                    type="button"
                    role="option"
                    aria-selected={workspace.active}
                    className={workspace.active ? "active" : ""}
                    key={workspace.id}
                    disabled={workspaceBusy}
                    title={workspace.path}
                    onClick={() => void switchWorkspace(workspace)}
                  >
                    <span className="workspace-avatar">{workspace.name.trim().slice(0, 1).toUpperCase()}</span>
                    <span><strong>{workspace.name}</strong><small>{workspace.id === "personal" ? "兼容原有本地数据" : "独立数据目录"}</small></span>
                    {workspace.active && <span className="workspace-current">当前</span>}
                  </button>
                ))}
              </div>
              {creatingWorkspace ? (
                <form className="workspace-create" onSubmit={(event) => void createWorkspace(event)}>
                  <label htmlFor="workspace-name">新建隔离工作区</label>
                  <div>
                    <input
                      id="workspace-name"
                      autoFocus
                      maxLength={60}
                      placeholder="例如：客户 A / 测试环境"
                      value={workspaceName}
                      onChange={(event) => setWorkspaceName(event.target.value)}
                    />
                    <button className="button primary small" type="submit" disabled={!workspaceName.trim() || workspaceBusy}>
                      {workspaceBusy ? <LoaderCircle className="spin" size={14} /> : "创建并进入"}
                    </button>
                  </div>
                  <button type="button" onClick={() => { setCreatingWorkspace(false); setWorkspaceName(""); }}>取消</button>
                </form>
              ) : (
                <button className="workspace-add" type="button" onClick={() => setCreatingWorkspace(true)}>
                  <Plus size={14} /> 新建工作区
                </button>
              )}
              {workspaceError && <p className="workspace-error">{workspaceError}</p>}
            </section>
          )}
        </div>
        <div className="titlebar-spacer" data-tauri-drag-region />
        <button className="sync-state" type="button" aria-label="Host 状态" onClick={() => setActiveNavigation("runtimes")}>
          <span className="pulse-dot" />
          {t("status.hostConnected")}
        </button>
        <div className="window-controls" aria-label="窗口控制">
          <button type="button" aria-label="最小化" onClick={() => void controlWindow("minimize")}><Minus size={14} /></button>
          <button type="button" aria-label={maximized ? "还原窗口" : "最大化"} onClick={() => void controlWindow("maximize")}>{maximized ? <Minimize2 size={13} /> : <Maximize2 size={13} />}</button>
          <button type="button" aria-label="关闭" onClick={() => void controlWindow("close")}><X size={14} /></button>
        </div>
      </header>

      <aside
        className={navigationCollapsed ? "sidebar navigation-rail" : "sidebar"}
        aria-label={navigationCollapsed ? "主导航图标栏" : "主侧边栏"}
      >
        {navigationCollapsed && <SidebarToggle id="app-navigation" side="left" label="主侧边栏" restore />}
        <button
          className="command-trigger"
          type="button"
          aria-label={t("nav.search")}
          title={navigationCollapsed ? t("nav.search") : undefined}
          onClick={() => setCommandOpen(true)}
        >
          <Search size={15} />
          <span>{t("nav.search")}</span>
          <kbd>⌘ K</kbd>
        </button>

        <nav aria-label="主导航">
          <NavigationGroup
            items={primaryNavigation.map((item) => ({ ...item, label: t(navigationTranslationKeys[item.id]!) }))}
            activeId={activeNavigation}
            onSelect={setActiveNavigation}
            compact={navigationCollapsed}
          />
          <div className="nav-label" aria-hidden={navigationCollapsed}>{t("nav.infrastructure")}</div>
          <NavigationGroup
            items={infrastructureNavigation.map((item) => ({ ...item, label: t(navigationTranslationKeys[item.id]!) }))}
            activeId={activeNavigation}
            onSelect={setActiveNavigation}
            compact={navigationCollapsed}
          />
        </nav>

        <div className="sidebar-spacer" />
        <button
          className="quick-run-card"
          type="button"
          aria-label="快速安装 RPAZ 包"
          title={navigationCollapsed ? "快速安装 RPAZ 包" : undefined}
          onClick={() => setActiveNavigation("library")}
        >
          <div className="quick-run-icon"><Sparkles size={16} /></div>
          <div>
            <strong>{t("nav.quickInstall")}</strong>
            <span>{t("nav.importPackage")}</span>
          </div>
          <Play size={14} fill="currentColor" />
        </button>
        <div className="sidebar-utility">
          <button type="button" aria-label={t("nav.docs")} title={navigationCollapsed ? t("nav.docs") : undefined} onClick={() => setActiveNavigation("docs")}><CircleHelp size={16} /><span>{t("nav.docs")}</span></button>
          <button type="button" aria-label={t("nav.settings")} title={navigationCollapsed ? t("nav.settings") : undefined} onClick={() => setActiveNavigation("settings")}><Settings size={16} /><span>{t("nav.settings")}</span></button>
        </div>
        <div className="account-card" title={navigationCollapsed ? currentUser.displayName : undefined}>
          <div className="avatar">{currentUser.initials}</div>
          <div><strong>{currentUser.displayName}</strong><span title={currentUser.accountName}>本机用户</span></div>
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
  compact?: boolean;
}

function NavigationGroup({ items, activeId, onSelect, compact = false }: NavigationGroupProps) {
  return (
    <div className="nav-group">
      {items.map((item) => {
        const Icon = item.icon;
        return (
          <button
            key={item.id}
            className={activeId === item.id ? "nav-item active" : "nav-item"}
            type="button"
            aria-label={compact ? item.label : undefined}
            title={compact ? item.label : undefined}
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
