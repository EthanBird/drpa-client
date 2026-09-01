import {
  Activity,
  CheckCircle2,
  CircleAlert,
  Cpu,
  Database,
  Download,
  FileArchive,
  Globe2,
  HardDrive,
  MemoryStick,
  PackagePlus,
  RefreshCw,
  Search,
  ShieldCheck,
  Trash2,
  Wrench,
} from "lucide-react";
import type { ReactNode } from "react";
import { useCallback, useEffect, useState } from "react";

import { useNavigationSurfaceActive } from "../app/NavigationSurface";
import type {
  PlatformCapabilities,
  RuntimeBrowserConfiguration,
  RuntimePythonPackageCatalog,
  RuntimeStatus,
  SystemDiskVolume,
  SystemMetricsSnapshot,
} from "../domain/models";
import { desktopGateway } from "../infra/gateway";

const METRICS_REFRESH_MS = 2_000;

export function RuntimePage() {
  const pageActive = useNavigationSurfaceActive();
  const [status, setStatus] = useState<RuntimeStatus | null>(null);
  const [platform, setPlatform] = useState<PlatformCapabilities | null>(null);
  const [metrics, setMetrics] = useState<SystemMetricsSnapshot | null>(null);
  const [metricsError, setMetricsError] = useState("");
  const [packageCatalog, setPackageCatalog] = useState<RuntimePythonPackageCatalog | null>(null);
  const [browserConfiguration, setBrowserConfiguration] = useState<RuntimeBrowserConfiguration | null>(null);
  const [browserBusy, setBrowserBusy] = useState(false);
  const [packageNotice, setPackageNotice] = useState("");
  const [packageQuery, setPackageQuery] = useState("");
  const [requirement, setRequirement] = useState("");
  const [packageBusy, setPackageBusy] = useState(false);
  const [selectedPackages, setSelectedPackages] = useState<string[]>([]);
  const [exportDialogOpen, setExportDialogOpen] = useState(false);
  const [exportDisplayName, setExportDisplayName] = useState("");
  const [exportDescription, setExportDescription] = useState("");
  const [exportBusy, setExportBusy] = useState(false);
  const [notice, setNotice] = useState("正在读取封装运行环境…");
  const [busy, setBusy] = useState(false);
  const [repairConfirmationOpen, setRepairConfirmationOpen] = useState(false);
  const selectedProfile = status?.profiles.find((profile) => profile.selected);
  const frozenProfile = selectedProfile?.environmentMode === "frozen";

  const loadRuntime = useCallback(async () => {
    try {
      const next = await desktopGateway.getRuntimeStatus();
      setStatus(next);
      setNotice(next.message);
    } catch (error) {
      setNotice(`无法读取运行环境：${String(error)}`);
    }
  }, []);

  const loadMetrics = useCallback(async () => {
    try {
      const next = await desktopGateway.getSystemMetrics();
      setMetrics(next);
      setMetricsError("");
    } catch (error) {
      setMetricsError(`无法读取本机资源：${String(error)}`);
    }
  }, []);

  const loadPackages = useCallback(async () => {
    try {
      const next = await desktopGateway.listRuntimePythonPackages();
      setPackageCatalog(next);
      setPackageNotice("");
    } catch (error) {
      setPackageCatalog(null);
      setPackageNotice(`无法读取 Python 包：${String(error)}`);
    }
  }, []);

  const loadBrowsers = useCallback(async () => {
    try {
      setBrowserConfiguration(await desktopGateway.listRuntimeBrowsers());
    } catch (error) {
      setPackageNotice(`无法读取浏览器环境：${String(error)}`);
    }
  }, []);

  useEffect(() => {
    void loadRuntime();
    void loadBrowsers();
    void desktopGateway.getPlatformCapabilities().then(setPlatform);
  }, [loadBrowsers, loadRuntime]);

  useEffect(() => {
    if (!pageActive) return;
    let disposed = false;
    let timer = 0;
    const refresh = async () => {
      await loadMetrics();
      if (!disposed) timer = window.setTimeout(refresh, METRICS_REFRESH_MS);
    };
    void refresh();
    return () => {
      disposed = true;
      window.clearTimeout(timer);
    };
  }, [loadMetrics, pageActive]);

  useEffect(() => {
    if (status?.state === "ready") void loadPackages();
  }, [loadPackages, status?.profileId, status?.state]);

  useEffect(() => {
    const available = new Set(packageCatalog?.packages.filter((item) => item.removable).map((item) => item.name) ?? []);
    setSelectedPackages((current) => current.filter((name) => available.has(name)));
  }, [packageCatalog]);

  const refreshAll = () => {
    void loadRuntime();
    void loadMetrics();
    void loadBrowsers();
    if (status?.state === "ready") void loadPackages();
  };

  const selectBrowser = async (path?: string, family?: "chromium" | "firefox") => {
    if (browserBusy) return;
    setBrowserBusy(true);
    setPackageNotice("正在切换浏览器环境并重置复用中的浏览器 Host…");
    try {
      const next = await desktopGateway.selectRuntimeBrowser(path, family);
      setBrowserConfiguration(next);
      await loadRuntime();
      setPackageNotice(next.message);
    } catch (error) {
      setPackageNotice(`切换浏览器失败：${String(error)}`);
    } finally {
      setBrowserBusy(false);
    }
  };

  const selectManualBrowser = async () => {
    const path = await desktopGateway.selectBrowserExecutable();
    if (!path) return;
    const family = /firefox/i.test(path.split(/[\\/]/).at(-1) ?? "") ? "firefox" : "chromium";
    await selectBrowser(path, family);
  };

  const initialize = async () => {
    setBusy(true);
    setNotice(frozenProfile
      ? "正在验证冻结型 Python Profile，过程不访问网络…"
      : "正在从安装包内的 wheelhouse 初始化并验证，过程不访问网络…");
    try {
      const next = await desktopGateway.initializeRuntime();
      setStatus(next);
      setNotice(`运行环境验证通过：${next.profileName}`);
    } catch (error) {
      setNotice(`初始化失败：${String(error)}`);
      await loadRuntime();
    } finally {
      setBusy(false);
    }
  };

  const repair = async () => {
    setBusy(true);
    setNotice("正在清理不完整环境并执行离线重建…");
    try {
      const next = await desktopGateway.repairRuntime();
      setStatus(next);
      setNotice("修复完成，全部运行依赖验证通过");
    } catch (error) {
      setNotice(`修复失败：${String(error)}`);
      await loadRuntime();
    } finally {
      setBusy(false);
    }
  };

  const confirmRepair = () => {
    setRepairConfirmationOpen(false);
    void repair();
  };

  const switchProfile = async (profileId: string) => {
    if (!status || status.profileId === profileId) return;
    setBusy(true);
    setPackageCatalog(null);
    setNotice("正在切换工作区默认 Python Profile；已启动任务继续使用原环境…");
    try {
      const next = await desktopGateway.selectRuntimeProfile(profileId);
      setStatus(next);
      setNotice(`已切换到 ${next.profileName}；新任务立即生效`);
    } catch (error) {
      setNotice(`切换失败：${String(error)}`);
      await loadRuntime();
    } finally {
      setBusy(false);
    }
  };

  const installPackage = async () => {
    const value = requirement.trim();
    if (!value) {
      setPackageNotice("请输入一个包名或 requirement，例如 pandas==2.3.0");
      return;
    }
    setPackageBusy(true);
    setPackageNotice(`正在通过 ${packageCatalog?.backend === "pip" ? "pip" : "uv"} 安装 ${value}…`);
    try {
      const next = await desktopGateway.installRuntimePythonPackage(value);
      setPackageCatalog(next);
      setRequirement("");
      setPackageNotice(`已安装 ${value}；新启动的 Kernel、Agent 和 RPAZ 任务立即可用`);
    } catch (error) {
      setPackageNotice(`安装失败：${String(error)}`);
    } finally {
      setPackageBusy(false);
    }
  };

  const uninstallPackage = async (packageName: string) => {
    setPackageBusy(true);
    setPackageNotice(`正在从当前 Profile 移除 ${packageName}…`);
    try {
      const next = await desktopGateway.uninstallRuntimePythonPackage(packageName);
      setPackageCatalog(next);
      setPackageNotice(`已移除 ${packageName}；基础 Runtime 未被修改`);
    } catch (error) {
      setPackageNotice(`卸载失败：${String(error)}`);
    } finally {
      setPackageBusy(false);
    }
  };

  const uninstallSelectedPackages = async () => {
    if (selectedPackages.length === 0) return;
    setPackageBusy(true);
    setPackageNotice(`正在从当前 Profile 批量移除 ${selectedPackages.length} 个用户包…`);
    try {
      const next = await desktopGateway.uninstallRuntimePythonPackages(selectedPackages);
      setPackageCatalog(next);
      setSelectedPackages([]);
      setPackageNotice(`已批量移除 ${selectedPackages.length} 个用户包；基础 Runtime 未被修改`);
    } catch (error) {
      setPackageNotice(`批量卸载失败：${String(error)}`);
    } finally {
      setPackageBusy(false);
    }
  };

  const openExportDialog = () => {
    if (!selectedProfile) return;
    const userPackageCount = packageCatalog?.packages.filter((item) => item.removable).length ?? 0;
    setExportDisplayName(`${selectedProfile.name} 离线组件`);
    setExportDescription(`基于 ${selectedProfile.name} 导出，包含当前 Profile 及 ${userPackageCount} 个用户安装包，可用于内网离线安装。`);
    setExportDialogOpen(true);
  };

  const exportProfile = async () => {
    const name = exportDisplayName.trim();
    const description = exportDescription.trim();
    if (!name || !description || exportBusy) return;
    const suggestedName = name.replace(/[\\/:*?"<>|]+/g, "-").replace(/\s+/g, "-").slice(0, 80) || "python-profile";
    const targetPath = await desktopGateway.selectRuntimeProfileExportPath(suggestedName);
    if (!targetPath) {
      setPackageNotice("已取消导出 Python Profile");
      return;
    }
    setExportBusy(true);
    setPackageNotice("正在合并 Runtime 与用户包并生成 .drpac，较大的环境可能需要几分钟…");
    try {
      const exported = await desktopGateway.exportRuntimeProfileComponent(targetPath, name, description);
      setExportDialogOpen(false);
      setPackageNotice(`已导出 ${exported.packageCount} 个用户包、${exported.fileCount} 个文件：${exported.path}`);
    } catch (error) {
      setPackageNotice(`导出失败：${String(error)}`);
    } finally {
      setExportBusy(false);
    }
  };

  const visiblePackages = packageCatalog?.packages.filter((item) => {
    const query = packageQuery.trim().toLocaleLowerCase();
    return !query || item.name.toLocaleLowerCase().includes(query) || item.version.toLocaleLowerCase().includes(query);
  }) ?? [];

  return (
    <div className="page runtime-page">
      <header className="page-header">
        <div>
          <div className="eyebrow">{platform ? `${platform.displayName} 离线运行基础设施` : "离线运行基础设施"}</div>
          <h1>运行环境</h1>
          <p>查看本机资源、真实发行内容和健康状态，不依赖系统 Python。</p>
        </div>
        <div className="header-actions">
          <button className="button secondary" type="button" onClick={refreshAll} disabled={busy}>
            <RefreshCw size={15} /> 刷新
          </button>
          <button
            className="button primary"
            type="button"
            onClick={status?.state === "broken" ? () => setRepairConfirmationOpen(true) : initialize}
            disabled={busy}
          >
            {status?.state === "broken" ? <Wrench size={15} /> : <ShieldCheck size={15} />}
            {busy ? "处理中…" : status?.state === "broken" ? "修复环境" : "初始化并验证"}
          </button>
        </div>
      </header>

      <SystemMonitor metrics={metrics} error={metricsError} />

      {status && status.profiles.length > 0 && (
        <section className="runtime-profiles panel" aria-label="Python 运行时 Profile">
          <header>
            <div>
              <strong>Python 运行时 Profile</strong>
              <small>工作区默认值 · 切换只影响新任务，运行中的任务由组件租约保护</small>
            </div>
            <span>{status.profiles.length} 个已安装</span>
          </header>
          <div className="runtime-profile-list">
            {status.profiles.map((profile) => (
              <article className={profile.selected ? "selected" : ""} key={profile.id}>
                <div className="runtime-profile-main">
                  <span>
                    <strong>{profile.name}</strong>
                    <small>Python {profile.pythonVersion} · {profile.environmentMode === "frozen" ? "冻结精简环境" : "隔离完整环境"}</small>
                  </span>
                  <em className={profile.ready ? "ready" : ""}>{profile.ready ? "已就绪" : "待初始化"}</em>
                </div>
                <div className="runtime-feature-list">
                  {profile.features.slice(0, 6).map((feature) => <code key={feature}>{feature}</code>)}
                </div>
                <footer>
                  <small>{profile.inUse > 0 ? `${profile.inUse} 个活动租约` : `组件 ${profile.componentVersion}`}</small>
                  {profile.selected && (
                    <button className="button secondary" type="button" disabled={busy || packageBusy || exportBusy} onClick={openExportDialog}>
                      <FileArchive size={14} /> 导出 .drpac
                    </button>
                  )}
                  <button
                    className={profile.selected ? "button secondary" : "button primary"}
                    type="button"
                    disabled={busy || packageBusy || profile.selected}
                    onClick={() => void switchProfile(profile.id)}
                  >
                    {profile.selected ? "当前使用" : "切换并用于新任务"}
                  </button>
                </footer>
              </article>
            ))}
          </div>
        </section>
      )}

      {status?.state === "ready" && (
        <section className="runtime-packages panel" aria-label="Python 包管理">
          <header>
            <div>
              <strong>Python 包管理</strong>
              <small>当前 Profile 独立用户包层 · 不修改只读 Runtime 组件</small>
            </div>
            <span className={`runtime-package-backend ${packageCatalog?.backend ?? "loading"}`}>
              {packageCatalog?.backend === "uv"
                ? packageCatalog.backendVersion || "uv"
                : packageCatalog?.backend === "pip"
                  ? "pip 降级模式"
                  : packageCatalog?.backend === "unavailable"
                    ? "无可用后端"
                    : "正在检测"}
            </span>
          </header>

          <form
            className="runtime-package-install"
            onSubmit={(event) => {
              event.preventDefault();
              void installPackage();
            }}
          >
            <label>
              <PackagePlus size={15} />
              <input
                value={requirement}
                onChange={(event) => setRequirement(event.target.value)}
                placeholder="包名或 requirement，例如 pandas==2.3.0"
                disabled={packageBusy || packageCatalog?.backend === "unavailable"}
                aria-label="要安装的 Python 包"
              />
            </label>
            <button
              className="button primary"
              type="submit"
              disabled={packageBusy || !requirement.trim() || packageCatalog?.backend === "unavailable"}
            >
              {packageBusy ? "处理中…" : "安装"}
            </button>
          </form>

          <div className="runtime-package-toolbar">
            <label>
              <Search size={14} />
              <input
                value={packageQuery}
                onChange={(event) => setPackageQuery(event.target.value)}
                placeholder="筛选已安装包"
                aria-label="筛选 Python 包"
              />
            </label>
            <div className="runtime-package-selection-tools">
              <button
                className="button secondary"
                type="button"
                disabled={packageBusy || visiblePackages.every((item) => !item.removable)}
                onClick={() => {
                  const visible = visiblePackages.filter((item) => item.removable).map((item) => item.name);
                  const allSelected = visible.length > 0 && visible.every((name) => selectedPackages.includes(name));
                  setSelectedPackages((current) => allSelected
                    ? current.filter((name) => !visible.includes(name))
                    : [...new Set([...current, ...visible])]);
                }}
              >
                全选用户包
              </button>
              <button className="button danger" type="button" disabled={packageBusy || selectedPackages.length === 0} onClick={() => void uninstallSelectedPackages()}>
                <Trash2 size={13} /> 删除所选 {selectedPackages.length || ""}
              </button>
              <span>
                {packageCatalog
                  ? `${packageCatalog.packages.filter((item) => item.removable).length} 个用户包 · ${packageCatalog.packages.length} 个包`
                  : "等待包清单"}
              </span>
            </div>
          </div>

          {packageNotice && <p className="runtime-package-notice" role="status">{packageNotice}</p>}
          <div className="runtime-package-list">
            {visiblePackages.map((item) => (
              <article key={`${item.source}:${item.name}`}>
                {item.removable ? (
                  <input
                    type="checkbox"
                    aria-label={`选择 ${item.name}`}
                    checked={selectedPackages.includes(item.name)}
                    disabled={packageBusy}
                    onChange={(event) => setSelectedPackages((current) => event.target.checked
                      ? [...new Set([...current, item.name])]
                      : current.filter((name) => name !== item.name))}
                  />
                ) : <span className="runtime-package-checkbox-placeholder" />}
                <span>
                  <strong>{item.name}</strong>
                  <small title={item.location}>{item.version || "未知版本"}</small>
                </span>
                <em className={item.removable ? "user" : "runtime"}>{item.removable ? "用户包" : "Runtime 内置"}</em>
                {item.removable ? (
                  <button
                    className="icon-button danger"
                    type="button"
                    title={`卸载 ${item.name}`}
                    aria-label={`卸载 ${item.name}`}
                    disabled={packageBusy}
                    onClick={() => void uninstallPackage(item.name)}
                  >
                    <Trash2 size={14} />
                  </button>
                ) : <span className="runtime-package-locked">只读</span>}
              </article>
            ))}
            {packageCatalog && visiblePackages.length === 0 && (
              <p className="runtime-package-empty">没有匹配的 Python 包</p>
            )}
          </div>
          {packageCatalog && (
            <footer title={packageCatalog.backendPath}>
              用户包目录：<code>{packageCatalog.overlayRoot}</code>
            </footer>
          )}
        </section>
      )}

      <section className="runtime-browsers panel" aria-label="浏览器运行环境">
        <header>
          <div>
            <strong>浏览器运行环境</strong>
            <small>自动发现系统与组件浏览器，也可固定到自定义可执行文件</small>
          </div>
          <div className="runtime-browser-actions">
            <button className="button secondary" type="button" disabled={browserBusy || browserConfiguration?.mode === "auto"} onClick={() => void selectBrowser()}>自动选择</button>
            <button className="button secondary" type="button" disabled={browserBusy} onClick={() => void selectManualBrowser()}><Search size={13} /> 手动选择</button>
          </div>
        </header>
        {browserConfiguration ? (
          <>
            <div className="runtime-browser-current">
              <span className={browserConfiguration.automationCompatible ? "ready" : browserConfiguration.activeFamily === "firefox" ? "warning" : "missing"}><Globe2 size={17} /></span>
              <div><strong>{browserConfiguration.activeName}</strong><small title={browserConfiguration.activeExecutable}>{browserConfiguration.activeExecutable || "尚未配置可执行文件"}</small></div>
              <em>{browserConfiguration.activeFamily === "firefox" ? "Firefox" : browserConfiguration.activeFamily === "chromium" ? "Chromium" : "未配置"}</em>
            </div>
            <div className="runtime-browser-candidates">
              {browserConfiguration.candidates.map((candidate) => (
                <button className={candidate.selected ? "selected" : ""} type="button" key={candidate.id} disabled={browserBusy || candidate.selected} onClick={() => void selectBrowser(candidate.executable, candidate.family === "firefox" ? "firefox" : "chromium")}>
                  <span><strong>{candidate.name}</strong><small>{candidate.source} · {candidate.family === "firefox" ? "Firefox" : "Chromium"}</small></span>
                  <em>{candidate.selected ? "当前" : "切换"}</em>
                </button>
              ))}
            </div>
            <p className={browserConfiguration.automationCompatible ? "runtime-browser-message" : "runtime-browser-message warning"}>{browserConfiguration.message}</p>
          </>
        ) : <p className="runtime-browser-message">正在检测 Chrome、Edge、Chromium、Brave、Vivaldi、Opera 与 Firefox…</p>}
      </section>

      <div className={`runtime-banner ${status?.state ?? "loading"}`}>
        <span>{status?.state === "ready" ? <CheckCircle2 size={20} /> : <Cpu size={20} />}</span>
        <div>
          <strong>
            {status?.state === "ready"
              ? "运行环境健康"
              : status?.state === "broken"
                ? "检测到不完整环境"
                : "等待首次初始化"}
          </strong>
          <p>{notice}</p>
        </div>
      </div>

      {status && (
        <div className="runtime-grid">
          <RuntimeCard
            icon={<Cpu size={18} />}
            title="Python + RPA"
            value={`${status.profileName} · Python ${status.pythonVersion}`}
            detail={`Bundle ${status.bundleVersion} · ${status.features.length} 项能力`}
            path={status.runtimeRoot}
          />
          <RuntimeCard
            icon={<Database size={18} />}
            title="Jupyter Kernel"
            value={status.features.includes("jupyter") ? "ipykernel · jupyter_client · ZMQ" : "DRPA 轻量 Kernel"}
            detail={status.features.includes("jupyter") ? "真实 Jupyter 消息协议，环境按 Profile 摘要隔离" : "仅提供标准库执行能力，适合轻量任务"}
            path={status.environmentRoot}
          />
          <RuntimeCard
            icon={<Globe2 size={18} />}
            title="浏览器自动化"
            value={status.browserName || (status.browserExecutable.includes("未安装") ? "未配置浏览器" : "Chromium 兼容浏览器")}
            detail={status.browserFamily === "firefox" ? "Firefox 已选中；DrissionPage 工具需要切换到 Chromium 浏览器" : "系统浏览器、可拔插组件与手动路径均可切换"}
            path={status.browserExecutable}
          />
        </div>
      )}

      <section className="panel runtime-policy">
        <div>
          <ShieldCheck size={18} />
          <span>
            <strong>离线与数据策略</strong>
            <small>
              Python、wheelhouse、Jupyter 与浏览器均随当前平台发行包交付；生成环境、缓存、项目和日志只写入
              {platform?.dataDirectoryPolicy ?? "应用数据目录"}。
            </small>
          </span>
        </div>
        <button
          className="button danger"
          type="button"
          onClick={frozenProfile ? () => void initialize() : () => setRepairConfirmationOpen(true)}
          disabled={busy}
        >
          <Wrench size={14} /> {frozenProfile ? "重新验证冻结环境" : "强制重建生成环境"}
        </button>
      </section>

      {repairConfirmationOpen && (
        <div
          className="knowledge-confirm-overlay"
          role="dialog"
          aria-modal="true"
          aria-label="确认强制重建运行环境"
          onMouseDown={() => setRepairConfirmationOpen(false)}
        >
          <section className="knowledge-confirm" onMouseDown={(event) => event.stopPropagation()}>
            <div className="knowledge-confirm-icon"><CircleAlert size={18} /></div>
            <div>
              <h2>强制重建整个运行环境？</h2>
              <p>
                该操作会停止 Studio Kernel，只删除并离线重建当前 Profile 的摘要隔离环境，通常需要数分钟。
                RPAZ 包、项目和运行产物会保留。
              </p>
            </div>
            <footer>
              <button className="button secondary" type="button" onClick={() => setRepairConfirmationOpen(false)}>取消</button>
              <button className="button danger" type="button" onClick={confirmRepair}>确认重建</button>
            </footer>
          </section>
        </div>
      )}

      {exportDialogOpen && (
        <div className="knowledge-confirm-overlay" role="dialog" aria-modal="true" aria-label="导出 Python Profile" onMouseDown={() => { if (!exportBusy) setExportDialogOpen(false); }}>
          <section className="knowledge-confirm runtime-export-dialog" onMouseDown={(event) => event.stopPropagation()}>
            <div className="knowledge-confirm-icon"><Download size={18} /></div>
            <div>
              <h2>导出为离线 .drpac 组件</h2>
              <p>会复制当前 Python Runtime，并把用户包层合并进组件；源 Profile 和用户包目录不会被修改。</p>
              <label><span>组件名称</span><input value={exportDisplayName} maxLength={120} disabled={exportBusy} onChange={(event) => setExportDisplayName(event.target.value)} /></label>
              <label><span>组件描述</span><textarea value={exportDescription} maxLength={1000} rows={4} disabled={exportBusy} onChange={(event) => setExportDescription(event.target.value)} /></label>
            </div>
            <footer>
              <button className="button secondary" type="button" disabled={exportBusy} onClick={() => setExportDialogOpen(false)}>取消</button>
              <button className="button primary" type="button" disabled={exportBusy || !exportDisplayName.trim() || !exportDescription.trim()} onClick={() => void exportProfile()}>
                <FileArchive size={14} /> {exportBusy ? "正在打包…" : "选择位置并导出"}
              </button>
            </footer>
          </section>
        </div>
      )}
    </div>
  );
}

function SystemMonitor({ metrics, error }: { metrics: SystemMetricsSnapshot | null; error: string }) {
  const cores = metrics?.cpu.logicalCores ?? 0;
  const busyCores = metrics ? cores * metrics.cpu.usagePercent / 100 : 0;
  const volumes = metrics?.disk.volumes ?? [];

  return (
    <section className="runtime-monitor" aria-label="本机资源监控">
      <header>
        <div>
          <Activity size={17} />
          <span>
            <strong>本机资源监控</strong>
            <small>普通用户权限 · 每 2 秒自动刷新</small>
          </span>
        </div>
        <span className={error ? "runtime-monitor-state error" : "runtime-monitor-state"}>
          <i />
          {error
            ? "读取异常"
            : metrics
              ? `更新于 ${new Date(metrics.sampledAt).toLocaleTimeString()}`
              : "正在采样"}
        </span>
      </header>

      {error && !metrics && <p className="runtime-monitor-error"><CircleAlert size={14} />{error}</p>}

      <div className="runtime-monitor-grid">
        <ResourceCard
          icon={<Cpu size={18} />}
          title="CPU"
          percent={metrics?.cpu.usagePercent}
          value={metrics ? `${formatPercent(metrics.cpu.usagePercent)} 使用中` : "正在采样…"}
          detail={metrics ? `${busyCores.toFixed(1)} / ${cores} 个逻辑核心` : "等待首个计数器差值"}
        />
        <ResourceCard
          icon={<MemoryStick size={18} />}
          title="内存"
          percent={metrics?.memory.usagePercent}
          value={metrics ? `${formatBytes(metrics.memory.usedBytes)} / ${formatBytes(metrics.memory.totalBytes)}` : "正在采样…"}
          detail={metrics ? `${formatPercent(metrics.memory.usagePercent)} 已使用` : "读取物理内存"}
        />
        <ResourceCard
          icon={<HardDrive size={18} />}
          title="磁盘"
          percent={metrics?.disk.usagePercent}
          value={metrics ? `${formatBytes(metrics.disk.usedBytes)} / ${formatBytes(metrics.disk.totalBytes)}` : "正在采样…"}
          detail={metrics ? `${volumes.length} 个本地卷 · ${formatPercent(metrics.disk.usagePercent)} 已使用` : "枚举本地磁盘"}
          volumes={volumes}
        />
      </div>
    </section>
  );
}

function ResourceCard({
  icon,
  title,
  percent,
  value,
  detail,
  volumes,
}: {
  icon: ReactNode;
  title: string;
  percent?: number;
  value: string;
  detail: string;
  volumes?: SystemDiskVolume[];
}) {
  const normalized = Math.max(0, Math.min(100, percent ?? 0));
  return (
    <article className="runtime-resource-card">
      <header>{icon}<span>{title}</span><strong>{percent === undefined ? "—" : formatPercent(percent)}</strong></header>
      <div
        className="runtime-resource-progress"
        role="progressbar"
        aria-label={`${title}使用率`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(normalized)}
      >
        <i style={{ width: `${normalized}%` }} />
      </div>
      <p>{value}</p>
      <small>{detail}</small>
      {volumes && volumes.length > 0 && (
        <div className="runtime-volume-list">
          {volumes.map((volume) => (
            <div key={volume.mountPoint} title={`${volume.name} · ${volume.mountPoint}`}>
              <span>{volume.mountPoint}</span>
              <em>{formatBytes(volume.usedBytes)} / {formatBytes(volume.totalBytes)}</em>
              <strong>{formatPercent(volume.usagePercent)}</strong>
            </div>
          ))}
        </div>
      )}
    </article>
  );
}

function RuntimeCard({ icon, title, value, detail, path }: {
  icon: ReactNode;
  title: string;
  value: string;
  detail: string;
  path: string;
}) {
  return (
    <article className="runtime-card">
      <header>{icon}<span>{title}</span></header>
      <strong>{value}</strong>
      <p>{detail}</p>
      <code title={path}>{path}</code>
    </article>
  );
}

function formatBytes(bytes: number) {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** index;
  return `${value >= 100 || index === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[index]}`;
}

function formatPercent(value: number) {
  return `${Math.max(0, Math.min(100, value)).toFixed(1)}%`;
}
