import {
  Activity,
  CheckCircle2,
  CircleAlert,
  Cpu,
  Database,
  Globe2,
  HardDrive,
  MemoryStick,
  RefreshCw,
  ShieldCheck,
  Wrench,
} from "lucide-react";
import type { ReactNode } from "react";
import { useCallback, useEffect, useState } from "react";

import { useNavigationSurfaceActive } from "../app/NavigationSurface";
import type {
  PlatformCapabilities,
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
  const [notice, setNotice] = useState("正在读取封装运行环境…");
  const [busy, setBusy] = useState(false);
  const [repairConfirmationOpen, setRepairConfirmationOpen] = useState(false);

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

  useEffect(() => {
    void loadRuntime();
    void desktopGateway.getPlatformCapabilities().then(setPlatform);
  }, [loadRuntime]);

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

  const refreshAll = () => {
    void loadRuntime();
    void loadMetrics();
  };

  const initialize = async () => {
    setBusy(true);
    setNotice("正在从安装包内的 wheelhouse 初始化并验证，过程不访问网络…");
    try {
      const next = await desktopGateway.initializeRuntime();
      setStatus(next);
      setNotice("运行环境验证通过：RPA Runtime、Jupyter Kernel 与浏览器模块均可导入");
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
            value={`Python ${status.pythonVersion}`}
            detail={`Bundle ${status.bundleVersion}`}
            path={status.runtimeRoot}
          />
          <RuntimeCard
            icon={<Database size={18} />}
            title="Jupyter Kernel"
            value="ipykernel · jupyter_client · ZMQ"
            detail="真实 Jupyter 消息协议，环境位于可写应用数据目录"
            path={status.environmentRoot}
          />
          <RuntimeCard
            icon={<Globe2 size={18} />}
            title="浏览器自动化"
            value="Chrome for Testing"
            detail="固定版本，与系统 Chrome 隔离"
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
          onClick={() => setRepairConfirmationOpen(true)}
          disabled={busy}
        >
          <Wrench size={14} /> 强制重建生成环境
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
                该操作会停止 Studio Kernel，删除并离线重建应用数据目录中的 runtime-environment，通常需要数分钟。
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
