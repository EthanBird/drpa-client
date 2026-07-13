import { open } from "@tauri-apps/plugin-dialog";
import { Activity, Box, CheckCircle2, FolderOpen, KeyRound, LoaderCircle, Play, Plus, RotateCcw, Save, Search, ShieldCheck, SlidersHorizontal } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { useAppStore } from "../app/store";
import type { ParameterSummary } from "../domain/models";
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

  const selectedPackage = useMemo(() => snapshot?.packages.find((item) => item.id === selectedPackageId) ?? snapshot?.packages[0], [selectedPackageId, snapshot]);
  const selectedProfile = selectedPackage?.profiles.find((item) => item.id === selectedProfileId) ?? selectedPackage?.profiles[0];
  const storageKey = selectedPackage && selectedProfile ? `drpa-profile:${selectedPackage.id}:${selectedProfile.id}` : "";

  useEffect(() => {
    if (!storageKey) return;
    try { setValues(JSON.parse(localStorage.getItem(storageKey) ?? "{}") as ParameterValues); }
    catch { setValues({}); }
    setNotice(null);
  }, [storageKey]);

  if (!snapshot) return <div className="page"><div className="empty-state"><LoaderCircle className="spin" size={26} /><h2>正在加载工作区</h2></div></div>;
  if (!selectedPackage || !selectedProfile) return <div className="page"><div className="empty-state"><Box size={30} /><h2>还没有可运行的脚本包</h2><p>安装 `.rpaz` 或在开发工作室创建一个项目。</p><div className="header-actions"><button className="button secondary" type="button" onClick={() => setActiveNavigation("library")}>安装脚本包</button><button className="button primary" type="button" onClick={() => setActiveNavigation("studio")}><Plus size={15} /> 新建项目</button></div></div></div>;

  const save = () => {
    localStorage.setItem(storageKey, JSON.stringify(values));
    setNotice("参数已保存到当前任务配置");
  };
  const reset = () => {
    localStorage.removeItem(storageKey);
    setValues({});
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
      const runId = await desktopGateway.startRun(selectedPackage.id, selectedProfile.id, values);
      setSnapshot(await desktopGateway.getWorkspaceSnapshot());
      setNotice(`运行已结束：${runId}`);
    } catch (error) {
      setNotice(`运行失败：${String(error)}`);
      try { setSnapshot(await desktopGateway.getWorkspaceSnapshot()); } catch { /* keep the original runtime error */ }
    }
    finally { setIsStarting(false); }
  };

  const visiblePackages = snapshot.packages.filter((item) => `${item.name} ${item.id}`.toLowerCase().includes(query.toLowerCase()));

  return (
    <div className="page workbench-page">
      <header className="page-header workbench-header"><div><div className="breadcrumb"><span>运行工作台</span><span>/</span><strong>{selectedPackage.name}</strong></div><div className="page-title-row"><h1>{selectedProfile.name}</h1><span className="status-badge neutral"><CheckCircle2 size={11} /> 可编辑</span></div></div><div className="header-actions"><button className="button ghost" type="button" onClick={reset}><RotateCcw size={15} /> 重置</button><button className="button secondary" type="button" onClick={save}><Save size={15} /> 保存配置</button></div></header>
      <div className="workbench-grid">
        <aside className="package-rail"><div className="rail-toolbar"><div className="rail-search"><Search size={14} /><input aria-label="筛选脚本包" placeholder="筛选脚本包" value={query} onChange={(event) => setQuery(event.target.value)} /></div></div><div className="rail-section-label"><span>脚本包</span><span>{visiblePackages.length}</span></div><div className="package-list">{visiblePackages.map((item) => <button key={item.id} className={item.id === selectedPackage.id ? "package-row selected" : "package-row"} type="button" onClick={() => selectPackage(item.id, item.profiles[0]?.id)}><span className="package-avatar" style={{ "--package-accent": item.accent } as React.CSSProperties}>{item.initials}</span><span><strong>{item.name}</strong><small>{item.runtime} · v{item.version}</small></span></button>)}</div><div className="rail-divider" /><div className="rail-section-label"><span>任务配置</span></div><div className="profile-list">{selectedPackage.profiles.map((profile) => <button key={profile.id} className={profile.id === selectedProfile.id ? "profile-row selected" : "profile-row"} type="button" onClick={() => selectProfile(profile.id)}><span className="profile-file"><span /></span><span><strong>{profile.name}</strong><small>{profile.lastRun ?? "尚未运行"}</small></span></button>)}</div></aside>
        <section className="configuration-pane"><div className="pane-scroll"><div className="configuration-heading"><div><div className="eyebrow">任务参数</div><h2>{selectedPackage.name}</h2><p>参数来自 `manifest.yaml`，保存后用于当前任务配置。</p></div></div><div className="trust-banner verified"><ShieldCheck size={18} /><div><strong>本地脚本包</strong><span>安装时已完成路径、压缩规模和 manifest 校验。</span></div></div><section className="form-section"><header><span className="section-icon"><SlidersHorizontal size={15} /></span><div><h3>运行参数</h3><p>必填参数会在预检和运行前再次验证。</p></div></header><div className="form-section-body">{selectedPackage.parameters.length === 0 ? <div className="empty-inline">这个脚本包没有声明参数，可以直接运行。</div> : <div className="field-grid two-columns">{selectedPackage.parameters.map((parameter) => <ParameterField key={parameter.id} parameter={parameter} value={values[parameter.id]} onChange={(value) => setValues((current) => ({ ...current, [parameter.id]: value }))} />)}</div>}</div></section>{selectedPackage.parameters.some((item) => item.kind === "secret") && <section className="form-section"><header><span className="section-icon"><KeyRound size={15} /></span><div><h3>敏感参数</h3><p>密码输入不会显示明文；正式版将改为凭据句柄，不写入配置文件。</p></div></header></section>}</div><footer className="run-bar"><div className="preflight-state"><CheckCircle2 size={16} /><div><strong>等待运行</strong><span>{selectedPackage.parameters.length} 个参数 · 使用应用内封装 Python，运行日志会写入右侧</span></div></div>{notice && <div className="run-notice">{notice}</div>}<button className="button secondary" type="button" onClick={dryRun}>预检</button><button className="button primary run-button" type="button" onClick={startRun} disabled={isStarting}>{isStarting ? <LoaderCircle className="spin" size={16} /> : <Play size={15} fill="currentColor" />}{isStarting ? "正在运行…" : "运行任务"}</button></footer></section>
        <aside className="activity-inspector"><header className="inspector-header"><div><Activity size={16} /><strong>工作区日志</strong></div></header><div className="log-stream">{snapshot.logs.slice(-30).reverse().map((entry) => <div className={`log-line ${entry.level}`} key={entry.id}><span>{entry.time}</span><em>{entry.scope}</em><p>{entry.message}</p></div>)}</div></aside>
      </div>
    </div>
  );
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
