import { ArrowRight, Construction, Sparkles } from "lucide-react";

export function PlaceholderPage({ eyebrow, title, description }: { eyebrow: string; title: string; description: string }) {
  return <div className="page placeholder-page"><div className="placeholder-orbit"><span><Construction size={24} /></span><i /><i /></div><div className="eyebrow">{eyebrow}</div><h1>{title}</h1><p>{description}</p><div className="placeholder-callout"><Sparkles size={16} /><span>此模块正在接入真实 Host 服务；尚未完成的操作不会伪装成可用功能。</span></div><button className="button secondary" type="button" disabled>开发中 <ArrowRight size={14} /></button></div>;
}
