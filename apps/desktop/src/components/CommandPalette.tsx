import { ArrowRight, Blocks, Command, Library, Play, Search, Settings, Zap } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import type { NavigationId } from "../domain/models";
import { useAppStore } from "../app/store";

const commands: Array<{
  id: string;
  label: string;
  detail: string;
  icon: typeof Search;
  navigation?: NavigationId;
}> = [
  { id: "run", label: "Run Monthly close", detail: "Invoice Hub", icon: Play, navigation: "workbench" },
  { id: "workbench", label: "Open Workbench", detail: "Navigation", icon: Blocks, navigation: "workbench" },
  { id: "library", label: "Browse package library", detail: "Navigation", icon: Library, navigation: "library" },
  { id: "automation", label: "Create an automation", detail: "New", icon: Zap, navigation: "automations" },
  { id: "settings", label: "Open settings", detail: "Application", icon: Settings },
];

export function CommandPalette({ onClose }: { onClose: () => void }) {
  const [query, setQuery] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const setActiveNavigation = useAppStore((state) => state.setActiveNavigation);
  const filtered = commands.filter((item) =>
    `${item.label} ${item.detail}`.toLowerCase().includes(query.toLowerCase()),
  );

  useEffect(() => inputRef.current?.focus(), []);

  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={onClose}>
      <section
        className="command-palette"
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="command-input-row">
          <Command size={18} />
          <input
            ref={inputRef}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Type a command or search packages…"
          />
          <kbd>ESC</kbd>
        </div>
        <div className="command-section-label">Suggested</div>
        <div className="command-results">
          {filtered.map((item, index) => {
            const Icon = item.icon;
            return (
              <button
                key={item.id}
                className={index === 0 ? "command-result selected" : "command-result"}
                type="button"
                onClick={() => {
                  if (item.navigation) setActiveNavigation(item.navigation);
                  onClose();
                }}
              >
                <span className="command-result-icon"><Icon size={16} /></span>
                <span><strong>{item.label}</strong><small>{item.detail}</small></span>
                <ArrowRight size={14} />
              </button>
            );
          })}
        </div>
        <footer><span><kbd>↑</kbd><kbd>↓</kbd> Navigate</span><span><kbd>↵</kbd> Select</span></footer>
      </section>
    </div>
  );
}
