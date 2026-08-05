import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  CheckCircle2,
  Copy,
  Download,
  Eye,
  EyeOff,
  KeyRound,
  LockKeyhole,
  Plus,
  RefreshCw,
  Save,
  Search,
  Server,
  ShieldCheck,
  Smartphone,
  Star,
  Trash2,
  Wifi,
  WifiOff,
} from "lucide-react";
import QRCode from "qrcode";
import { useCallback, useEffect, useMemo, useState } from "react";

import type {
  VaultCredential,
  VaultCredentialInput,
  VaultCredentialKind,
  VaultCredentialSummary,
  VaultSetup,
  VaultStatus,
} from "../domain/models";
import { desktopGateway } from "../infra/gateway";

const KIND_LABELS: Record<VaultCredentialKind, string> = {
  login: "登录账号",
  apiKey: "API Key",
  token: "访问令牌",
  database: "数据库",
  ssh: "SSH",
  secureNote: "安全笔记",
};

const KIND_OPTIONS = Object.entries(KIND_LABELS) as Array<[VaultCredentialKind, string]>;

function emptyCredential(): VaultCredentialInput {
  return {
    id: "",
    name: "",
    kind: "login",
    username: "",
    secret: "",
    uri: "",
    notes: "",
    tags: [],
    favorite: false,
  };
}

function formatExpiry(value?: number) {
  if (!value) return "未解锁";
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value * 1000));
}

function downloadTextFile(name: string, content: string) {
  const blob = new Blob([content], { type: "text/plain;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = name;
  anchor.click();
  URL.revokeObjectURL(url);
}

export function SecretsPage() {
  const [status, setStatus] = useState<VaultStatus | null>(null);
  const [setup, setSetup] = useState<VaultSetup | null>(null);
  const [qrCode, setQrCode] = useState("");
  const [verificationCode, setVerificationCode] = useState("");
  const [recoveryInput, setRecoveryInput] = useState("");
  const [unlockMethod, setUnlockMethod] = useState<"totp" | "recovery">("totp");
  const [recoveryCode, setRecoveryCode] = useState("");
  const [serviceToken, setServiceToken] = useState("");
  const [credentials, setCredentials] = useState<VaultCredentialSummary[]>([]);
  const [selectedId, setSelectedId] = useState("");
  const [draft, setDraft] = useState<VaultCredentialInput>(emptyCredential);
  const [search, setSearch] = useState("");
  const [kindFilter, setKindFilter] = useState<"all" | "favorite" | VaultCredentialKind>("all");
  const [tagsDraft, setTagsDraft] = useState("");
  const [revealSecret, setRevealSecret] = useState(false);
  const [servicePort, setServicePort] = useState(34131);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [error, setError] = useState("");
  const [deletePending, setDeletePending] = useState(false);

  const updateDraft = useCallback(<Key extends keyof VaultCredentialInput>(key: Key, value: VaultCredentialInput[Key]) => {
    setDraft((current) => ({ ...current, [key]: value }));
  }, []);

  const loadCredentials = useCallback(async (preferredId?: string) => {
    const items = await desktopGateway.listVaultCredentials();
    setCredentials(items);
    if (preferredId && items.some((item) => item.id === preferredId)) setSelectedId(preferredId);
    else if (selectedId && !items.some((item) => item.id === selectedId)) {
      setSelectedId("");
      setDraft(emptyCredential());
      setTagsDraft("");
    }
  }, [selectedId]);

  const loadStatus = useCallback(async () => {
    try {
      const next = await desktopGateway.getVaultStatus();
      setStatus(next);
      setServicePort(next.service.port || 34131);
      if (next.unlocked) await loadCredentials();
      else {
        setCredentials([]);
        setSelectedId("");
        setServiceToken("");
      }
    } catch (caught) {
      setError(String(caught));
    }
  }, [loadCredentials]);

  useEffect(() => {
    void loadStatus();
    const timer = window.setInterval(() => { void loadStatus(); }, 30_000);
    return () => window.clearInterval(timer);
  }, [loadStatus]);

  useEffect(() => {
    if (!setup) {
      setQrCode("");
      return;
    }
    let disposed = false;
    void QRCode.toDataURL(setup.otpAuthUri, {
      errorCorrectionLevel: "M",
      margin: 2,
      width: 240,
      color: { dark: "#17202b", light: "#ffffff" },
    }).then((result) => { if (!disposed) setQrCode(result); })
      .catch((caught: unknown) => { if (!disposed) setError(`二维码生成失败：${String(caught)}`); });
    return () => { disposed = true; };
  }, [setup]);

  useEffect(() => {
    if (!selectedId) return;
    let disposed = false;
    setError("");
    void desktopGateway.getVaultCredential(selectedId)
      .then((item) => {
        if (disposed) return;
        setDraft({
          id: item.id,
          name: item.name,
          kind: item.kind,
          username: item.username,
          secret: item.secret,
          uri: item.uri,
          notes: item.notes,
          tags: item.tags,
          favorite: item.favorite,
        });
        setTagsDraft(item.tags.join(", "));
        setRevealSecret(false);
      })
      .catch((caught: unknown) => { if (!disposed) setError(String(caught)); });
    return () => { disposed = true; };
  }, [selectedId]);

  const filteredCredentials = useMemo(() => {
    const query = search.trim().toLocaleLowerCase();
    return credentials.filter((item) => {
      if (kindFilter === "favorite" && !item.favorite) return false;
      if (kindFilter !== "all" && kindFilter !== "favorite" && item.kind !== kindFilter) return false;
      if (!query) return true;
      return [item.name, item.username, item.uri, ...item.tags].some((value) => value.toLocaleLowerCase().includes(query));
    });
  }, [credentials, kindFilter, search]);

  const copyText = async (value: string, label: string) => {
    await navigator.clipboard.writeText(value);
    setNotice(`${label}已复制，关闭 DRPA 后运行时授权自动失效`);
  };

  const beginSetup = async () => {
    setBusy(true);
    setError("");
    try {
      setSetup(await desktopGateway.beginVaultSetup());
      setVerificationCode("");
    } catch (caught) {
      setError(String(caught));
    } finally {
      setBusy(false);
    }
  };

  const completeSetup = async () => {
    if (!setup || !/^\d{6}$/.test(verificationCode)) return;
    setBusy(true);
    setError("");
    try {
      const result = await desktopGateway.completeVaultSetup(setup.setupId, verificationCode);
      setStatus(result.status);
      setServiceToken(result.serviceToken);
      setRecoveryCode(result.recoveryCode ?? "");
      setSetup(null);
      setVerificationCode("");
      await loadCredentials();
    } catch (caught) {
      setError(String(caught));
    } finally {
      setBusy(false);
    }
  };

  const unlock = async () => {
    setBusy(true);
    setError("");
    try {
      const result = unlockMethod === "totp"
        ? await desktopGateway.unlockVault(verificationCode)
        : await desktopGateway.unlockVaultWithRecovery(recoveryInput);
      setStatus(result.status);
      setServiceToken(result.serviceToken);
      setVerificationCode("");
      setRecoveryInput("");
      if (result.recoveryCode) setRecoveryCode(result.recoveryCode);
      await loadCredentials();
    } catch (caught) {
      setError(String(caught));
      await loadStatus();
    } finally {
      setBusy(false);
    }
  };

  const lock = async () => {
    await desktopGateway.lockVault();
    setServiceToken("");
    setCredentials([]);
    setSelectedId("");
    setDraft(emptyCredential());
    await loadStatus();
  };

  const saveRecoveryCode = async () => {
    const fileName = `DRPA-vault-recovery-${new Date().toISOString().slice(0, 10)}.txt`;
    const content = `DRPA 凭据保险箱恢复码\r\n\r\n${recoveryCode}\r\n\r\n请离线保存。恢复解锁后此恢复码会立即轮换。\r\n`;
    try {
      if (!("__TAURI_INTERNALS__" in window)) {
        downloadTextFile(fileName, content);
        setNotice("恢复码文件已下载");
        return;
      }
      const target = await saveDialog({
        defaultPath: fileName,
        filters: [{ name: "文本文件", extensions: ["txt"] }],
      });
      if (!target) return;
      const savedPath = await desktopGateway.exportVaultRecoveryCode(recoveryCode, target);
      setNotice(`恢复码已保存到 ${savedPath}`);
    } catch (caught) {
      setError(`恢复码保存失败：${String(caught)}`);
    }
  };

  const createCredential = () => {
    setSelectedId("");
    setDraft(emptyCredential());
    setTagsDraft("");
    setRevealSecret(true);
    setDeletePending(false);
    setNotice("正在新建凭据");
  };

  const saveCredential = async () => {
    if (!draft.name.trim()) {
      setError("请输入凭据名称");
      return;
    }
    setBusy(true);
    setError("");
    try {
      const saved = await desktopGateway.saveVaultCredential({
        ...draft,
        name: draft.name.trim(),
        tags: tagsDraft.split(",").map((tag) => tag.trim()).filter(Boolean),
      });
      setSelectedId(saved.id);
      setDraft({ ...saved });
      setTagsDraft(saved.tags.join(", "));
      await loadCredentials(saved.id);
      setNotice("凭据已加密保存");
    } catch (caught) {
      setError(String(caught));
    } finally {
      setBusy(false);
    }
  };

  const deleteCredential = async () => {
    if (!draft.id) return;
    setBusy(true);
    try {
      await desktopGateway.deleteVaultCredential(draft.id);
      setDeletePending(false);
      setSelectedId("");
      setDraft(emptyCredential());
      setTagsDraft("");
      await loadCredentials();
      setNotice("凭据已删除");
    } catch (caught) {
      setError(String(caught));
    } finally {
      setBusy(false);
    }
  };

  const toggleService = async () => {
    setBusy(true);
    setError("");
    try {
      const service = status?.service.running
        ? await desktopGateway.stopVaultService()
        : await desktopGateway.startVaultService(servicePort);
      setStatus((current) => current ? { ...current, service } : current);
      setNotice(service.running ? `凭据服务已监听 ${service.endpoint}` : "凭据服务已停止");
    } catch (caught) {
      setError(String(caught));
    } finally {
      setBusy(false);
    }
  };

  if (!status) {
    return <div className="page vault-page"><div className="vault-loading"><RefreshCw className="spin" /><strong>正在读取凭据保险箱…</strong></div></div>;
  }

  if (!status.initialized) {
    return (
      <div className="page vault-page vault-onboarding-page">
        <header className="vault-compact-header"><div><span className="vault-brand-icon"><ShieldCheck /></span><div><h1>开发凭据保险箱</h1><p>本地加密保存账号、API Key、数据库密码与令牌</p></div></div><span className="status-badge neutral">本机存储</span></header>
        <main className="vault-onboarding">
          <section className="vault-onboarding-copy">
            <span className="vault-kicker">DRPA VAULT</span>
            <h2>让凭据留在设备上，<br />只在需要时短暂解锁。</h2>
            <p>保险箱使用随机主密钥加密全部字段。Google Authenticator 验证通过后，派生密钥仅在当前 DRPA 进程中保留，最长 24 小时。</p>
            <div className="vault-security-points">
              <span><ShieldCheck size={17} /><strong>AES-256-GCM</strong><small>完整内容加密与篡改检测</small></span>
              <span><Smartphone size={17} /><strong>6 位动态验证码</strong><small>扫码初始化，离线可验证</small></span>
              <span><LockKeyhole size={17} /><strong>运行时自动失效</strong><small>退出软件、手动锁定或 24 小时到期</small></span>
            </div>
          </section>
          <section className="vault-setup-card">
            {!setup ? (
              <>
                <div className="vault-setup-icon"><KeyRound /></div>
                <h2>初始化保险箱</h2>
                <p>准备好 Google Authenticator。下一步会显示二维码和手动设置密钥。</p>
                <button className="button primary wide" type="button" disabled={busy} onClick={() => void beginSetup()}><Smartphone size={17} />生成初始化二维码</button>
              </>
            ) : (
              <>
                <div className="vault-setup-title"><div><span>步骤 1 / 2</span><h2>扫描二维码</h2></div><Smartphone /></div>
                <p>在 Google Authenticator 中添加账号，然后输入应用生成的 6 位验证码。</p>
                <div className="vault-qr-wrap">{qrCode ? <img src={qrCode} alt="Google Authenticator 初始化二维码" /> : <RefreshCw className="spin" />}</div>
                <label className="vault-manual-key"><span>无法扫码？手动输入密钥</span><div><code>{setup.manualKey}</code><button type="button" title="复制设置密钥" onClick={() => void copyText(setup.manualKey, "设置密钥")}><Copy size={15} /></button></div></label>
                <label className="vault-code-field"><span>6 位验证码</span><input autoFocus inputMode="numeric" autoComplete="one-time-code" maxLength={6} placeholder="000000" value={verificationCode} onChange={(event) => setVerificationCode(event.currentTarget.value.replace(/\D/g, "").slice(0, 6))} onKeyDown={(event) => { if (event.key === "Enter") void completeSetup(); }} /></label>
                <button className="button primary wide" type="button" disabled={busy || verificationCode.length !== 6} onClick={() => void completeSetup()}><ShieldCheck size={17} />验证并创建保险箱</button>
              </>
            )}
            {error && <div className="vault-error"><AlertTriangle size={16} />{error}</div>}
          </section>
        </main>
      </div>
    );
  }

  if (!status.unlocked) {
    return (
      <div className="page vault-page vault-lock-page">
        <header className="vault-compact-header"><div><span className="vault-brand-icon"><ShieldCheck /></span><div><h1>开发凭据保险箱</h1><p>保险箱已锁定</p></div></div><span className="status-badge neutral"><LockKeyhole size={12} /> 已锁定</span></header>
        <main className="vault-unlock-card">
          <div className="vault-lock-visual"><LockKeyhole /></div>
          <h2>验证身份以继续</h2>
          <p>解锁后密钥只保留在当前 DRPA 运行时，最长 24 小时。</p>
          <div className="vault-method-tabs" role="tablist"><button type="button" className={unlockMethod === "totp" ? "active" : ""} onClick={() => setUnlockMethod("totp")}><Smartphone size={15} />动态验证码</button><button type="button" className={unlockMethod === "recovery" ? "active" : ""} onClick={() => setUnlockMethod("recovery")}><KeyRound size={15} />恢复码</button></div>
          {unlockMethod === "totp" ? <label className="vault-code-field"><span>Google Authenticator 验证码</span><input autoFocus inputMode="numeric" autoComplete="one-time-code" maxLength={6} placeholder="000000" value={verificationCode} onChange={(event) => setVerificationCode(event.currentTarget.value.replace(/\D/g, "").slice(0, 6))} onKeyDown={(event) => { if (event.key === "Enter") void unlock(); }} /></label> : <label className="vault-recovery-input"><span>恢复码</span><input autoFocus value={recoveryInput} placeholder="XXXX-XXXX-XXXX-XXXX-XXXX" onChange={(event) => setRecoveryInput(event.currentTarget.value.toUpperCase())} onKeyDown={(event) => { if (event.key === "Enter") void unlock(); }} /><small>使用后会立即生成新的恢复码。</small></label>}
          {status.retryAfterSeconds > 0 && <div className="vault-retry">连续验证失败，请在 {status.retryAfterSeconds} 秒后重试</div>}
          {error && <div className="vault-error"><AlertTriangle size={16} />{error}</div>}
          <button className="button primary wide" type="button" disabled={busy || status.retryAfterSeconds > 0 || (unlockMethod === "totp" ? verificationCode.length !== 6 : !recoveryInput.trim())} onClick={() => void unlock()}><ShieldCheck size={17} />立即解锁</button>
        </main>
      </div>
    );
  }

  return (
    <div className="page vault-page vault-workspace-page">
      <header className="vault-workspace-header">
        <div><span className="vault-brand-icon"><ShieldCheck /></span><div><h1>开发凭据保险箱</h1><p>{credentials.length} 项凭据 · 授权到 {formatExpiry(status.unlockedUntil)}</p></div></div>
        <div><span className="status-badge success"><CheckCircle2 size={12} /> 已解锁</span><button className="button secondary small" type="button" onClick={() => void lock()}><LockKeyhole size={14} />锁定</button></div>
      </header>
      <div className="vault-workspace">
        <aside className="vault-nav">
          <button className="button primary wide" type="button" onClick={createCredential}><Plus size={16} />新增凭据</button>
          <nav aria-label="凭据分类">
            <button type="button" className={kindFilter === "all" ? "active" : ""} onClick={() => setKindFilter("all")}><KeyRound size={15} /><span>全部项目</span><em>{credentials.length}</em></button>
            <button type="button" className={kindFilter === "favorite" ? "active" : ""} onClick={() => setKindFilter("favorite")}><Star size={15} /><span>收藏</span><em>{credentials.filter((item) => item.favorite).length}</em></button>
            <div className="vault-nav-label">类型</div>
            {KIND_OPTIONS.map(([kind, label]) => <button key={kind} type="button" className={kindFilter === kind ? "active" : ""} onClick={() => setKindFilter(kind)}><span className={`vault-kind-dot ${kind}`} /><span>{label}</span><em>{credentials.filter((item) => item.kind === kind).length}</em></button>)}
          </nav>
          <section className="vault-service-card">
            <header>{status.service.running ? <Wifi size={16} /> : <WifiOff size={16} />}<div><strong>本地凭据服务</strong><span>{status.service.running ? `127.0.0.1:${status.service.port}` : "已停止"}</span></div></header>
            <label><span>端口</span><input type="number" min={1024} max={65535} disabled={status.service.running} value={servicePort} onChange={(event) => setServicePort(event.currentTarget.valueAsNumber || 34131)} /></label>
            <button className={`button small wide ${status.service.running ? "danger" : "secondary"}`} type="button" disabled={busy} onClick={() => void toggleService()}><Server size={14} />{status.service.running ? "停止服务" : "启动服务"}</button>
          </section>
        </aside>
        <section className="vault-list-pane">
          <div className="vault-search"><Search size={16} /><input aria-label="搜索凭据" value={search} placeholder="搜索名称、账号、地址或标签" onChange={(event) => setSearch(event.currentTarget.value)} /></div>
          <div className="vault-list-meta"><strong>{kindFilter === "all" ? "全部项目" : kindFilter === "favorite" ? "收藏" : KIND_LABELS[kindFilter]}</strong><span>{filteredCredentials.length} 项</span></div>
          <div className="vault-list">
            {filteredCredentials.map((item) => <button key={item.id} type="button" className={selectedId === item.id ? "active" : ""} onClick={() => setSelectedId(item.id)}><span className={`vault-item-icon ${item.kind}`}><KeyRound size={16} /></span><span><strong>{item.name}</strong><small>{item.username || item.uri || KIND_LABELS[item.kind]}</small></span>{item.favorite && <Star className="vault-favorite" size={13} fill="currentColor" />}</button>)}
            {filteredCredentials.length === 0 && <div className="vault-empty-list"><KeyRound size={24} /><strong>没有匹配的凭据</strong><span>新增一项，或调整筛选条件。</span></div>}
          </div>
        </section>
        <main className="vault-detail-pane">
          <header><div><span>{draft.id ? "编辑凭据" : "新建凭据"}</span><h2>{draft.name || "未命名凭据"}</h2></div><button className={`vault-star-button ${draft.favorite ? "active" : ""}`} type="button" title="收藏" onClick={() => updateDraft("favorite", !draft.favorite)}><Star size={18} fill={draft.favorite ? "currentColor" : "none"} /></button></header>
          <div className="vault-detail-form">
            <label><span>名称</span><input autoFocus={!draft.id} value={draft.name} placeholder="例如：OpenAI 开发密钥" onChange={(event) => updateDraft("name", event.currentTarget.value)} /></label>
            <label><span>类型</span><select value={draft.kind} onChange={(event) => updateDraft("kind", event.currentTarget.value as VaultCredentialKind)}>{KIND_OPTIONS.map(([kind, label]) => <option key={kind} value={kind}>{label}</option>)}</select></label>
            <label><span>账号 / 标识</span><input value={draft.username} placeholder="用户名、邮箱或 Key ID" onChange={(event) => updateDraft("username", event.currentTarget.value)} /></label>
            <label><span>密码 / Secret</span><div className="vault-secret-input"><input type={revealSecret ? "text" : "password"} value={draft.secret} placeholder="输入敏感值" onChange={(event) => updateDraft("secret", event.currentTarget.value)} /><button type="button" title={revealSecret ? "隐藏" : "显示"} onClick={() => setRevealSecret((value) => !value)}>{revealSecret ? <EyeOff size={16} /> : <Eye size={16} />}</button><button type="button" title="复制" disabled={!draft.secret} onClick={() => void copyText(draft.secret, "Secret")}><Copy size={16} /></button></div></label>
            <label><span>网站 / 服务地址</span><input value={draft.uri} placeholder="https://api.example.com/v1" onChange={(event) => updateDraft("uri", event.currentTarget.value)} /></label>
            <label><span>标签</span><input value={tagsDraft} placeholder="开发, 本地, 数据库（逗号分隔）" onChange={(event) => setTagsDraft(event.currentTarget.value)} /></label>
            <label><span>安全备注</span><textarea rows={6} value={draft.notes} placeholder="仅存储在加密保险箱中的说明" onChange={(event) => updateDraft("notes", event.currentTarget.value)} /></label>
          </div>
          <footer><button className="button danger-ghost" type="button" disabled={!draft.id} onClick={() => setDeletePending(true)}><Trash2 size={15} />删除</button><span>{notice || "保存后全部字段会重新加密写入本地文件"}</span><button className="button primary" type="button" disabled={busy || !draft.name.trim()} onClick={() => void saveCredential()}><Save size={16} />保存凭据</button></footer>
        </main>
      </div>
      {error && <button className="vault-toast error" type="button" onClick={() => setError("")}><AlertTriangle size={16} />{error}</button>}
      {notice && <button className="vault-toast" type="button" onClick={() => setNotice("")}><CheckCircle2 size={16} />{notice}</button>}
      {recoveryCode && <div className="vault-dialog-backdrop"><section className="vault-recovery-dialog" role="dialog" aria-modal="true" aria-labelledby="vault-recovery-title"><span className="vault-recovery-icon"><KeyRound /></span><h2 id="vault-recovery-title">保存恢复码</h2><p>这是紧急解锁保险箱的唯一恢复码。它只展示在当前步骤；使用一次后会立即轮换。</p><div className="vault-recovery-code"><code>{recoveryCode}</code><button type="button" title="复制恢复码" onClick={() => void copyText(recoveryCode, "恢复码")}><Copy size={16} /></button></div><div className="vault-recovery-warning"><AlertTriangle size={16} />请保存在离线位置，不要和保险箱数据文件放在一起。</div><footer><button className="button secondary" type="button" onClick={() => void saveRecoveryCode()}><Download size={16} />下载恢复码文件</button><button className="button primary" type="button" onClick={() => setRecoveryCode("")}><CheckCircle2 size={16} />我已妥善保存</button></footer></section></div>}
      {deletePending && <div className="vault-dialog-backdrop"><section className="vault-delete-dialog" role="dialog" aria-modal="true"><span><Trash2 /></span><div><h2>删除“{draft.name}”？</h2><p>该凭据会从本地加密保险箱中永久移除。</p></div><footer><button className="button secondary" type="button" onClick={() => setDeletePending(false)}>取消</button><button className="button danger" type="button" disabled={busy} onClick={() => void deleteCredential()}>确认删除</button></footer></section></div>}
      {status.service.running && serviceToken && <div className="vault-service-token" title="当前运行时服务令牌"><Server size={14} /><span>本地服务已授权</span><button type="button" onClick={() => void copyText(serviceToken, "运行时服务令牌")}><Copy size={13} />复制 Token</button></div>}
    </div>
  );
}
