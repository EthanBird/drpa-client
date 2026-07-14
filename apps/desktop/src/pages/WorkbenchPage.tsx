import { open } from "@tauri-apps/plugin-dialog";
import { Activity, Box, CheckCircle2, FolderOpen, KeyRound, LoaderCircle, Play, Plus, RotateCcw, Save, Search, ShieldCheck, SlidersHorizontal } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { useAppStore } from "../app/store";
import type { ParameterSummary, TaskProfile } from "../domain/models";
import { desktopGateway } from "../infra/gateway";

type ParameterValues = Record<string, string | number | boolean>;

export function WorkbenchPage() {
  const snapshot = useAppStore((state) => state.snapshot);
  const setSnapshot = useAppStore((state) => state.setSnapshot);
  const selectedPackageId = useAppStore((state) => state.selectedPackageId);
  const selectedProfileId = useAppStore((state) => state.selectedProfileId);
  const selectPackage = useAppStore((state) => state.selectPackage);
  const selectProfile = useAppStore((state) => state.selectProfile);
  const setActiveNavigation = useAppStore((state) => state.setActiveNavigation);
  const [values, setValues] = useState<ParameterValues>({});
  const [notice, setNotice] = useState<string | null>(null);
  const [isStarting, setIsStarting] = useState(false);
  const [query, setQuery] = useState("");
  const [localProfiles, setLocalProfiles] = useState<TaskProfile[]>([]);
  const [logQuery, setLogQuery] = useState("");
  const [logFilter, setLogFilter] = useState<"all" | "warning" | "error">("all");

  const selectedPackage = useMemo(() => snapshot?.packages.find((item) => item.id === selectedPackageId) ?? snapshot?.packages[0], [selectedPackageId, snapshot]);
  const localProfileKey = selectedPackage ? `drpa-local-profiles:${selectedPackage.id}` : "";
  const profiles = useMemo(() => [...(selectedPackage?.profiles ?? []), ...localProfiles], [localProfiles, selectedPackage]);
  const selectedProfile = profiles.find((item) => item.id === selectedProfileId) ?? profiles[0];
  const storageKey = selectedPackage && selectedProfile ? `drpa-profile:${selectedPackage.id}:${selectedProfile.id}` : "";
  const latestRun = snapshot?.runs[0];
  const visibleLogs = useMemo(() => (snapshot?.logs ?? []).filter((entry) => {
    const matchesLevel = logFilter === "all" || entry.level === logFilter;
    const text = `${entry.scope} ${entry.message}`.toLowerCase();
    return matchesLevel && text.includes(logQuery.toLowerCase());
  }).slice(-200), [logFilter, logQuery, snapshot]);

  useEffect(() => {
    if (!localProfileKey) { setLocalProfiles([]); return; }
    try {
      const stored = JSON.parse(localStorage.getItem(localProfileKey) ?? "[]") as TaskProfile[];
      const runtimeProfileId = selectedPackage?.profiles[0]?.id;
      setLocalProfiles(stored.map((profile) => ({ ...profile, runtimeProfileId: profile.runtimeProfileId ?? runtimeProfileId })));
    }
    catch { setLocalProfiles([]); }
  }, [localProfileKey, selectedPackage]);

  useEffect(() => {
    if (!storageKey || !selectedPackage) return;
    const defaults = defaultValues(selectedPackage.parameters);
    try { setValues({ ...defaults, ...(JSON.parse(localStorage.getItem(storageKey) ?? "{}") as ParameterValues) }); }
    catch { setValues(defaults); }
  }, [selectedPackage, storageKey]);

  if (!snapshot) return <div className="page"><div className="empty-state"><LoaderCircle className="spin" size={26} /><h2>正在加载工作区</h2></div></div>;
  if (!selectedPackage || !selectedProfile) return <div className="page"><div className="empty-state"><Box size={30} /><h2>还没有可运行的脚本包</h2><p>安装 `.rpaz` 或在开发工作室创建一个项目。</p><div className="header-actions"><button className="button secondary" type="button" onClick={() => setActiveNavigation("library")}>安装脚本包</button><button className="button primary" type="button" onClick={() => setActiveNavigation("studio")}><Plus size={15} /> 新建项目</button></div></div></div>;

  const save = () => {
    localStorage.setItem(storageKey, JSON.stringify(values));
    setNotice("参数已保存到当前任务配置");
  };
  const reset = () => {
    localStorage.removeItem(storageKey);
    setValues(defaultValues(selectedPackage.parameters));
    setNotice("参数已恢复默认值");
  };
  const validate = () => {
    const missing = selectedPackage.parameters.filter((parameter) => parameter.required && (values[parameter.id] === undefined || values[parameter.id] === ""));
    if (missing.length) {
      setNotice(`请填写必填参数：${missing.map((item) => item.id).join("、")}`);
      return false;
    }
    return true;
  };
  const dryRun = () => {
    if (validate()) setNotice(`预检通过：${selectedPackage.parameters.length} 个参数有效`);
  };
  const startRun = async () => {
    if (!validate()) return;
    save();
    setIsStarting(true);
    try {
      const runId = await desktopGateway.startRun(selectedPackage.id, selectedProfile.runtimeProfileId ?? selectedProfile.id, values);
      setSnapshot(await desktopGateway.getWorkspaceSnapshot());
      setNotice(`运行已结束：${runId}`);
    } catch (error) {
      setNotice(`运行失败：${String(error)}`);
      try { setSnapshot(await desktopGateway.getWorkspaceSnapshot()); } catch { /* keep the original runtime error */ }
    }
    finally { setIsStarting(false); }
  };


  const createTaskProfile = () => {
    if (!selectedPackage) return;
    const name = window.prompt("任务配置名称", `${selectedPackage.name} 配置 ${profiles.length + 1}`)?.trim();
    if (!name) return;
    const profile: TaskProfile = {
      id: `local-${Date.now().toString(36)}`,
      name,
      lastRun: "本地配置",
      runtimeProfileId: selectedPackage.profiles[0]?.id ?? selectedProfile.id,
    };
    const nextProfiles = [...localProfiles, profile];
    setLocalProfiles(nextProfiles);
    localStorage.setItem(localProfileKey, JSON.stringify(nextProfiles));
    const defaults = defaultValues(selectedPackage.parameters);
    localStorage.setItem(`drpa-profile:${selectedPackage.id}:${profile.id}`, JSON.stringify(defaults));
    selectProfile(profile.id);
    setValues(defaults);
    setNotice(`已创建任务配置：${name}`);
  };

  const visiblePackages = snapshot.packages.filter((item) => `${item.name} ${item.id}`.toLowerCase().includes(query.toLowerCase()));

  return (
    <div className="page workbench-page">
      <header className="page-header workbench-header"><div><div className="breadcrumb"><span>运行工作台</span><span>/</span><strong>{selectedPackage.name}</strong></div><div className="page-title-row"><h1>{selectedProfile.name}</h1><span className="status-badge neutral"><CheckCircle2 size={11} /> 可编辑</span></div></div><div className="header-actions"><button className="button ghost" type="button" onClick={reset}><RotateCcw size={15} /> 重置</button><button className="button secondary" type="button" onClick={save}><Save size={15} /> 保存配置</button></div></header>
      <div className="workbench-grid">
        <aside className="package-rail"><div className="rail-toolbar"><div className="rail-search"><Search size={14} /><input aria-label="筛选脚本包" placeholder="筛选脚本包" value={query} onChange={(event) => setQuery(event.target.value)} /></div></div><div className="rail-section-label"><span>脚本包</span><span>{visiblePackages.length}</span></div><div className="package-list">{visiblePackages.map((item) => <button key={item.id} className={item.id === selectedPackage.id ? "package-row selected" : "package-row"} type="button" onClick={() => selectPackage(item.id, item.profiles[0]?.id)}><span className="package-avatar" style={{ "--package-accent": item.accent } as React.CSSProperties}>{item.initials}</span><span><strong>{item.name}</strong><small>{item.runtime} · v{item.version}</small></span></button>)}</div><div className="rail-divider" /><div className="rail-section-label"><span>任务配置</span><button className="icon-button subtle" type="button" title="创建任务配置" aria-label="创建任务配置" onClick={createTaskProfile}><Plus size={13} /></button></div><div className="profile-list">{profiles.map((profile) => <button key={profile.id} className={profile.id === selectedProfile.id ? "profile-row selected" : "profile-row"} type="button" onClick={() => selectProfile(profile.id)}><span className="profile-file"><span /></span><span><strong>{profile.name}</strong><small>{profile.lastRun ?? "尚未运行"}</small></span></button>)}</div></aside>
        <section className="configuration-pane"><div className="pane-scroll"><div className="configuration-heading"><div><div className="eyebrow">任务参数</div><h2>{selectedPackage.name}</h2><p>参数来自 `manifest.yaml`，保存后用于当前任务配置。</p></div></div><div className="trust-banner verified"><ShieldCheck size={18} /><div><strong>本地脚本包</strong><span>安装时已完成路径、压缩规模和 manifest 校验。</span></div></div><section className="form-section"><header><span className="section-icon"><SlidersHorizontal size={15} /></span><div><h3>运行参数</h3><p>必填参数会在预检和运行前再次验证。</p></div></header><div className="form-section-body">{selectedPackage.parameters.length === 0 ? <div className="empty-inline">这个脚本包没有声明参数，可以直接运行。</div> : <div className="field-grid two-columns">{selectedPackage.parameters.map((parameter) => <ParameterField key={parameter.id} parameter={parameter} value={values[parameter.id]} onChange={(value) => setValues((current) => ({ ...current, [parameter.id]: value }))} />)}</div>}</div></section>{selectedPackage.parameters.some((item) => item.kind === "secret") && <section className="form-section"><header><span className="section-icon"><KeyRound size={15} /></span><div><h3>敏感参数</h3><p>密码输入不会显示明文；正式版将改为凭据句柄，不写入配置文件。</p></div></header></section>}</div><footer className="run-bar"><div className="preflight-state"><CheckCircle2 size={16} /><div><strong>等待运行</strong><span>{selectedPackage.parameters.length} 个参数 · 使用应用内封装 Python，运行日志会写入右侧</span></div></div>{notice && <div className="run-notice">{notice}</div>}<button className="button secondary" type="button" onClick={dryRun}>预检</button><button className="button primary run-button" type="button" onClick={startRun} disabled={isStarting}>{isStarting ? <LoaderCircle className="spin" size={16} /> : <Play size={15} fill="currentColor" />}{isStarting ? "正在运行…" : "运行任务"}</button></footer></section>
        <aside className="activity-inspector">
          <header className="inspector-header">
            <div><Activity size={16} /><strong>运行日志</strong></div>
            <span className="live-pill"><span /> live</span>
          </header>
          <section className="run-summary">
            <div className="run-summary-title">
              <span className="running-indicator">{latestRun?.status === "running" ? <LoaderCircle className="spin" size={15} /> : <CheckCircle2 size={15} />}</span>
              <div><strong>{latestRun?.packageName ?? selectedPackage.name}</strong><span>{latestRun ? `${latestRun.id} · ${latestRun.profileName}` : "等待首次运行"}</span></div>
              <span>{latestRun?.progress ?? (latestRun?.status === "success" ? 100 : 0)}%</span>
            </div>
            <div className="progress-track"><span style={{ width: `${latestRun?.progress ?? (latestRun?.status === "success" ? 100 : 0)}%` }} /></div>
            <div className="run-stats"><span>状态 · {latestRun?.status ?? "idle"}</span><span>开始 · {latestRun?.startedAt ?? "--:--:--"}</span><span>耗时 · {latestRun?.duration ?? "--"}</span></div>
          </section>
          <nav className="inspector-tabs" aria-label="日志级别">
            <button type="button" className={logFilter === "all" ? "active" : ""} onClick={() => setLogFilter("all")}>全部 <span>{snapshot.logs.length}</span></button>
            <button type="button" className={logFilter === "warning" ? "active" : ""} onClick={() => setLogFilter("warning")}>警告 <span>{snapshot.logs.filter((entry) => entry.level === "warning").length}</span></button>
            <button type="button" className={logFilter === "error" ? "active" : ""} onClick={() => setLogFilter("error")}>错误 <span>{snapshot.logs.filter((entry) => entry.level === "error").length}</span></button>
          </nav>
          <div className="log-toolbar"><Search size={13} /><input aria-label="搜索运行日志" placeholder="搜索消息或作用域" value={logQuery} onChange={(event) => setLogQuery(event.target.value)} /><button className="follow-button active" type="button"><span /> 跟随最新</button></div>
          <div className="log-view" role="log" aria-live="polite">
            {visibleLogs.map((entry) => <div className={`log-row level-${entry.level}`} key={entry.id}><span className="log-time">{entry.time}</span><span className="log-level">{logLevelLabel(entry.level)}</span><span className="log-scope">{entry.scope}</span><span className="log-message">{entry.message}</span></div>)}
            {visibleLogs.length === 0 && <div className="log-empty">没有匹配的日志记录</div>}
            <div className="log-cursor"><span /> 等待运行时事件</div>
          </div>
          <footer className="inspector-footer"><span className="status-badge neutral">保留最近 200 条</span><span className="footer-spacer" /><span className="status-badge success">UTF-8 容错解码</span></footer>
        </aside>
      </div>
    </div>
  );
}


function defaultValues(parameters: ParameterSummary[]): ParameterValues {
  return parameters.reduce<ParameterValues>((accumulator, parameter) => {
    if (parameter.defaultValue !== undefined) accumulator[parameter.id] = parameter.defaultValue;
    return accumulator;
  }, {});
}

function logLevelLabel(level: "trace" | "info" | "success" | "warning" | "error") {
  return { trace: "TRC", info: "INF", success: "OK", warning: "WRN", error: "ERR" }[level];
}

function ParameterField({ parameter, value, onChange }: { parameter: ParameterSummary; value: string | number | boolean | undefined; onChange: (value: string | number | boolean) => void }) {
  const label = parameter.id.replaceAll("_", " ");
  if (parameter.kind === "boolean") return <label className="field checkbox-field"><span className="field-label">{label}{parameter.required && <em>必填</em>}</span><button className={`switch ${value ? "on" : ""}`} type="button" onClick={() => onChange(!value)} aria-pressed={Boolean(value)}><span /></button></label>;
  const selectPath = async () => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const selected = await open({ directory: parameter.kind === "directory", multiple: false });
    if (selected) onChange(selected);
  };
  return <label className="field"><span className="field-label">{label}{parameter.required && <em>必填</em>}</span><div className="input-with-action"><input type={parameter.kind === "secret" ? "password" : parameter.kind === "number" ? "number" : "text"} value={typeof value === "boolean" ? "" : value ?? ""} onChange={(event) => onChange(parameter.kind === "number" ? Number(event.target.value) : event.target.value)} placeholder={`请输入 ${label}`} />{(parameter.kind === "file" || parameter.kind === "directory") && <button type="button" onClick={selectPath}><FolderOpen size={14} /> 选择</button>}</div></label>;
}
