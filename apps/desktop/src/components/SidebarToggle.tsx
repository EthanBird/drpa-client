import {
  PanelLeftClose,
  PanelLeftOpen,
  PanelRightClose,
  PanelRightOpen,
} from "lucide-react";

import { useAppStore } from "../app/store";

interface SidebarToggleProps {
  id: string;
  side: "left" | "right";
  label: string;
  restore?: boolean;
}

export function useSidebarCollapsed(id: string) {
  return useAppStore((state) => Boolean(state.collapsedSidebars[id]));
}

export function SidebarToggle({ id, side, label, restore = false }: SidebarToggleProps) {
  const setSidebarCollapsed = useAppStore((state) => state.setSidebarCollapsed);
  const Icon = restore
    ? side === "left" ? PanelLeftOpen : PanelRightOpen
    : side === "left" ? PanelLeftClose : PanelRightClose;
  const action = restore ? `展开${label}` : `隐藏${label}`;

  return (
    <button
      className={restore ? `sidebar-restore-button ${side}` : `sidebar-collapse-button ${side}`}
      type="button"
      title={action}
      aria-label={action}
      onClick={() => setSidebarCollapsed(id, !restore)}
    >
      <Icon size={14} />
      {restore && <span>{label}</span>}
    </button>
  );
}
