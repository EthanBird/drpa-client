import {
  Braces,
  FileSearch,
  PackagePlus,
  Puzzle,
  RefreshCw,
  ShieldCheck,
  Trash2,
  Wrench,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

import { useAppStore } from "../app/store";
import type { AgentExtensionSummary, AgentToolPolicy } from "../domain/models";
import { desktopGateway } from "../infra/gateway";
import "../styles/extension-tools.css";

type PolicyKey = Exclude<keyof AgentToolPolicy, "enabled" | "fileReadScope">;

const builtInPolicies: Array<{
  key: PolicyKey;
  label: string;
  detail: string;
  tools: string[];
}> = [
  { key: "databaseRead", label: "只读数据库", detail: "列出连接、读取结构并执行 Host 强制的只读查询。", tools: ["data_list_connections", "data_get_schema", "data_query"] },
  { key: "databaseConnections", label: "创建连接配置", detail: "新增 PostgreSQL、MySQL、SQLite、Excel 配置，不保存密码。", tools: ["data_create_connection"] },
  { key: "arbitraryFileRead", label: "文件检索与只读", detail: "按配置范围执行 find/search/read；只读文本单次最多 2000 行，扫描不跟随符号链接。", tools: ["find_files", "search_text", "read_file"] },
  { key: "projectWrite", label: "项目内编辑", detail: "精确 edit 与文件写入严格限制在当前通用项目内。", tools: ["edit_file", "rpaz_write_file", "rpaz_validate", "rpaz_build"] },
  { key: "python", label: "Python 辅助执行", detail: "在当前通用项目目录运行内置 Python。", tools: ["rpaz_python"] },
  { key: "knowledgeBaseRead", label: "知识库查询", detail: "列出并混合检索当前隔离工作区的向量知识库。", tools: ["knowledge_base_list", "knowledge_base_search"] },
  { key: "documentRead", label: "文档读取", detail: "读取对话中的 PDF、Word、Excel 与 PowerPoint 附件。", tools: ["document_read"] },
  { key: "documentWrite", label: "文档创建", detail: "在隔离的对话产物目录创建办公文档。", tools: ["document_create"] },
  { key: "documentConvert", label: "文档转换", detail: "转换对话附件并保留原文件。", tools: ["document_convert"] },
  { key: "workspaceWrite", label: "知识与记忆写入", detail: "更新 Skill、结构化记忆账本、MEMORY.md 与本地知识文档。", tools: ["agent_write_skill", "agent_remember", "agent_write_memory", "knowledge_write_document"] },
  { key: "extensions", label: "Skills、插件与 QuickJS", detail: "加载已选择的 Skill、已启用插件及离线 QuickJS 扩展工具。", tools: ["skill_*", "plugin_*", "ext__*"] },
  { key: "browser", label: "Chrome 浏览器控制", detail: "通过内置 Chrome/DrissionPage 桥打开、读取并操作页面。", tools: ["browser_*"] },
  { key: "rpazRuns", label: "运行 RPAZ 包", detail: "通过 DRPA Host 启动 RPAZ 包，运行会进入统一运行记录。", tools: ["rpaz_list_packages", "rpaz_run_package"] },
  { key: "runRecords", label: "读取运行记录", detail: "列出运行记录并读取事件、debug 日志和结果详情。", tools: ["run_list", "run_get_detail"] },
  { key: "vaultRead", label: "读取凭据保险箱", detail: "保险箱已由用户验证解锁时，允许列出并读取本地凭据。", tools: ["vault_list_credentials", "vault_get_credential"] },
  { key: "vaultWrite", label: "写入凭据保险箱", detail: "保险箱已解锁时，允许 Agent 新增或更新本地凭据。", tools: ["vault_upsert_credential"] },
];

function PolicyToggle({
  item,
  checked,
  disabled,
  onChange,
}: {
  item: (typeof builtInPolicies)[number];
  checked: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <article className={disabled ? "extension-policy-card disabled" : "extension-policy-card"}>
      <div className="extension-policy-copy">
        <strong>{item.label}</strong>
        <p>{item.detail}</p>
        <div>{item.tools.map((tool) => <code key={tool}>{tool}</code>)}</div>
      </div>
      <button
        className={`switch ${checked ? "on" : ""}`}
        type="button"
        role="switch"
        aria-label={item.label}
        aria-checked={checked}
        disabled={disabled}
        onClick={() => onChange(!checked)}
      >
        <span />
      </button>
    </article>
  );
}

function sourceLabel(source: string) {
  if (source === "bundled") return "内置";
  if (source === "npm-tgz-offline") return "离线 npm";
  return "本地 JS";
}

export function ExtensionToolsPage() {
  const policy = useAppStore((state) => state.agentToolPolicy);
  const pythonTimeout = useAppStore((state) => state.agentPythonTimeoutSeconds);
  const setPolicy = useAppStore((state) => state.setAgentToolPolicy);
  const [tab, setTab] = useState<"builtin" | "extensions">("builtin");
  const [extensions, setExtensions] = useState<AgentExtensionSummary[]>([]);
  const [busyId, setBusyId] = useState("");
  const [loading, setLoading] = useState(true);
  const [notice, setNotice] = useState("");

  const loadExtensions = useCallback(async () => {
    setLoading(true);
    try {
      setExtensions(await desktopGateway.listAgentExtensions());
      setNotice("");
    } catch (error) {
      setNotice(`读取扩展失败：${String(error)}`);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { void loadExtensions(); }, [loadExtensions]);

  const enabledToolCount = useMemo(
    () => builtInPolicies.filter((item) => policy.enabled && policy[item.key]).reduce((sum, item) => sum + item.tools.length, 0),
    [policy],
  );

  const installExtension = async () => {
    try {
      const packagePath = await desktopGateway.selectAgentExtensionPackage();
      if (!packagePath) return;
      setBusyId("install");
      const installed = await desktopGateway.installAgentExtension(packagePath);
      setExtensions((items) => [...items.filter((item) => item.id !== installed.id), installed].sort((a, b) => a.name.localeCompare(b.name)));
      setNotice(`已安装 ${installed.name}，注册 ${installed.tools.length} 个工具`);
      setTab("extensions");
    } catch (error) {
      setNotice(`安装失败：${String(error)}`);
    } finally {
      setBusyId("");
    }
  };

  const toggleExtension = async (extension: AgentExtensionSummary) => {
    setBusyId(extension.id);
    try {
      const updated = await desktopGateway.setAgentExtensionEnabled(extension.id, !extension.enabled);
      setExtensions((items) => items.map((item) => item.id === updated.id ? updated : item));
      setNotice(`${updated.name} 已${updated.enabled ? "启用" : "停用"}`);
    } catch (error) {
      setNotice(`更新失败：${String(error)}`);
    } finally {
      setBusyId("");
    }
  };

  const removeExtension = async (extension: AgentExtensionSummary) => {
    if (!window.confirm(`卸载扩展“${extension.name}”？本操作会删除当前工作区中的扩展副本。`)) return;
    setBusyId(extension.id);
    try {
      await desktopGateway.removeAgentExtension(extension.id);
      setExtensions((items) => items.filter((item) => item.id !== extension.id));
      setNotice(`已卸载 ${extension.name}`);
    } catch (error) {
      setNotice(`卸载失败：${String(error)}`);
    } finally {
      setBusyId("");
    }
  };

  return (
    <div className="page extensions-page">
      <header className="page-header">
        <div><div className="eyebrow">Agent capability registry</div><h1>扩展工具</h1><p>集中管理内置权限、Skills、插件与离线 QuickJS 扩展。</p></div>
        <button className="button primary" type="button" onClick={() => void installExtension()} disabled={busyId === "install"}>
          <PackagePlus size={15} /> {busyId === "install" ? "正在预检…" : "安装本地扩展"}
        </button>
      </header>

      <div className="extension-toolbar">
        <div className="extension-tabs" role="tablist" aria-label="扩展工具分类">
          <button type="button" role="tab" aria-selected={tab === "builtin"} className={tab === "builtin" ? "active" : ""} onClick={() => setTab("builtin")}><Wrench size={14} /> 内置工具 <span>{enabledToolCount}</span></button>
          <button type="button" role="tab" aria-selected={tab === "extensions"} className={tab === "extensions" ? "active" : ""} onClick={() => setTab("extensions")}><Puzzle size={14} /> 扩展 <span>{extensions.length}</span></button>
        </div>
        <div className="extension-security"><ShieldCheck size={14} /><span>Host 执行时再次校验权限；数据库不提供写工具</span></div>
        <button className="icon-button subtle" type="button" aria-label="刷新扩展" title="刷新扩展" onClick={() => void loadExtensions()} disabled={loading}><RefreshCw className={loading ? "spin" : ""} size={15} /></button>
      </div>

      {notice && <div className={notice.includes("失败") ? "extension-notice error" : "extension-notice"} role="status">{notice}</div>}

      <div className="extension-content">
        {tab === "builtin" ? (
          <>
            <section className="extension-master">
              <div className="extension-master-icon"><FileSearch size={19} /></div>
              <div><strong>启用 AI Agent 工具</strong><span>关闭后只进行模型对话，不向模型发送任何工具定义。Python 当前超时 {pythonTimeout} 秒。</span></div>
              <button className={`switch ${policy.enabled ? "on" : ""}`} type="button" role="switch" aria-label="启用 AI Agent 工具" aria-checked={policy.enabled} onClick={() => setPolicy({ enabled: !policy.enabled })}><span /></button>
            </section>
            <section className={`extension-read-scope ${!policy.enabled || !policy.arbitraryFileRead ? "disabled" : ""}`} aria-label="Agent 文件读取范围">
              <div>
                <strong>文件读取范围</strong>
                <span>只影响 <code>read_file</code>、<code>find_files</code>、<code>search_text</code>；所有写入仍锁定在项目目录。</span>
              </div>
              <div className="extension-scope-options" role="group" aria-label="选择文件读取范围">
                <button type="button" className={(policy.fileReadScope ?? "system") === "system" ? "active" : ""} aria-pressed={(policy.fileReadScope ?? "system") === "system"} disabled={!policy.enabled || !policy.arbitraryFileRead} onClick={() => setPolicy({ fileReadScope: "system" })}><strong>整个操作系统</strong><small>允许绝对路径，受当前用户系统权限限制</small></button>
                <button type="button" className={policy.fileReadScope === "project" ? "active" : ""} aria-pressed={policy.fileReadScope === "project"} disabled={!policy.enabled || !policy.arbitraryFileRead} onClick={() => setPolicy({ fileReadScope: "project" })}><strong>仅当前项目</strong><small>相对或绝对路径都不能越过项目根目录</small></button>
              </div>
            </section>
            <section className="extension-policy-grid">
              {builtInPolicies.map((item) => (
                <PolicyToggle
                  key={item.key}
                  item={item}
                  checked={policy[item.key]}
                  disabled={!policy.enabled}
                  onChange={(checked) => setPolicy({ [item.key]: checked })}
                />
              ))}
            </section>
          </>
        ) : (
          <section className="extension-list">
            {loading && extensions.length === 0 && <div className="empty-inline">正在扫描当前工作区的扩展目录…</div>}
            {!loading && extensions.length === 0 && <div className="empty-state"><Puzzle size={30} /><h2>尚未安装扩展</h2><p>可导入无外部依赖的 .js/.mjs 或 npm .tgz 离线包。</p><button className="button primary" type="button" onClick={() => void installExtension()}><PackagePlus size={14} /> 安装扩展</button></div>}
            {extensions.map((extension) => (
              <article className={extension.enabled ? "extension-card enabled" : "extension-card"} key={extension.id}>
                <header>
                  <div className="extension-card-icon"><Braces size={18} /></div>
                  <div><h2>{extension.name}</h2><p>{extension.description || "未提供扩展说明"}</p></div>
                  <div className="extension-card-actions">
                    <span className={`status-badge ${extension.enabled ? "success" : "neutral"}`}>{extension.enabled ? "已启用" : "已停用"}</span>
                    <button className={`switch ${extension.enabled ? "on" : ""}`} type="button" role="switch" aria-label={`启用 ${extension.name}`} aria-checked={extension.enabled} disabled={busyId === extension.id || !policy.enabled || !policy.extensions} onClick={() => void toggleExtension(extension)}><span /></button>
                    {extension.source !== "bundled" && <button className="icon-button subtle extension-remove" type="button" aria-label={`卸载 ${extension.name}`} title="卸载扩展" disabled={busyId === extension.id} onClick={() => void removeExtension(extension)}><Trash2 size={14} /></button>}
                  </div>
                </header>
                <div className="extension-meta">
                  <span><b>{extension.runtime}</b> {extension.version}</span>
                  <span>{sourceLabel(extension.source)}</span>
                  <code title={extension.integrity}>sha256 {extension.integrity.slice(0, 12)}</code>
                  <code title={extension.directory}>{extension.directory}</code>
                </div>
                <div className="extension-tool-list">
                  {extension.tools.length === 0 && <span className="extension-no-tools">没有发现可注册工具</span>}
                  {extension.tools.map((tool) => (
                    <div key={tool.exposedName}>
                      <span><strong>{tool.label}</strong><small>{tool.description}</small></span>
                      <code title={tool.exposedName}>{tool.exposedName}</code>
                    </div>
                  ))}
                </div>
              </article>
            ))}
            <aside className="extension-compatibility-note">
              <strong>离线兼容边界</strong>
              <p>当前运行时支持 Pi 风格的 <code>pi.registerTool</code>、<code>pi.hostcall</code> 与 <code>pi.tool</code>。扩展必须预先打包为单文件 JavaScript；TypeScript 入口、Node 内置模块和未打包的外部 import 会在安装预检时明确拒绝。</p>
            </aside>
          </section>
        )}
      </div>
    </div>
  );
}
