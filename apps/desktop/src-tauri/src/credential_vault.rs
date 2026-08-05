use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD as BASE64, URL_SAFE_NO_PAD};
use data_encoding::BASE32_NOPAD;
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand::RngCore;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha1::Sha1;
use sha2::Sha256;
use tauri::State;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::AppPaths;

const VAULT_SCHEMA: u32 = 1;
const SESSION_SECONDS: i64 = 24 * 60 * 60;
const DEFAULT_SERVICE_PORT: u16 = 34_131;
const MAX_ITEMS: usize = 2_000;
const MAX_HTTP_BODY: usize = 256 * 1024;
const TOTP_PERIOD: i64 = 30;
const TOTP_DIGITS: u32 = 6;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VaultStatus {
    initialized: bool,
    unlocked: bool,
    unlocked_until: Option<i64>,
    item_count: usize,
    failed_attempts: u32,
    retry_after_seconds: u64,
    service: VaultServiceStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VaultSetup {
    setup_id: String,
    account: String,
    issuer: String,
    manual_key: String,
    otp_auth_uri: String,
    expires_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VaultUnlockResult {
    status: VaultStatus,
    service_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    recovery_code: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VaultCredential {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub username: String,
    pub secret: String,
    pub uri: String,
    pub notes: String,
    pub tags: Vec<String>,
    pub favorite: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VaultCredentialInput {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default = "default_credential_kind")]
    pub kind: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub secret: String,
    #[serde(default)]
    pub uri: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub favorite: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VaultCredentialSummary {
    id: String,
    name: String,
    kind: String,
    username: String,
    uri: String,
    tags: Vec<String>,
    favorite: bool,
    has_secret: bool,
    updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VaultServiceStatus {
    running: bool,
    port: u16,
    endpoint: String,
    started_at: Option<i64>,
    last_error: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct CipherBlob {
    nonce: String,
    ciphertext: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct VaultEnvelope {
    schema: u32,
    vault_id: String,
    protected_totp_secret: String,
    protected_device_secret: String,
    device_key_wrap: CipherBlob,
    recovery_salt: String,
    recovery_key_wrap: CipherBlob,
    payload_revision: u64,
    payload: CipherBlob,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct VaultPayload {
    revision: u64,
    items: Vec<VaultCredential>,
}

struct PendingSetup {
    id: String,
    secret: Zeroizing<Vec<u8>>,
    expires_at: i64,
}

struct VaultSession {
    key: Zeroizing<[u8; 32]>,
    token: Zeroizing<String>,
    expires_at: i64,
}

#[derive(Default)]
struct AttemptState {
    failures: u32,
    locked_until: Option<Instant>,
}

struct ServiceControl {
    stop: Option<Arc<AtomicBool>>,
    running: bool,
    port: u16,
    started_at: Option<i64>,
    last_error: String,
}

impl Default for ServiceControl {
    fn default() -> Self {
        Self {
            stop: None,
            running: false,
            port: DEFAULT_SERVICE_PORT,
            started_at: None,
            last_error: String::new(),
        }
    }
}

struct VaultRuntime {
    session: Mutex<Option<VaultSession>>,
    setup: Mutex<Option<PendingSetup>>,
    attempts: Mutex<AttemptState>,
    service: Mutex<ServiceControl>,
}

impl Default for VaultRuntime {
    fn default() -> Self {
        Self {
            session: Mutex::new(None),
            setup: Mutex::new(None),
            attempts: Mutex::new(AttemptState::default()),
            service: Mutex::new(ServiceControl::default()),
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct CredentialVaultManager {
    runtime: Arc<VaultRuntime>,
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn default_credential_kind() -> String {
    "login".to_owned()
}

fn now_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}

fn vault_root(paths: &AppPaths) -> PathBuf {
    paths.data_root.join("credential-vault")
}

fn envelope_path(paths: &AppPaths) -> PathBuf {
    vault_root(paths).join("vault.json")
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

fn random_token() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes::<32>())
}

fn aad(label: &str, vault_id: &str, revision: Option<u64>) -> Vec<u8> {
    match revision {
        Some(revision) => format!("drpa-vault:{label}:{vault_id}:{revision}").into_bytes(),
        None => format!("drpa-vault:{label}:{vault_id}").into_bytes(),
    }
}

fn encrypt_blob(
    key: &[u8; 32],
    plaintext: &[u8],
    associated_data: &[u8],
) -> Result<CipherBlob, String> {
    let nonce = random_bytes::<12>();
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "保险箱密钥长度无效".to_owned())?;
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: associated_data,
            },
        )
        .map_err(|_| "加密凭据保险箱失败".to_owned())?;
    Ok(CipherBlob {
        nonce: BASE64.encode(nonce),
        ciphertext: BASE64.encode(ciphertext),
    })
}

fn decrypt_blob(
    key: &[u8; 32],
    blob: &CipherBlob,
    associated_data: &[u8],
) -> Result<Zeroizing<Vec<u8>>, String> {
    let nonce = BASE64
        .decode(&blob.nonce)
        .map_err(|_| "保险箱 nonce 编码无效".to_owned())?;
    let ciphertext = BASE64
        .decode(&blob.ciphertext)
        .map_err(|_| "保险箱密文编码无效".to_owned())?;
    if nonce.len() != 12 {
        return Err("保险箱 nonce 长度无效".to_owned());
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "保险箱密钥长度无效".to_owned())?;
    cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &ciphertext,
                aad: associated_data,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| "保险箱解密或完整性校验失败".to_owned())
}

fn derive_device_key(
    device_secret: &[u8],
    totp_secret: &[u8],
    vault_id: &str,
) -> Result<[u8; 32], String> {
    let mut input = Zeroizing::new(Vec::with_capacity(device_secret.len() + totp_secret.len()));
    input.extend_from_slice(device_secret);
    input.extend_from_slice(totp_secret);
    let hkdf = Hkdf::<Sha256>::new(Some(vault_id.as_bytes()), &input);
    let mut output = [0u8; 32];
    hkdf.expand(b"drpa-vault-device-key-wrap-v1", &mut output)
        .map_err(|_| "派生设备密钥失败".to_owned())?;
    Ok(output)
}

fn derive_recovery_key(recovery: &[u8], salt: &[u8]) -> Result<[u8; 32], String> {
    let hkdf = Hkdf::<Sha256>::new(Some(salt), recovery);
    let mut output = [0u8; 32];
    hkdf.expand(b"drpa-vault-recovery-key-wrap-v1", &mut output)
        .map_err(|_| "派生恢复密钥失败".to_owned())?;
    Ok(output)
}

#[cfg(windows)]
fn platform_protect(_paths: &AppPaths, data: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        CryptProtectData(
            &input,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err(format!(
            "Windows DPAPI 加密失败：{}",
            std::io::Error::last_os_error()
        ));
    }
    let protected =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe { LocalFree(output.pbData as *mut std::ffi::c_void) };
    Ok(protected)
}

#[cfg(windows)]
fn platform_unprotect(_paths: &AppPaths, data: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err(format!(
            "Windows DPAPI 解密失败：{}",
            std::io::Error::last_os_error()
        ));
    }
    let plaintext = Zeroizing::new(unsafe {
        std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec()
    });
    unsafe { LocalFree(output.pbData as *mut std::ffi::c_void) };
    Ok(plaintext)
}

#[cfg(not(windows))]
fn platform_key(paths: &AppPaths) -> Result<Zeroizing<[u8; 32]>, String> {
    let path = vault_root(paths).join(".device-key");
    if let Ok(bytes) = fs::read(&path) {
        return bytes
            .try_into()
            .map(Zeroizing::new)
            .map_err(|_| "本地设备密钥长度无效".to_owned());
    }
    fs::create_dir_all(vault_root(paths))
        .map_err(|error| format!("创建保险箱目录失败：{error}"))?;
    let key = random_bytes::<32>();
    fs::write(&path, key).map_err(|error| format!("写入本地设备密钥失败：{error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("设置设备密钥权限失败：{error}"))?;
    }
    Ok(Zeroizing::new(key))
}

#[cfg(not(windows))]
fn platform_protect(paths: &AppPaths, data: &[u8]) -> Result<Vec<u8>, String> {
    let key = platform_key(paths)?;
    serde_json::to_vec(&encrypt_blob(&key, data, b"drpa-platform-protect-v1")?)
        .map_err(|error| error.to_string())
}

#[cfg(not(windows))]
fn platform_unprotect(paths: &AppPaths, data: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
    let key = platform_key(paths)?;
    let blob: CipherBlob =
        serde_json::from_slice(data).map_err(|error| format!("设备密钥封装无效：{error}"))?;
    decrypt_blob(&key, &blob, b"drpa-platform-protect-v1")
}

fn totp_value(secret: &[u8], timestamp: i64) -> Result<String, String> {
    let counter = timestamp.div_euclid(TOTP_PERIOD) as u64;
    let mut mac =
        <Hmac<Sha1> as Mac>::new_from_slice(secret).map_err(|_| "TOTP 密钥无效".to_owned())?;
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = (digest[digest.len() - 1] & 0x0f) as usize;
    let binary = ((digest[offset] as u32 & 0x7f) << 24)
        | ((digest[offset + 1] as u32) << 16)
        | ((digest[offset + 2] as u32) << 8)
        | digest[offset + 3] as u32;
    Ok(format!(
        "{:0width$}",
        binary % 10u32.pow(TOTP_DIGITS),
        width = TOTP_DIGITS as usize
    ))
}

fn verify_totp(secret: &[u8], code: &str, timestamp: i64) -> Result<bool, String> {
    let normalized = code.trim();
    if normalized.len() != TOTP_DIGITS as usize
        || !normalized.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Ok(false);
    }
    for offset in -1..=1 {
        if totp_value(secret, timestamp + offset * TOTP_PERIOD)? == normalized {
            return Ok(true);
        }
    }
    Ok(false)
}

fn format_recovery_code(bytes: &[u8]) -> String {
    let encoded = BASE32_NOPAD.encode(bytes);
    encoded
        .as_bytes()
        .chunks(4)
        .map(|chunk| String::from_utf8_lossy(chunk))
        .collect::<Vec<_>>()
        .join("-")
}

fn decode_recovery_code(value: &str) -> Result<Zeroizing<Vec<u8>>, String> {
    let normalized = value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase();
    let decoded = BASE32_NOPAD
        .decode(normalized.as_bytes())
        .map_err(|_| "恢复码格式无效".to_owned())?;
    if decoded.len() < 20 {
        return Err("恢复码长度无效".to_owned());
    }
    Ok(Zeroizing::new(decoded))
}

fn percent_encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn read_envelope(paths: &AppPaths) -> Result<VaultEnvelope, String> {
    let source =
        fs::read(envelope_path(paths)).map_err(|error| format!("读取凭据保险箱失败：{error}"))?;
    let envelope: VaultEnvelope =
        serde_json::from_slice(&source).map_err(|error| format!("凭据保险箱格式无效：{error}"))?;
    if envelope.schema != VAULT_SCHEMA {
        return Err(format!("凭据保险箱版本不受支持：{}", envelope.schema));
    }
    Ok(envelope)
}

fn write_envelope(paths: &AppPaths, envelope: &VaultEnvelope) -> Result<(), String> {
    let path = envelope_path(paths);
    let parent = path
        .parent()
        .ok_or_else(|| "凭据保险箱目录无效".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| format!("创建凭据保险箱目录失败：{error}"))?;
    let temporary = parent.join(format!(".vault-{}.tmp", Uuid::new_v4().simple()));
    let backup = parent.join(format!(".vault-{}.bak", Uuid::new_v4().simple()));
    let mut bytes = serde_json::to_vec_pretty(envelope)
        .map_err(|error| format!("序列化凭据保险箱失败：{error}"))?;
    bytes.push(b'\n');
    fs::write(&temporary, bytes).map_err(|error| format!("写入凭据保险箱临时文件失败：{error}"))?;
    let had_existing = path.exists();
    if had_existing {
        fs::rename(&path, &backup).map_err(|error| {
            let _ = fs::remove_file(&temporary);
            format!("备份凭据保险箱失败：{error}")
        })?;
    }
    if let Err(error) = fs::rename(&temporary, &path) {
        if had_existing {
            let _ = fs::rename(&backup, &path);
        }
        let _ = fs::remove_file(&temporary);
        return Err(format!("提交凭据保险箱失败：{error}"));
    }
    if had_existing {
        let _ = fs::remove_file(backup);
    }
    Ok(())
}

fn decrypt_payload(envelope: &VaultEnvelope, key: &[u8; 32]) -> Result<VaultPayload, String> {
    let plaintext = decrypt_blob(
        key,
        &envelope.payload,
        &aad(
            "payload",
            &envelope.vault_id,
            Some(envelope.payload_revision),
        ),
    )?;
    let payload: VaultPayload = serde_json::from_slice(&plaintext)
        .map_err(|error| format!("保险箱数据格式无效：{error}"))?;
    if payload.revision != envelope.payload_revision {
        return Err("保险箱数据修订号不一致".to_owned());
    }
    Ok(payload)
}

fn save_payload(
    paths: &AppPaths,
    key: &[u8; 32],
    mut envelope: VaultEnvelope,
    mut payload: VaultPayload,
) -> Result<VaultEnvelope, String> {
    payload.revision = envelope.payload_revision.saturating_add(1);
    let plaintext = Zeroizing::new(
        serde_json::to_vec(&payload).map_err(|error| format!("序列化保险箱数据失败：{error}"))?,
    );
    envelope.payload_revision = payload.revision;
    envelope.payload = encrypt_blob(
        key,
        &plaintext,
        &aad("payload", &envelope.vault_id, Some(payload.revision)),
    )?;
    envelope.updated_at = now_timestamp();
    write_envelope(paths, &envelope)?;
    Ok(envelope)
}

impl CredentialVaultManager {
    fn service_status(&self) -> Result<VaultServiceStatus, String> {
        let service = self
            .runtime
            .service
            .lock()
            .map_err(|_| "保险箱服务状态已损坏".to_owned())?;
        Ok(VaultServiceStatus {
            running: service.running,
            port: service.port,
            endpoint: format!("http://127.0.0.1:{}/v1/vault", service.port),
            started_at: service.started_at,
            last_error: service.last_error.clone(),
        })
    }

    pub(crate) fn status(&self, paths: &AppPaths) -> Result<VaultStatus, String> {
        let initialized = envelope_path(paths).is_file();
        let now = now_timestamp();
        let (unlocked, unlocked_until) = {
            let mut session = self
                .runtime
                .session
                .lock()
                .map_err(|_| "保险箱会话状态已损坏".to_owned())?;
            if session
                .as_ref()
                .is_some_and(|session| session.expires_at <= now)
            {
                *session = None;
            }
            (
                session.is_some(),
                session.as_ref().map(|session| session.expires_at),
            )
        };
        let item_count = if unlocked {
            self.with_key(|key| Ok(decrypt_payload(&read_envelope(paths)?, key)?.items.len()))?
        } else {
            0
        };
        let attempts = self
            .runtime
            .attempts
            .lock()
            .map_err(|_| "保险箱验证状态已损坏".to_owned())?;
        let retry_after_seconds = attempts
            .locked_until
            .and_then(|until| until.checked_duration_since(Instant::now()))
            .map_or(0, |duration| duration.as_secs().saturating_add(1));
        Ok(VaultStatus {
            initialized,
            unlocked,
            unlocked_until,
            item_count,
            failed_attempts: attempts.failures,
            retry_after_seconds,
            service: self.service_status()?,
        })
    }

    fn with_key<T>(
        &self,
        operation: impl FnOnce(&[u8; 32]) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut session = self
            .runtime
            .session
            .lock()
            .map_err(|_| "保险箱会话状态已损坏".to_owned())?;
        if session
            .as_ref()
            .is_some_and(|session| session.expires_at <= now_timestamp())
        {
            *session = None;
        }
        let session = session
            .as_ref()
            .ok_or_else(|| "凭据保险箱已锁定，请先完成 6 位验证码验证".to_owned())?;
        operation(&session.key)
    }

    fn runtime_token(&self) -> Result<String, String> {
        let mut session = self
            .runtime
            .session
            .lock()
            .map_err(|_| "保险箱会话状态已损坏".to_owned())?;
        if session
            .as_ref()
            .is_some_and(|session| session.expires_at <= now_timestamp())
        {
            *session = None;
        }
        session
            .as_ref()
            .map(|session| session.token.to_string())
            .ok_or_else(|| "凭据保险箱会话已锁定".to_owned())
    }

    fn install_session(&self, key: [u8; 32]) -> Result<String, String> {
        let token = random_token();
        *self
            .runtime
            .session
            .lock()
            .map_err(|_| "保险箱会话状态已损坏".to_owned())? = Some(VaultSession {
            key: Zeroizing::new(key),
            token: Zeroizing::new(token.clone()),
            expires_at: now_timestamp() + SESSION_SECONDS,
        });
        let mut attempts = self
            .runtime
            .attempts
            .lock()
            .map_err(|_| "保险箱验证状态已损坏".to_owned())?;
        attempts.failures = 0;
        attempts.locked_until = None;
        Ok(token)
    }

    fn before_verify(&self) -> Result<(), String> {
        let attempts = self
            .runtime
            .attempts
            .lock()
            .map_err(|_| "保险箱验证状态已损坏".to_owned())?;
        if let Some(remaining) = attempts
            .locked_until
            .and_then(|until| until.checked_duration_since(Instant::now()))
        {
            return Err(format!(
                "验证码尝试过多，请在 {} 秒后重试",
                remaining.as_secs().saturating_add(1)
            ));
        }
        Ok(())
    }

    fn record_failed_verify(&self) -> Result<(), String> {
        let mut attempts = self
            .runtime
            .attempts
            .lock()
            .map_err(|_| "保险箱验证状态已损坏".to_owned())?;
        attempts.failures = attempts.failures.saturating_add(1);
        if attempts.failures >= 5 {
            let exponent = (attempts.failures - 5).min(4);
            let seconds = 30u64.saturating_mul(1u64 << exponent).min(300);
            attempts.locked_until = Some(Instant::now() + Duration::from_secs(seconds));
        }
        Ok(())
    }

    pub(crate) fn begin_setup(&self, paths: &AppPaths) -> Result<VaultSetup, String> {
        if envelope_path(paths).exists() {
            return Err("凭据保险箱已经初始化".to_owned());
        }
        let id = format!("setup-{}", Uuid::new_v4().simple());
        let secret = random_bytes::<20>().to_vec();
        let expires_at = now_timestamp() + 10 * 60;
        let user = std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "developer".to_owned());
        let computer = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "local".to_owned());
        let account = format!("{user}@{computer}");
        let manual_key = BASE32_NOPAD.encode(&secret);
        let label = format!("DRPA:{account}");
        let otp_auth_uri = format!(
            "otpauth://totp/{}?secret={}&issuer=DRPA&algorithm=SHA1&digits={TOTP_DIGITS}&period={TOTP_PERIOD}",
            percent_encode(&label),
            manual_key
        );
        *self
            .runtime
            .setup
            .lock()
            .map_err(|_| "保险箱初始化状态已损坏".to_owned())? = Some(PendingSetup {
            id: id.clone(),
            secret: Zeroizing::new(secret),
            expires_at,
        });
        Ok(VaultSetup {
            setup_id: id,
            account,
            issuer: "DRPA".to_owned(),
            manual_key,
            otp_auth_uri,
            expires_at,
        })
    }

    pub(crate) fn complete_setup(
        &self,
        paths: &AppPaths,
        setup_id: &str,
        code: &str,
    ) -> Result<VaultUnlockResult, String> {
        self.before_verify()?;
        if envelope_path(paths).exists() {
            return Err("凭据保险箱已经初始化".to_owned());
        }
        let setup = self
            .runtime
            .setup
            .lock()
            .map_err(|_| "保险箱初始化状态已损坏".to_owned())?
            .take()
            .ok_or_else(|| "初始化二维码已过期，请重新生成".to_owned())?;
        if setup.id != setup_id || setup.expires_at <= now_timestamp() {
            return Err("初始化二维码已过期，请重新生成".to_owned());
        }
        if !verify_totp(&setup.secret, code, now_timestamp())? {
            self.record_failed_verify()?;
            *self
                .runtime
                .setup
                .lock()
                .map_err(|_| "保险箱初始化状态已损坏".to_owned())? = Some(setup);
            return Err("6 位验证码不正确".to_owned());
        }
        let vault_id = format!("vault-{}", Uuid::new_v4().simple());
        let device_secret = Zeroizing::new(random_bytes::<32>());
        let vault_key = random_bytes::<32>();
        let recovery_bytes = Zeroizing::new(random_bytes::<20>());
        let recovery_code = format_recovery_code(&recovery_bytes[..]);
        let recovery_salt = random_bytes::<16>();
        let device_key = Zeroizing::new(derive_device_key(
            &device_secret[..],
            &setup.secret,
            &vault_id,
        )?);
        let recovery_key =
            Zeroizing::new(derive_recovery_key(&recovery_bytes[..], &recovery_salt)?);
        let payload = VaultPayload::default();
        let payload_plaintext =
            Zeroizing::new(serde_json::to_vec(&payload).map_err(|error| error.to_string())?);
        let now = now_timestamp();
        let envelope = VaultEnvelope {
            schema: VAULT_SCHEMA,
            vault_id: vault_id.clone(),
            protected_totp_secret: BASE64.encode(platform_protect(paths, &setup.secret)?),
            protected_device_secret: BASE64.encode(platform_protect(paths, &device_secret[..])?),
            device_key_wrap: encrypt_blob(
                &device_key,
                &vault_key,
                &aad("device-key-wrap", &vault_id, None),
            )?,
            recovery_salt: BASE64.encode(recovery_salt),
            recovery_key_wrap: encrypt_blob(
                &recovery_key,
                &vault_key,
                &aad("recovery-key-wrap", &vault_id, None),
            )?,
            payload_revision: 0,
            payload: encrypt_blob(
                &vault_key,
                &payload_plaintext,
                &aad("payload", &vault_id, Some(0)),
            )?,
            created_at: now,
            updated_at: now,
        };
        write_envelope(paths, &envelope)?;
        let service_token = self.install_session(vault_key)?;
        Ok(VaultUnlockResult {
            status: self.status(paths)?,
            service_token,
            recovery_code: Some(recovery_code),
        })
    }

    pub(crate) fn unlock_totp(
        &self,
        paths: &AppPaths,
        code: &str,
    ) -> Result<VaultUnlockResult, String> {
        self.before_verify()?;
        let envelope = read_envelope(paths)?;
        let protected_totp = BASE64
            .decode(&envelope.protected_totp_secret)
            .map_err(|_| "TOTP 密钥封装无效".to_owned())?;
        let protected_device = BASE64
            .decode(&envelope.protected_device_secret)
            .map_err(|_| "设备密钥封装无效".to_owned())?;
        let totp_secret = platform_unprotect(paths, &protected_totp)?;
        if !verify_totp(&totp_secret, code, now_timestamp())? {
            self.record_failed_verify()?;
            return Err("6 位验证码不正确".to_owned());
        }
        let device_secret = platform_unprotect(paths, &protected_device)?;
        let device_key = Zeroizing::new(derive_device_key(
            &device_secret,
            &totp_secret,
            &envelope.vault_id,
        )?);
        let key_bytes = decrypt_blob(
            &device_key,
            &envelope.device_key_wrap,
            &aad("device-key-wrap", &envelope.vault_id, None),
        )?;
        let vault_key: [u8; 32] = key_bytes
            .as_slice()
            .try_into()
            .map_err(|_| "保险箱主密钥长度无效".to_owned())?;
        let _ = decrypt_payload(&envelope, &vault_key)?;
        let service_token = self.install_session(vault_key)?;
        Ok(VaultUnlockResult {
            status: self.status(paths)?,
            service_token,
            recovery_code: None,
        })
    }

    pub(crate) fn unlock_recovery(
        &self,
        paths: &AppPaths,
        code: &str,
    ) -> Result<VaultUnlockResult, String> {
        self.before_verify()?;
        let mut envelope = read_envelope(paths)?;
        let recovery = decode_recovery_code(code).inspect_err(|_| {
            let _ = self.record_failed_verify();
        })?;
        let salt = BASE64
            .decode(&envelope.recovery_salt)
            .map_err(|_| "恢复密钥 salt 无效".to_owned())?;
        let recovery_key = Zeroizing::new(derive_recovery_key(&recovery, &salt)?);
        let key_bytes = decrypt_blob(
            &recovery_key,
            &envelope.recovery_key_wrap,
            &aad("recovery-key-wrap", &envelope.vault_id, None),
        )
        .inspect_err(|_| {
            let _ = self.record_failed_verify();
        })?;
        let vault_key: [u8; 32] = key_bytes
            .as_slice()
            .try_into()
            .map_err(|_| "保险箱主密钥长度无效".to_owned())?;
        let _ = decrypt_payload(&envelope, &vault_key)?;
        let replacement = Zeroizing::new(random_bytes::<20>());
        let replacement_code = format_recovery_code(&replacement[..]);
        let replacement_salt = random_bytes::<16>();
        let replacement_key =
            Zeroizing::new(derive_recovery_key(&replacement[..], &replacement_salt)?);
        envelope.recovery_salt = BASE64.encode(replacement_salt);
        envelope.recovery_key_wrap = encrypt_blob(
            &replacement_key,
            &vault_key,
            &aad("recovery-key-wrap", &envelope.vault_id, None),
        )?;
        envelope.updated_at = now_timestamp();
        write_envelope(paths, &envelope)?;
        let service_token = self.install_session(vault_key)?;
        Ok(VaultUnlockResult {
            status: self.status(paths)?,
            service_token,
            recovery_code: Some(replacement_code),
        })
    }

    pub(crate) fn lock(&self) -> Result<(), String> {
        *self
            .runtime
            .session
            .lock()
            .map_err(|_| "保险箱会话状态已损坏".to_owned())? = None;
        Ok(())
    }

    pub(crate) fn list_credentials(
        &self,
        paths: &AppPaths,
    ) -> Result<Vec<VaultCredentialSummary>, String> {
        self.with_key(|key| {
            let mut summaries = decrypt_payload(&read_envelope(paths)?, key)?
                .items
                .into_iter()
                .map(|item| VaultCredentialSummary {
                    id: item.id,
                    name: item.name,
                    kind: item.kind,
                    username: item.username,
                    uri: item.uri,
                    tags: item.tags,
                    favorite: item.favorite,
                    has_secret: !item.secret.is_empty(),
                    updated_at: item.updated_at,
                })
                .collect::<Vec<_>>();
            summaries.sort_by(|left, right| {
                right
                    .favorite
                    .cmp(&left.favorite)
                    .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            });
            Ok(summaries)
        })
    }

    pub(crate) fn get_credential(
        &self,
        paths: &AppPaths,
        id: &str,
    ) -> Result<VaultCredential, String> {
        validate_id(id)?;
        self.with_key(|key| {
            decrypt_payload(&read_envelope(paths)?, key)?
                .items
                .into_iter()
                .find(|item| item.id == id)
                .ok_or_else(|| "凭据不存在".to_owned())
        })
    }

    pub(crate) fn save_credential(
        &self,
        paths: &AppPaths,
        input: VaultCredentialInput,
    ) -> Result<VaultCredential, String> {
        validate_input(&input)?;
        self.with_key(|key| {
            let envelope = read_envelope(paths)?;
            let mut payload = decrypt_payload(&envelope, key)?;
            let now = now_timestamp();
            let saved = if input.id.trim().is_empty() {
                if payload.items.len() >= MAX_ITEMS {
                    return Err(format!("凭据数量已达到 {MAX_ITEMS} 条上限"));
                }
                VaultCredential {
                    id: format!("credential-{}", Uuid::new_v4().simple()),
                    name: input.name.trim().to_owned(),
                    kind: input.kind,
                    username: input.username,
                    secret: input.secret,
                    uri: input.uri,
                    notes: input.notes,
                    tags: normalize_tags(input.tags),
                    favorite: input.favorite,
                    created_at: now,
                    updated_at: now,
                }
            } else {
                validate_id(&input.id)?;
                let existing = payload
                    .items
                    .iter()
                    .find(|item| item.id == input.id)
                    .ok_or_else(|| "凭据不存在".to_owned())?;
                VaultCredential {
                    id: existing.id.clone(),
                    name: input.name.trim().to_owned(),
                    kind: input.kind,
                    username: input.username,
                    secret: input.secret,
                    uri: input.uri,
                    notes: input.notes,
                    tags: normalize_tags(input.tags),
                    favorite: input.favorite,
                    created_at: existing.created_at,
                    updated_at: now,
                }
            };
            if let Some(existing) = payload.items.iter_mut().find(|item| item.id == saved.id) {
                *existing = saved.clone();
            } else {
                payload.items.push(saved.clone());
            }
            let _ = save_payload(paths, key, envelope, payload)?;
            Ok(saved)
        })
    }

    pub(crate) fn delete_credential(&self, paths: &AppPaths, id: &str) -> Result<(), String> {
        validate_id(id)?;
        self.with_key(|key| {
            let envelope = read_envelope(paths)?;
            let mut payload = decrypt_payload(&envelope, key)?;
            let previous = payload.items.len();
            payload.items.retain(|item| item.id != id);
            if payload.items.len() == previous {
                return Err("凭据不存在".to_owned());
            }
            let _ = save_payload(paths, key, envelope, payload)?;
            Ok(())
        })
    }

    fn verify_runtime_token(&self, token: &str) -> Result<(), String> {
        let expected = self.runtime_token()?;
        if token.len() != expected.len()
            || !constant_time_equal(token.as_bytes(), expected.as_bytes())
        {
            return Err("保险箱运行期 Token 无效".to_owned());
        }
        Ok(())
    }

    fn start_service(&self, port: u16, paths: AppPaths) -> Result<VaultServiceStatus, String> {
        if port < 1_024 {
            return Err("保险箱服务端口必须大于等于 1024".to_owned());
        }
        let listener = TcpListener::bind(("127.0.0.1", port))
            .map_err(|error| format!("启动保险箱服务失败：{error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        {
            let mut service = self
                .runtime
                .service
                .lock()
                .map_err(|_| "保险箱服务状态已损坏".to_owned())?;
            if service.running {
                return Err("保险箱服务已经在运行".to_owned());
            }
            service.running = true;
            service.port = port;
            service.started_at = Some(now_timestamp());
            service.last_error.clear();
            service.stop = Some(Arc::clone(&stop));
        }
        let manager = self.clone();
        let worker_manager = self.clone();
        thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let request_manager = manager.clone();
                        let request_paths = paths.clone();
                        thread::spawn(move || {
                            let _ =
                                handle_service_connection(stream, &request_manager, &request_paths);
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(25))
                    }
                    Err(error) => {
                        if let Ok(mut service) = worker_manager.runtime.service.lock() {
                            service.last_error = error.to_string();
                        }
                        break;
                    }
                }
            }
            if let Ok(mut service) = worker_manager.runtime.service.lock() {
                service.running = false;
                service.stop = None;
            }
        });
        self.service_status()
    }

    fn stop_service(&self) -> Result<VaultServiceStatus, String> {
        if let Some(stop) = self
            .runtime
            .service
            .lock()
            .map_err(|_| "保险箱服务状态已损坏".to_owned())?
            .stop
            .clone()
        {
            stop.store(true, Ordering::Relaxed);
        }
        for _ in 0..40 {
            if !self.service_status()?.running {
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
        self.service_status()
    }
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut normalized = tags
        .into_iter()
        .map(|tag| tag.trim().to_owned())
        .filter(|tag| !tag.is_empty())
        .collect::<Vec<_>>();
    normalized.sort_by_key(|tag| tag.to_lowercase());
    normalized.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    normalized.truncate(20);
    normalized
}

fn validate_id(value: &str) -> Result<(), String> {
    if value.len() < 2
        || value.len() > 96
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("凭据标识无效".to_owned());
    }
    Ok(())
}

fn validate_input(input: &VaultCredentialInput) -> Result<(), String> {
    if input.name.trim().is_empty() || input.name.chars().count() > 120 {
        return Err("凭据名称应为 1 到 120 个字符".to_owned());
    }
    if !matches!(
        input.kind.as_str(),
        "login" | "apiKey" | "token" | "database" | "ssh" | "secureNote"
    ) {
        return Err("凭据类型无效".to_owned());
    }
    if input.username.len() > 500
        || input.secret.len() > 64 * 1024
        || input.uri.len() > 4_096
        || input.notes.len() > 128 * 1024
    {
        return Err("凭据字段内容过长".to_owned());
    }
    if input.tags.len() > 50 || input.tags.iter().any(|tag| tag.chars().count() > 80) {
        return Err("凭据标签过多或过长".to_owned());
    }
    Ok(())
}

fn read_http_request(stream: &mut TcpStream) -> Result<HttpRequest, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .map_err(|error| error.to_string())?;
    let mut reader = BufReader::new(stream.try_clone().map_err(|error| error.to_string())?);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|error| error.to_string())?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_ascii_uppercase();
    let path = parts
        .next()
        .unwrap_or_default()
        .split('?')
        .next()
        .unwrap_or_default()
        .to_owned();
    if method.is_empty() || path.is_empty() {
        return Err("HTTP 请求行无效".to_owned());
    }
    let mut headers = HashMap::new();
    let mut header_bytes = request_line.len();
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|error| error.to_string())?;
        header_bytes += line.len();
        if header_bytes > 32 * 1024 {
            return Err("HTTP Header 过大".to_owned());
        }
        if line == "\r\n" || line == "\n" || line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    let content_length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_default();
    if content_length > MAX_HTTP_BODY {
        return Err("HTTP 请求体超过 256 KiB".to_owned());
    }
    let mut body = vec![0u8; content_length];
    reader
        .read_exact(&mut body)
        .map_err(|error| error.to_string())?;
    Ok(HttpRequest {
        method,
        path,
        headers,
        body,
    })
}

fn write_json_response(stream: &mut TcpStream, status: &str, value: &Value) -> Result<(), String> {
    let body = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json; charset=utf-8\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).map_err(|error| error.to_string())?;
    stream.write_all(&body).map_err(|error| error.to_string())
}

fn bearer_token(request: &HttpRequest) -> &str {
    request
        .headers
        .get("authorization")
        .and_then(|value| {
            value
                .strip_prefix("Bearer ")
                .or_else(|| value.strip_prefix("bearer "))
        })
        .unwrap_or_default()
        .trim()
}

fn request_json<T: serde::de::DeserializeOwned>(request: &HttpRequest) -> Result<T, String> {
    serde_json::from_slice(&request.body).map_err(|error| format!("JSON 请求体无效：{error}"))
}

fn handle_service_connection(
    mut stream: TcpStream,
    manager: &CredentialVaultManager,
    paths: &AppPaths,
) -> Result<(), String> {
    let request = match read_http_request(&mut stream) {
        Ok(request) => request,
        Err(error) => {
            return write_json_response(
                &mut stream,
                "400 Bad Request",
                &json!({"code":"bad_request","message":error}),
            );
        }
    };
    if request.method == "GET" && request.path == "/v1/vault/health" {
        return write_json_response(
            &mut stream,
            "200 OK",
            &json!({"status":"ok","initialized":envelope_path(paths).is_file(),"apiVersion":1}),
        );
    }
    if request.method == "POST" && request.path == "/v1/vault/session/verify" {
        #[derive(Deserialize)]
        struct VerifyRequest {
            code: String,
        }
        let verified = request_json::<VerifyRequest>(&request)
            .and_then(|body| manager.unlock_totp(paths, &body.code));
        return match verified {
            Ok(result) => write_json_response(
                &mut stream,
                "200 OK",
                &json!({"token":result.service_token,"expiresAt":result.status.unlocked_until}),
            ),
            Err(error) => write_json_response(
                &mut stream,
                "401 Unauthorized",
                &json!({"code":"verification_failed","message":error}),
            ),
        };
    }
    if let Err(error) = manager.verify_runtime_token(bearer_token(&request)) {
        return write_json_response(
            &mut stream,
            "401 Unauthorized",
            &json!({"code":"session_required","message":error}),
        );
    }
    if request.method == "GET" && request.path == "/v1/vault/credentials" {
        return match manager.list_credentials(paths) {
            Ok(items) => write_json_response(&mut stream, "200 OK", &json!({"items":items})),
            Err(error) => write_json_response(
                &mut stream,
                "500 Internal Server Error",
                &json!({"code":"vault_error","message":error}),
            ),
        };
    }
    if request.method == "POST" && request.path == "/v1/vault/credentials" {
        return match request_json::<VaultCredentialInput>(&request)
            .and_then(|input| manager.save_credential(paths, input))
        {
            Ok(item) => write_json_response(&mut stream, "201 Created", &json!({"item":item})),
            Err(error) => write_json_response(
                &mut stream,
                "400 Bad Request",
                &json!({"code":"save_failed","message":error}),
            ),
        };
    }
    let credential_id = request
        .path
        .strip_prefix("/v1/vault/credentials/")
        .filter(|id| !id.is_empty());
    if let Some(id) = credential_id {
        let response = match request.method.as_str() {
            "GET" => manager
                .get_credential(paths, id)
                .map(|item| json!({"item":item})),
            "PUT" => request_json::<VaultCredentialInput>(&request)
                .and_then(|mut input| {
                    input.id = id.to_owned();
                    manager.save_credential(paths, input)
                })
                .map(|item| json!({"item":item})),
            "DELETE" => manager
                .delete_credential(paths, id)
                .map(|_| json!({"ok":true})),
            _ => {
                return write_json_response(
                    &mut stream,
                    "405 Method Not Allowed",
                    &json!({"code":"method_not_allowed"}),
                );
            }
        };
        return match response {
            Ok(value) => write_json_response(&mut stream, "200 OK", &value),
            Err(error) => write_json_response(
                &mut stream,
                "400 Bad Request",
                &json!({"code":"credential_error","message":error}),
            ),
        };
    }
    write_json_response(
        &mut stream,
        "404 Not Found",
        &json!({"code":"not_found","message":"保险箱 API 路由不存在"}),
    )
}

#[tauri::command]
pub(crate) fn get_vault_status(
    manager: State<'_, CredentialVaultManager>,
    paths: State<'_, AppPaths>,
) -> Result<VaultStatus, String> {
    manager.status(&paths)
}

#[tauri::command]
pub(crate) fn begin_vault_setup(
    manager: State<'_, CredentialVaultManager>,
    paths: State<'_, AppPaths>,
) -> Result<VaultSetup, String> {
    manager.begin_setup(&paths)
}

#[tauri::command]
pub(crate) fn complete_vault_setup(
    setup_id: String,
    code: String,
    manager: State<'_, CredentialVaultManager>,
    paths: State<'_, AppPaths>,
) -> Result<VaultUnlockResult, String> {
    manager.complete_setup(&paths, &setup_id, &code)
}

#[tauri::command]
pub(crate) fn unlock_vault(
    code: String,
    manager: State<'_, CredentialVaultManager>,
    paths: State<'_, AppPaths>,
) -> Result<VaultUnlockResult, String> {
    manager.unlock_totp(&paths, &code)
}

#[tauri::command]
pub(crate) fn unlock_vault_with_recovery(
    recovery_code: String,
    manager: State<'_, CredentialVaultManager>,
    paths: State<'_, AppPaths>,
) -> Result<VaultUnlockResult, String> {
    manager.unlock_recovery(&paths, &recovery_code)
}

#[tauri::command]
pub(crate) fn lock_vault(manager: State<'_, CredentialVaultManager>) -> Result<(), String> {
    manager.lock()
}

#[tauri::command]
pub(crate) fn list_vault_credentials(
    manager: State<'_, CredentialVaultManager>,
    paths: State<'_, AppPaths>,
) -> Result<Vec<VaultCredentialSummary>, String> {
    manager.list_credentials(&paths)
}

#[tauri::command]
pub(crate) fn get_vault_credential(
    id: String,
    manager: State<'_, CredentialVaultManager>,
    paths: State<'_, AppPaths>,
) -> Result<VaultCredential, String> {
    manager.get_credential(&paths, &id)
}

#[tauri::command]
pub(crate) fn save_vault_credential(
    input: VaultCredentialInput,
    manager: State<'_, CredentialVaultManager>,
    paths: State<'_, AppPaths>,
) -> Result<VaultCredential, String> {
    manager.save_credential(&paths, input)
}

#[tauri::command]
pub(crate) fn delete_vault_credential(
    id: String,
    manager: State<'_, CredentialVaultManager>,
    paths: State<'_, AppPaths>,
) -> Result<(), String> {
    manager.delete_credential(&paths, &id)
}

#[tauri::command]
pub(crate) fn start_vault_service(
    port: u16,
    manager: State<'_, CredentialVaultManager>,
    paths: State<'_, AppPaths>,
) -> Result<VaultServiceStatus, String> {
    manager.start_service(port, paths.inner().clone())
}

#[tauri::command]
pub(crate) fn stop_vault_service(
    manager: State<'_, CredentialVaultManager>,
) -> Result<VaultServiceStatus, String> {
    manager.stop_service()
}

#[tauri::command]
pub(crate) fn export_vault_recovery_code(
    recovery_code: String,
    target_path: String,
) -> Result<String, String> {
    let _ = decode_recovery_code(&recovery_code)?;
    let path = PathBuf::from(target_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建恢复文件目录失败：{error}"))?;
    }
    let content = format!(
        "DRPA Credential Vault Recovery Code\r\n\r\n{recovery_code}\r\n\r\nStore this file offline. A recovery unlock rotates this code.\r\n"
    );
    fs::write(&path, content.as_bytes()).map_err(|error| format!("写入恢复码文件失败：{error}"))?;
    Ok(path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_paths() -> AppPaths {
        let root =
            std::env::temp_dir().join(format!("drpa-vault-test-{}", Uuid::new_v4().simple()));
        AppPaths {
            data_root: root.clone(),
            workspace_root: root.join("workspace"),
            resource_dir: None,
        }
    }

    fn initialize(manager: &CredentialVaultManager, paths: &AppPaths) -> VaultUnlockResult {
        let setup = manager.begin_setup(paths).unwrap();
        let secret = manager
            .runtime
            .setup
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .secret
            .to_vec();
        let code = totp_value(&secret, now_timestamp()).unwrap();
        manager
            .complete_setup(paths, &setup.setup_id, &code)
            .unwrap()
    }

    #[test]
    fn totp_matches_rfc_6238_sha1_projection() {
        assert_eq!(totp_value(b"12345678901234567890", 59).unwrap(), "287082");
    }

    #[test]
    fn encrypted_vault_round_trips_and_locks() {
        let paths = test_paths();
        let manager = CredentialVaultManager::default();
        let initialized = initialize(&manager, &paths);
        assert!(initialized.recovery_code.is_some());
        let saved = manager
            .save_credential(
                &paths,
                VaultCredentialInput {
                    id: String::new(),
                    name: "GitHub".to_owned(),
                    kind: "token".to_owned(),
                    username: "developer".to_owned(),
                    secret: "secret-token".to_owned(),
                    uri: "https://github.com".to_owned(),
                    notes: "local".to_owned(),
                    tags: vec!["dev".to_owned()],
                    favorite: true,
                },
            )
            .unwrap();
        assert_eq!(manager.list_credentials(&paths).unwrap().len(), 1);
        assert_eq!(
            manager.get_credential(&paths, &saved.id).unwrap().secret,
            "secret-token"
        );
        let source = fs::read_to_string(envelope_path(&paths)).unwrap();
        assert!(!source.contains("secret-token"));
        assert!(!source.contains("GitHub"));
        manager.lock().unwrap();
        assert!(manager.list_credentials(&paths).is_err());
        let _ = fs::remove_dir_all(paths.data_root);
    }

    #[test]
    fn recovery_unlock_rotates_recovery_code() {
        let paths = test_paths();
        let manager = CredentialVaultManager::default();
        let recovery = initialize(&manager, &paths).recovery_code.unwrap();
        manager.lock().unwrap();
        let recovered = manager.unlock_recovery(&paths, &recovery).unwrap();
        assert!(recovered.status.unlocked);
        assert_ne!(recovered.recovery_code.as_deref(), Some(recovery.as_str()));
        manager.lock().unwrap();
        assert!(manager.unlock_recovery(&paths, &recovery).is_err());
        let _ = fs::remove_dir_all(paths.data_root);
    }
}
