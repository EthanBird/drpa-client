import { Box, ChevronDown, Download, Grid2X2, List, Plus, Search, ShieldCheck, SlidersHorizontal } from "lucide-react";

import { useAppStore } from "../app/store";

export function LibraryPage() {
  const snapshot = useAppStore((state) => state.snapshot);
  const selectPackage = useAppStore((state) => state.selectPackage);
  const setActiveNavigation = useAppStore((state) => state.setActiveNavigation);
  if (!snapshot) return null;
  return (
    <div className="page library-page">
      <header className="page-header"><div><div className="eyebrow">Package management</div><h1>Library</h1><p>Install, inspect and govern executable packages in this workspace.</p></div><button className="button primary" type="button"><Plus size={15} /> Install package</button></header>
      <div className="library-toolbar"><div className="large-search"><Search size={16} /><input aria-label="Search packages" placeholder="Search packages, runtimes or capabilities" /></div><button className="button secondary" type="button"><SlidersHorizontal size={15} /> Filters <ChevronDown size={13} /></button><div className="view-toggle"><button className="active" type="button" aria-label="Grid view"><Grid2X2 size={14} /></button><button type="button" aria-label="List view"><List size={15} /></button></div></div>
      <div className="package-card-grid">
        {snapshot.packages.map((item) => (
          <article className="package-card" key={item.id}>
            <header><span className="package-card-avatar" style={{ "--package-accent": item.accent } as React.CSSProperties}>{item.initials}</span><span className={`status-badge ${item.trust === "verified" ? "success" : "neutral"}`}>{item.trust === "verified" && <ShieldCheck size={11} />}{item.trust}</span></header>
            <h2>{item.name}</h2><p>{item.description}</p>
            <div className="package-meta"><span><Box size={13} /> v{item.version}</span><span>{item.runtime}</span><span>{item.profiles.length} profiles</span></div>
            <footer><button className="button secondary" type="button" onClick={() => { selectPackage(item.id, item.profiles[0]?.id); setActiveNavigation("workbench"); }}>Open Workbench</button><button className="icon-button subtle" type="button" aria-label={`Update ${item.name}`}><Download size={15} /></button></footer>
          </article>
        ))}
      </div>
    </div>
  );
}
