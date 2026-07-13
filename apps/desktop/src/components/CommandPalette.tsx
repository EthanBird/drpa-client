import { ArrowRight, Blocks, Code2, Command, Library, Play, Search, Settings, Zap } from "lucide-react";
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
  { id: "run", label: "打开运行工作台", detail: "运行脚本包", icon: Play, navigation: "workbench" },
  { id: "workbench", label: "配置任务参数", detail: "运行工作台", icon: Blocks, navigation: "workbench" },
  { id: "library", label: "安装脚本包", detail: "脚本包管理", icon: Library, navigation: "library" },
  { id: "studio", label: "新建 RPaz 项目", detail: "开发工作室", icon: Code2, navigation: "studio" },
  { id: "automation", label: "创建自动化计划", detail: "任务编排", icon: Zap, navigation: "automations" },
  { id: "settings", label: "打开设置", detail: "应用配置", icon: Settings, navigation: "settings" },
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
        aria-label="命令面板"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="command-input-row">
          <Command size={18} />
          <input
            ref={inputRef}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="输入命令或搜索脚本包…"
          />
          <kbd>ESC</kbd>
        </div>
        <div className="command-section-label">建议操作</div>
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
        <footer><span><kbd>↑</kbd><kbd>↓</kbd> 导航</span><span><kbd>↵</kbd> 选择</span></footer>
      </section>
    </div>
  );
}
