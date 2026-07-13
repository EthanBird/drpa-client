import { open } from "@tauri-apps/plugin-dialog";
import { Box, Grid2X2, List, Plus, Search, ShieldCheck } from "lucide-react";
import { useMemo, useState } from "react";

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

  return (
    <div className="page library-page">
      <header className="page-header"><div><div className="eyebrow">脚本包管理</div><h1>脚本包</h1><p>安装、检查和管理本地可执行的 `.rpaz` 自动化包。</p></div><button className="button primary" type="button" onClick={install} disabled={installing}><Plus size={15} /> {installing ? "正在安装…" : "安装脚本包"}</button></header>
      {notice && <div className={notice.startsWith("安装成功") ? "operation-notice success" : "operation-notice error"}>{notice}</div>}
      <div className="library-toolbar"><div className="large-search"><Search size={16} /><input aria-label="搜索脚本包" placeholder="搜索名称、ID 或运行环境" value={query} onChange={(event) => setQuery(event.target.value)} /></div><div className="view-toggle"><button className={view === "grid" ? "active" : ""} type="button" aria-label="网格视图" onClick={() => setView("grid")}><Grid2X2 size={14} /></button><button className={view === "list" ? "active" : ""} type="button" aria-label="列表视图" onClick={() => setView("list")}><List size={15} /></button></div></div>
      {packages.length === 0 && <div className="empty-state"><Box size={28} /><h2>{query ? "没有匹配的脚本包" : "尚未安装脚本包"}</h2><p>{query ? "请调整搜索条件。" : "可以安装示例 `.rpaz`，或在开发工作室中新建项目。"}</p><button className="button secondary" type="button" onClick={() => setActiveNavigation("studio")}>新建项目</button></div>}
      <div className={view === "grid" ? "package-card-grid" : "package-card-grid list-view"}>
        {packages.map((item) => (
          <article className="package-card" key={item.id}>
            <header><span className="package-card-avatar" style={{ "--package-accent": item.accent } as React.CSSProperties}>{item.initials}</span><span className={`status-badge ${item.trust === "verified" ? "success" : "neutral"}`}>{item.trust === "verified" && <ShieldCheck size={11} />}{item.trust === "verified" ? "已验证" : item.trust === "local" ? "本地" : "未信任"}</span></header>
            <h2>{item.name}</h2><p>{item.description}</p>
            <div className="package-meta"><span><Box size={13} /> v{item.version}</span><span>{item.runtime}</span><span>{item.parameters.length} 个参数</span></div>
            <footer><button className="button secondary" type="button" onClick={() => { selectPackage(item.id, item.profiles[0]?.id); setActiveNavigation("workbench"); }}>打开运行工作台</button></footer>
          </article>
        ))}
      </div>
    </div>
  );
}
