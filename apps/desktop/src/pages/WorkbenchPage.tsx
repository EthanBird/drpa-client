import {
  Activity,
  Archive,
  Box,
  Check,
  ChevronDown,
  ChevronsUpDown,
  CircleCheck,
  CircleDot,
  Clock3,
  Copy,
  Ellipsis,
  ExternalLink,
  FileOutput,
  Filter,
  FolderOpen,
  History,
  KeyRound,
  ListFilter,
  LoaderCircle,
  LockKeyhole,
  PanelRightClose,
  PanelRightOpen,
  Pause,
  Play,
  Plus,
  RotateCcw,
  Search,
  Settings2,
  ShieldCheck,
  SlidersHorizontal,
  Square,
  TerminalSquare,
} from "lucide-react";
import { useMemo, useState } from "react";

import { useAppStore } from "../app/store";
import type { LogEntry, PackageSummary, TrustLevel } from "../domain/models";
import { desktopGateway } from "../infra/gateway";

export function WorkbenchPage() {
  const snapshot = useAppStore((state) => state.snapshot);
  const selectedPackageId = useAppStore((state) => state.selectedPackageId);
  const selectedProfileId = useAppStore((state) => state.selectedProfileId);
  const selectPackage = useAppStore((state) => state.selectPackage);
  const selectProfile = useAppStore((state) => state.selectProfile);
  const inspectorOpen = useAppStore((state) => state.inspectorOpen);
  const toggleInspector = useAppStore((state) => state.toggleInspector);
  const [isStarting, setIsStarting] = useState(false);
  const [runNotice, setRunNotice] = useState<string | null>(null);
  const [followLogs, setFollowLogs] = useState(true);

  const selectedPackage = useMemo(
    () => snapshot?.packages.find((item) => item.id === selectedPackageId) ?? snapshot?.packages[0],
    [selectedPackageId, snapshot],
  );
  const selectedProfile = selectedPackage?.profiles.find((item) => item.id === selectedProfileId)
    ?? selectedPackage?.profiles[0];

  if (!snapshot || !selectedPackage || !selectedProfile) {
    return <WorkbenchSkeleton />;
  }

  const startRun = async () => {
    setIsStarting(true);
    setRunNotice(null);
    try {
      const runId = await desktopGateway.startRun(selectedPackage.id, selectedProfile.id);
      setRunNotice(`Started ${runId}`);
    } finally {
      setIsStarting(false);
    }
  };

  return (
    <div className="page workbench-page">
      <PageHeader
        selectedPackage={selectedPackage}
        selectedProfileName={selectedProfile.name}
        inspectorOpen={inspectorOpen}
        onToggleInspector={toggleInspector}
      />

      <div className={inspectorOpen ? "workbench-grid" : "workbench-grid inspector-collapsed"}>
        <PackageRail
          packages={snapshot.packages}
          selectedPackageId={selectedPackage.id}
          selectedProfileId={selectedProfile.id}
          onSelectPackage={(packageId) => {
            const nextPackage = snapshot.packages.find((item) => item.id === packageId);
            selectPackage(packageId, nextPackage?.profiles[0]?.id);
          }}
          onSelectProfile={selectProfile}
        />

        <section className="configuration-pane" aria-label="Task configuration">
          <div className="pane-scroll">
            <div className="configuration-heading">
              <div>
                <div className="eyebrow">Task profile</div>
                <h2>{selectedProfile.name}</h2>
                <p>Reusable configuration for {selectedPackage.name}.</p>
              </div>
              <button className="icon-button" type="button" aria-label="Task profile actions">
                <Ellipsis size={17} />
              </button>
            </div>

            <TrustBanner trust={selectedPackage.trust} />

            <FormSection
              icon={<SlidersHorizontal size={15} />}
              title="Run parameters"
              description="Values are validated against the package schema before the run starts."
            >
              <div className="field-grid two-columns">
                <Field label="Account" required>
                  <div className="select-control">
                    <span>North region · Billing</span><ChevronsUpDown size={14} />
                  </div>
                </Field>
                <Field label="Statement period" required>
                  <div className="select-control">
                    <span>July 2026</span><ChevronDown size={14} />
                  </div>
                </Field>
                <Field label="Supplier group">
                  <div className="select-control">
                    <span>All active suppliers</span><ChevronDown size={14} />
                  </div>
                </Field>
                <Field label="Output format">
                  <div className="segmented-control" role="group" aria-label="Output format">
                    <button className="active" type="button">XLSX</button>
                    <button type="button">CSV</button>
                    <button type="button">JSON</button>
                  </div>
                </Field>
              </div>
              <Field label="Output directory" hint="Artifacts stay isolated per run.">
                <div className="input-with-action">
                  <FolderOpen size={15} />
                  <span>Workspace / Invoice Hub / Monthly close</span>
                  <button type="button">Choose</button>
                </div>
              </Field>
            </FormSection>

            <FormSection
              icon={<KeyRound size={15} />}
              title="Credentials"
              description="Profiles store references. Secret values are resolved only when a run starts."
            >
              <div className="secret-binding">
                <div className="secret-icon"><LockKeyhole size={16} /></div>
                <div>
                  <strong>Supplier portal · North</strong>
                  <span>Workspace secret · Updated 3 days ago</span>
                </div>
                <span className="status-badge success"><Check size={12} /> Available</span>
                <button className="button secondary small" type="button">Change</button>
              </div>
            </FormSection>

            <details className="advanced-section">
              <summary>
                <span><Settings2 size={15} /> Advanced runtime options</span>
                <ChevronDown size={14} />
              </summary>
              <div className="advanced-content">Retry, timeout and browser behavior are package-scoped advanced parameters.</div>
            </details>
          </div>

          <footer className="run-bar">
            <div className="preflight-state">
              <CircleCheck size={16} />
              <div><strong>Ready to run</strong><span>7 checks passed · Runtime cached</span></div>
            </div>
            {runNotice && <div className="run-notice">{runNotice}</div>}
            <button className="button secondary" type="button"><History size={15} /> Dry run</button>
            <button className="button primary run-button" type="button" onClick={startRun} disabled={isStarting}>
              {isStarting ? <LoaderCircle className="spin" size={16} /> : <Play size={15} fill="currentColor" />}
              {isStarting ? "Starting…" : `Run ${selectedProfile.name}`}
              <kbd>⌘ ↵</kbd>
            </button>
          </footer>
        </section>

        {inspectorOpen && (
          <ActivityInspector
            logs={snapshot.logs}
            followLogs={followLogs}
            onToggleFollow={() => setFollowLogs((value) => !value)}
          />
        )}
      </div>
    </div>
  );
}

function PageHeader({
  selectedPackage,
  selectedProfileName,
  inspectorOpen,
  onToggleInspector,
}: {
  selectedPackage: PackageSummary;
  selectedProfileName: string;
  inspectorOpen: boolean;
  onToggleInspector: () => void;
}) {
  return (
    <header className="page-header workbench-header">
      <div>
        <div className="breadcrumb"><span>Workbench</span><span>/</span><strong>{selectedPackage.name}</strong></div>
        <div className="page-title-row">
          <h1>{selectedProfileName}</h1>
          <span className="status-badge neutral"><CircleDot size={11} /> Saved</span>
        </div>
      </div>
      <div className="header-actions">
        <button className="button ghost" type="button"><RotateCcw size={15} /> Reset</button>
        <button className="button secondary" type="button"><Archive size={15} /> Save profile</button>
        <button className="icon-button" type="button" onClick={onToggleInspector} aria-label="Toggle activity inspector">
          {inspectorOpen ? <PanelRightClose size={17} /> : <PanelRightOpen size={17} />}
        </button>
      </div>
    </header>
  );
}

function PackageRail({
  packages,
  selectedPackageId,
  selectedProfileId,
  onSelectPackage,
  onSelectProfile,
}: {
  packages: PackageSummary[];
  selectedPackageId: string;
  selectedProfileId: string;
  onSelectPackage: (id: string) => void;
  onSelectProfile: (id: string) => void;
}) {
  const selectedPackage = packages.find((item) => item.id === selectedPackageId) ?? packages[0];
  return (
    <aside className="package-rail">
      <div className="rail-toolbar">
        <div className="rail-search"><Search size={14} /><input aria-label="Filter packages" placeholder="Filter packages" /></div>
        <button className="icon-button subtle" type="button" aria-label="Package filters"><ListFilter size={15} /></button>
      </div>
      <div className="rail-section-label"><span>Packages</span><span>{packages.length}</span></div>
      <div className="package-list">
        {packages.map((item) => (
          <button
            key={item.id}
            className={item.id === selectedPackageId ? "package-row selected" : "package-row"}
            type="button"
            onClick={() => onSelectPackage(item.id)}
          >
            <span className="package-avatar" style={{ "--package-accent": item.accent } as React.CSSProperties}>{item.initials}</span>
            <span><strong>{item.name}</strong><small>{item.runtime} · v{item.version}</small></span>
            {item.trust === "verified" && <ShieldCheck className="verified-icon" size={14} />}
          </button>
        ))}
      </div>
      <div className="rail-divider" />
      <div className="rail-section-label"><span>Profiles</span><button type="button" aria-label="Create profile"><Plus size={14} /></button></div>
      <div className="profile-list">
        {selectedPackage.profiles.map((profile) => (
          <button
            key={profile.id}
            className={profile.id === selectedProfileId ? "profile-row selected" : "profile-row"}
            type="button"
            onClick={() => onSelectProfile(profile.id)}
          >
            <span className="profile-file"><span /></span>
            <span><strong>{profile.name}</strong><small>{profile.schedule ?? `Last run · ${profile.lastRun}`}</small></span>
            <Ellipsis size={14} />
          </button>
        ))}
      </div>
      <button className="new-profile-button" type="button"><Plus size={14} /> New task profile</button>
    </aside>
  );
}

function TrustBanner({ trust }: { trust: TrustLevel }) {
  if (trust === "verified") {
    return (
      <div className="trust-banner verified">
        <ShieldCheck size={18} />
        <div><strong>Verified package</strong><span>Checksum and publisher signature are valid. Requests network and output-folder access.</span></div>
        <button type="button">View capabilities <ExternalLink size={12} /></button>
      </div>
    );
  }
  return (
    <div className="trust-banner warning">
      <ShieldCheck size={18} />
      <div><strong>{trust === "local" ? "Local package" : "Trust required"}</strong><span>Review publisher and requested capabilities before running.</span></div>
      <button type="button">Review trust <ExternalLink size={12} /></button>
    </div>
  );
}

function FormSection({
  icon,
  title,
  description,
  children,
}: {
  icon: React.ReactNode;
  title: string;
  description: string;
  children: React.ReactNode;
}) {
  return (
    <section className="form-section">
      <header><span className="section-icon">{icon}</span><div><h3>{title}</h3><p>{description}</p></div></header>
      <div className="form-section-body">{children}</div>
    </section>
  );
}

function Field({
  label,
  required,
  hint,
  children,
}: {
  label: string;
  required?: boolean;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <label className="field">
      <span className="field-label">{label}{required && <em>Required</em>}</span>
      {children}
      {hint && <small>{hint}</small>}
    </label>
  );
}

function ActivityInspector({
  logs,
  followLogs,
  onToggleFollow,
}: {
  logs: LogEntry[];
  followLogs: boolean;
  onToggleFollow: () => void;
}) {
  return (
    <aside className="activity-inspector">
      <header className="inspector-header">
        <div><Activity size={16} /><strong>Live activity</strong><span className="live-pill"><span /> Live</span></div>
        <button className="icon-button subtle" type="button" aria-label="Activity options"><Ellipsis size={16} /></button>
      </header>
      <div className="run-summary">
        <div className="run-summary-title">
          <span className="running-indicator"><LoaderCircle className="spin" size={16} /></span>
          <div><strong>Monthly close</strong><span>run-1842 · Started 4m ago</span></div>
          <span>68%</span>
        </div>
        <div className="progress-track"><span style={{ width: "68%" }} /></div>
        <div className="run-stats">
          <span><Clock3 size={13} /> 04:18 elapsed</span>
          <span><FileOutput size={13} /> 2 artifacts</span>
          <span><Activity size={13} /> 2.1 MB/s</span>
        </div>
      </div>
      <div className="inspector-tabs">
        <button className="active" type="button">Logs <span>9</span></button>
        <button type="button">Artifacts <span>2</span></button>
        <button type="button">Details</button>
      </div>
      <div className="log-toolbar">
        <div className="rail-search"><Search size={13} /><input aria-label="Search logs" placeholder="Search logs" /></div>
        <button className="icon-button subtle" type="button" aria-label="Filter logs"><Filter size={14} /></button>
        <button
          className={followLogs ? "follow-button active" : "follow-button"}
          type="button"
          onClick={onToggleFollow}
          aria-pressed={followLogs}
        >
          {followLogs ? <Pause size={12} /> : <Play size={12} />} Follow
        </button>
      </div>
      <div className="log-view" role="log" aria-live="polite">
        {logs.map((entry) => <LogRow key={entry.id} entry={entry} />)}
        <div className="log-cursor"><span /> Waiting for events</div>
      </div>
      <footer className="inspector-footer">
        <button className="button danger-ghost" type="button"><Square size={13} fill="currentColor" /> Stop run</button>
        <span className="footer-spacer" />
        <button className="icon-button subtle" type="button" aria-label="Copy logs"><Copy size={14} /></button>
        <button className="button secondary small" type="button"><TerminalSquare size={14} /> Open full log</button>
      </footer>
    </aside>
  );
}

function LogRow({ entry }: { entry: LogEntry }) {
  return (
    <div className={`log-row level-${entry.level}`}>
      <span className="log-time">{entry.time}</span>
      <span className="log-level">{entry.level.slice(0, 3).toUpperCase()}</span>
      <span className="log-scope">{entry.scope}</span>
      <span className="log-message">{entry.message}</span>
    </div>
  );
}

function WorkbenchSkeleton() {
  return (
    <div className="page loading-page">
      <div className="skeleton skeleton-title" />
      <div className="skeleton-grid"><div /><div /><div /></div>
    </div>
  );
}
