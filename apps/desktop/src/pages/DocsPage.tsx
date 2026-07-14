import { BookOpen, Bot, Boxes, Code2, FileText, RefreshCw, RotateCcw } from "lucide-react";
import { useMemo, useState } from "react";

const documents = [
  { id: "overview", file: "index.html", label: "开发概览", detail: "架构、目录与快速开始", icon: BookOpen },
  { id: "rpaz", file: "rpaz.html", label: "RPAZ 规范", detail: "Manifest、Context 与构建", icon: Code2 },
  { id: "runtime", file: "runtime.html", label: "运行环境", detail: "离线 Python、浏览器与 Jupyter", icon: Boxes },
  { id: "agent", file: "agent.html", label: "AI Agent", detail: "连接配置、会话与内置工具", icon: Bot },
  { id: "update", file: "update.html", label: "发布与更新", detail: "全量基线和差量协议", icon: FileText },
] as const;

export function DocsPage() {
  const [selectedId, setSelectedId] = useState<(typeof documents)[number]["id"]>("overview");
  const [frameVersion, setFrameVersion] = useState(0);
  const selected = documents.find((item) => item.id === selectedId) ?? documents[0];
  const source = useMemo(
    () => new URL(`docs/${selected.file}?view=${frameVersion}`, window.location.href).toString(),
    [frameVersion, selected.file],
  );

  return (
    <div className="page docs-page">
      <header className="docs-header">
        <div><div className="eyebrow">Local HTML Reference</div><h1>开发文档</h1><p>文档随应用安装并离线打开，不依赖外部站点。</p></div>
        <button className="button ghost small" type="button" onClick={() => setFrameVersion((value) => value + 1)}><RefreshCw size={13} /> 重新载入</button>
      </header>
      <div className="docs-layout">
        <aside className="docs-navigation" aria-label="开发文档目录">
          <div className="docs-navigation-title"><BookOpen size={14} /><strong>DRPA 0.3</strong><span>OFFLINE</span></div>
          {documents.map((item) => {
            const Icon = item.icon;
            return (
              <button type="button" className={item.id === selected.id ? "active" : ""} onClick={() => setSelectedId(item.id)} key={item.id}>
                <Icon size={14} /><span><strong>{item.label}</strong><small>{item.detail}</small></span>
              </button>
            );
          })}
          <footer><RotateCcw size={12} /> 文档版本与桌面版本同步</footer>
        </aside>
        <section className="docs-reader" aria-label={selected.label}>
          <div className="docs-reader-toolbar"><span>{selected.label}</span><code>docs/{selected.file}</code></div>
          <iframe key={`${selected.file}-${frameVersion}`} title={`DRPA ${selected.label}`} src={source} />
        </section>
      </div>
    </div>
  );
}
