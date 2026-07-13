import { ArrowRight, Construction, Sparkles } from "lucide-react";

export function PlaceholderPage({ eyebrow, title, description }: { eyebrow: string; title: string; description: string }) {
  return <div className="page placeholder-page"><div className="placeholder-orbit"><span><Construction size={24} /></span><i /><i /></div><div className="eyebrow">{eyebrow}</div><h1>{title}</h1><p>{description}</p><div className="placeholder-callout"><Sparkles size={16} /><span>This module is part of the DRPA Next architecture and will inherit the same design tokens and host security model.</span></div><button className="button secondary" type="button">Read architecture notes <ArrowRight size={14} /></button></div>;
}
