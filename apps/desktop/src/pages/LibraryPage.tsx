import { open } from "@tauri-apps/plugin-dialog";
import { Box, Code2, Grid2X2, List, Play, Plus, Search, ShieldCheck, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { useAppStore } from "../app/store";
import { desktopGateway } from "../infra/gateway";

export function LibraryPage() {
  const snapshot = useAppStore((state) => state.snapshot);
  const setSnapshot = useAppStore((state) => state.setSnapshot);
  const selectPackage = useAppStore((state) => state.selectPackage);
  const setActiveNavigation = useAppStore((state) => state.setActiveNavigation);
  const [query, setQuery] = useState("");
  const [view, setView] = useState<"grid" | "list">("grid");
  const [notice, setNotice] = useState<string | null>(null);
  const [installing, setInstalling] = useState(false);
  const [menu, setMenu] = useState<{ packageId: string; x: number; y: number } | null>(null);

  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape") close(); };
    window.addEventListener("click", close);
    window.addEventListener("blur", close);
    window.addEventListener("keydown", escape);
    return () => { window.removeEventListener("click", close); window.removeEventListener("blur", close); window.removeEventListener("keydown", escape); };
  }, [menu]);

  const packages = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    if (!snapshot || !normalized) return snapshot?.packages ?? [];
    return snapshot.packages.filter((item) => `${item.name} ${item.id} ${item.runtime}`.toLowerCase().includes(normalized));
  }, [query, snapshot]);

  if (!snapshot) return null;

  const install = async () => {
    setNotice(null);
    setInstalling(true);
    try {
      if (!("__TAURI_INTERNALS__" in window)) throw new Error("浏览器预览模式不能选择本地文件，请运行桌面应用。");
      const selected = await open({ multiple: false, filters: [{ name: "DRPA 脚本包", extensions: ["rpaz"] }] });
      if (!selected) return;
      const installed = await desktopGateway.installPackage(selected);
      setSnapshot(await desktopGateway.getWorkspaceSnapshot());
      selectPackage(installed.id, installed.profiles[0]?.id);
      setNotice(`安装成功：${installed.name} v${installed.version}`);
    } catch (error) {
      setNotice(`安装失败：${String(error)}`);
    } finally {
      setInstalling(false);
    }
  };

  const openWorkbench = (packageId: string) => {
    const item = snapshot.packages.find((candidate) => candidate.id === packageId);
    if (!item) return;
    selectPackage(item.id, item.profiles[0]?.id);
    setActiveNavigation("workbench");
  };

  const editPackage = async (packageId: string) => {
    try {
      const project = await desktopGateway.openInstalledPackage(packageId);
      setActiveNavigation("studio");
      setNotice(`已创建可编辑工作副本：${project.name}`);
    } catch (error) { setNotice(`打开失败：${String(error)}`); }
  };

  const uninstall = async (packageId: string) => {
    const item = snapshot.packages.find((candidate) => candidate.id === packageId);
    if (!item || !window.confirm(`卸载“${item.name}” v${item.version}？项目副本和历史运行不会被删除。`)) return;
    try {
      await desktopGateway.uninstallPackage(packageId);
      setSnapshot(await desktopGateway.getWorkspaceSnapshot());
      setNotice(`已卸载：${item.name} v${item.version}`);
    } catch (error) { setNotice(`卸载失败：${String(error)}`); }
  };

  return (
    <div className="page library-page">
      <header className="page-header"><div><div className="eyebrow">脚本包管理</div><h1>脚本包</h1><p>安装、检查和管理本地可执行的 `.rpaz` 自动化包。</p></div><button className="button primary" type="button" onClick={install} disabled={installing}><Plus size={15} /> {installing ? "正在安装…" : "安装脚本包"}</button></header>
      {notice && <div className={notice.startsWith("安装成功") ? "operation-notice success" : "operation-notice error"}>{notice}</div>}
      <div className="library-toolbar"><div className="large-search"><Search size={16} /><input aria-label="搜索脚本包" placeholder="搜索名称、ID 或运行环境" value={query} onChange={(event) => setQuery(event.target.value)} /></div><div className="view-toggle"><button className={view === "grid" ? "active" : ""} type="button" aria-label="网格视图" onClick={() => setView("grid")}><Grid2X2 size={14} /></button><button className={view === "list" ? "active" : ""} type="button" aria-label="列表视图" onClick={() => setView("list")}><List size={15} /></button></div></div>
      {packages.length === 0 && <div className="empty-state"><Box size={28} /><h2>{query ? "没有匹配的脚本包" : "尚未安装脚本包"}</h2><p>{query ? "请调整搜索条件。" : "可以安装示例 `.rpaz`，或在开发工作室中新建项目。"}</p><button className="button secondary" type="button" onClick={() => setActiveNavigation("studio")}>新建项目</button></div>}
      <div className={view === "grid" ? "package-card-grid" : "package-card-grid list-view"}>
        {packages.map((item) => (
          <article className="package-card" key={item.id} onContextMenu={(event) => { event.preventDefault(); setMenu({ packageId: item.id, x: event.clientX, y: event.clientY }); }}>
            <header><span className="package-card-avatar" style={{ "--package-accent": item.accent } as React.CSSProperties}>{item.initials}</span><span className={`status-badge ${item.trust === "verified" ? "success" : "neutral"}`}>{item.trust === "verified" && <ShieldCheck size={11} />}{item.trust === "verified" ? "已验证" : item.trust === "local" ? "本地" : "未信任"}</span></header>
            <h2>{item.name}</h2><p>{item.description}</p>
            <div className="package-meta"><span><Box size={13} /> v{item.version}</span><span>{item.runtime}</span><span>{item.parameters.length} 个参数</span></div>
            <footer><button className="button secondary" type="button" onClick={() => openWorkbench(item.id)}>打开运行工作台</button><small>右键管理</small></footer>
          </article>
        ))}
      </div>
      {menu && <div className="context-menu" role="menu" style={{ left: Math.min(menu.x, window.innerWidth - 210), top: Math.min(menu.y, window.innerHeight - 150) }} onClick={(event) => event.stopPropagation()}>
        <button type="button" role="menuitem" onClick={() => { openWorkbench(menu.packageId); setMenu(null); }}><Play size={14} /> 打开运行工作台</button>
        <button type="button" role="menuitem" onClick={() => { void editPackage(menu.packageId); setMenu(null); }}><Code2 size={14} /> 在开发工作室编辑</button>
        <span />
        <button className="danger" type="button" role="menuitem" onClick={() => { void uninstall(menu.packageId); setMenu(null); }}><Trash2 size={14} /> 卸载脚本包</button>
      </div>}
    </div>
  );
}
