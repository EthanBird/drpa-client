use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

mod capabilities;

pub use capabilities::{
    BrowserResolution, BrowserSource, detect_system_browser, resolve_browser,
    resolve_fixed_webview2, system_webview2_version,
};

pub const INSTALL_MARKER: &str = ".drpa-install.json";
pub const COMPONENT_STATE: &str = "state/active-components.json";
pub const COMPONENT_LEASES: &str = "state/component-leases";
pub const CORE_FILE_STATE: &str = "state/core-files.json";
pub const COMPONENT_MANIFEST: &str = "component.json";
pub const COMPONENT_SCHEMA: u32 = 1;
const DESKTOP_COMPONENT_ID: &str = "org.drpa.desktop-ui";
const INSTALL_SCHEMA: u32 = 1;
const LOCATOR_SCHEMA: u32 = 1;
const MAX_COMPONENT_FILES: usize = 50_000;
const MAX_COMPONENT_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_COMPONENT_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum InstallError {
    #[error("文件系统操作失败：{0}")]
    Io(#[from] io::Error),
    #[error("JSON 数据无效：{0}")]
    Json(#[from] serde_json::Error),
    #[error("组件包无效：{0}")]
    Archive(#[from] zip::result::ZipError),
    #[error("安装目录不是 DRPA 安装：{0}")]
    NotAnInstallation(String),
    #[error("无效的组件清单：{0}")]
    InvalidManifest(String),
    #[error("组件包文件校验失败：{0}")]
    Verification(String),
    #[error("找不到组件：{0}")]
    ComponentNotFound(String),
    #[error("组件正在使用：{0}")]
    ComponentInUse(String),
    #[error("找不到 DRPA 安装目录")]
    InstallationNotFound,
}

pub type Result<T> = std::result::Result<T, InstallError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallMarker {
    pub schema: u32,
    pub install_id: String,
    pub layout_version: u32,
    pub channel: String,
    pub data_root: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocatorEntry {
    pub install_id: String,
    pub root: String,
    pub channel: String,
    pub last_seen_at: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocatorIndex {
    pub schema: u32,
    #[serde(default)]
    pub installations: Vec<LocatorEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
    #[serde(default)]
    pub executable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentManifest {
    pub schema: u32,
    pub id: String,
    pub version: String,
    pub platform: String,
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub provides: Vec<String>,
    #[serde(default)]
    pub requires: BTreeMap<String, String>,
    #[serde(default)]
    pub entrypoints: BTreeMap<String, String>,
    pub files: Vec<ComponentFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentSelection {
    pub version: String,
    pub relative_root: String,
    #[serde(default)]
    pub provides: Vec<String>,
    #[serde(default)]
    pub entrypoints: BTreeMap<String, String>,
    pub activated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentState {
    pub schema: u32,
    #[serde(default)]
    pub active: BTreeMap<String, ComponentSelection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentLeaseRecord {
    pub schema: u32,
    pub lease_id: String,
    pub component_id: String,
    pub version: String,
    pub purpose: String,
    pub pid: u32,
    pub created_at: u64,
}

#[derive(Debug)]
struct ComponentLeaseInner {
    path: PathBuf,
    record: ComponentLeaseRecord,
}

impl Drop for ComponentLeaseInner {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Keeps an installed component version alive while a task or service uses it.
/// Clones share one lease file; the file is removed after the last clone drops.
#[derive(Debug, Clone)]
pub struct ComponentLeaseGuard {
    inner: Arc<ComponentLeaseInner>,
}

impl ComponentLeaseGuard {
    pub fn record(&self) -> &ComponentLeaseRecord {
        &self.inner.record
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreFileState {
    pub schema: u32,
    #[serde(default)]
    pub files: BTreeSet<String>,
}

impl Default for ComponentState {
    fn default() -> Self {
        Self {
            schema: COMPONENT_SCHEMA,
            active: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct InstallLayout {
    root: PathBuf,
    marker: InstallMarker,
}

impl InstallLayout {
    pub fn initialize(root: impl AsRef<Path>, channel: &str) -> Result<Self> {
        let root = absolute_path(root.as_ref())?;
        fs::create_dir_all(root.join("components"))?;
        fs::create_dir_all(root.join("state"))?;
        fs::create_dir_all(root.join("data"))?;
        let marker_path = root.join(INSTALL_MARKER);
        let marker = if marker_path.is_file() {
            read_json(&marker_path)?
        } else {
            let marker = InstallMarker {
                schema: INSTALL_SCHEMA,
                install_id: Uuid::new_v4().simple().to_string(),
                layout_version: 1,
                channel: validate_token(channel, "安装通道")?.to_owned(),
                data_root: "data".to_owned(),
                created_at: now_millis(),
            };
            write_json_atomic(&marker_path, &marker)?;
            marker
        };
        validate_marker(&marker)?;
        let layout = Self { root, marker };
        if !layout.state_path().is_file() {
            write_json_atomic(&layout.state_path(), &ComponentState::default())?;
        }
        Ok(layout)
    }

    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = absolute_path(root.as_ref())?;
        let marker_path = root.join(INSTALL_MARKER);
        if !marker_path.is_file() {
            return Err(InstallError::NotAnInstallation(root.display().to_string()));
        }
        let marker: InstallMarker = read_json(&marker_path)?;
        validate_marker(&marker)?;
        Ok(Self { root, marker })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn marker(&self) -> &InstallMarker {
        &self.marker
    }

    pub fn data_root(&self) -> PathBuf {
        self.root.join(&self.marker.data_root)
    }

    pub fn component_root(&self, id: &str, version: &str) -> Result<PathBuf> {
        validate_token(id, "组件标识")?;
        validate_token(version, "组件版本")?;
        Ok(self.root.join("components").join(id).join(version))
    }

    pub fn state_path(&self) -> PathBuf {
        self.root.join(COMPONENT_STATE)
    }

    pub fn reconcile_core_files(&self, desired: &BTreeSet<String>) -> Result<Vec<String>> {
        for relative in desired {
            validate_core_file_path(relative)?;
            if !safe_join(&self.root, relative)?.is_file() {
                return Err(InstallError::Verification(format!(
                    "核心清单中的文件尚未安装：{relative}"
                )));
            }
        }
        let state_path = self.root.join(CORE_FILE_STATE);
        let previous: CoreFileState = if state_path.is_file() {
            read_json(&state_path)?
        } else {
            CoreFileState::default()
        };
        let mut removed = Vec::new();
        for relative in previous.files.difference(desired) {
            validate_core_file_path(relative)?;
            let path = safe_join(&self.root, relative)?;
            if path.is_file() || path.is_symlink() {
                fs::remove_file(&path)?;
                removed.push(relative.clone());
                remove_empty_parents(&self.root, path.parent());
            }
        }
        write_json_atomic(
            &state_path,
            &CoreFileState {
                schema: COMPONENT_SCHEMA,
                files: desired.clone(),
            },
        )?;
        Ok(removed)
    }

    pub fn read_state(&self) -> Result<ComponentState> {
        if self.state_path().is_file() {
            read_json(&self.state_path())
        } else {
            Ok(ComponentState::default())
        }
    }

    pub fn active_component(&self, id: &str) -> Result<Option<(ComponentSelection, PathBuf)>> {
        validate_token(id, "组件标识")?;
        let state = self.read_state()?;
        let Some(selection) = state.active.get(id).cloned() else {
            return Ok(None);
        };
        let root = safe_join(&self.root, &selection.relative_root)?;
        Ok(Some((selection, root)))
    }

    pub fn provider_for(
        &self,
        capability: &str,
    ) -> Result<Option<(String, ComponentSelection, PathBuf)>> {
        let state = self.read_state()?;
        for (id, selection) in state.active {
            if selection.provides.iter().any(|item| item == capability) {
                let root = safe_join(&self.root, &selection.relative_root)?;
                return Ok(Some((id, selection, root)));
            }
        }
        Ok(None)
    }

    pub fn providers_for(
        &self,
        capability: &str,
    ) -> Result<Vec<(String, ComponentSelection, PathBuf)>> {
        let state = self.read_state()?;
        let mut providers = Vec::new();
        for (id, selection) in state.active {
            if selection.provides.iter().any(|item| item == capability) {
                let root = safe_join(&self.root, &selection.relative_root)?;
                providers.push((id, selection, root));
            }
        }
        Ok(providers)
    }

    pub fn acquire_component_lease(
        &self,
        id: &str,
        version: &str,
        purpose: &str,
    ) -> Result<ComponentLeaseGuard> {
        validate_token(id, "组件标识")?;
        validate_token(version, "组件版本")?;
        let purpose = purpose.trim();
        if purpose.is_empty() || purpose.len() > 160 {
            return Err(InstallError::InvalidManifest(
                "组件租约用途不能为空且不能超过 160 个字符".to_owned(),
            ));
        }
        let root = self.component_root(id, version)?;
        if !root.is_dir() {
            return Err(InstallError::ComponentNotFound(format!("{id}@{version}")));
        }
        let leases_root = self.root.join(COMPONENT_LEASES);
        fs::create_dir_all(&leases_root)?;
        let lease_id = Uuid::new_v4().simple().to_string();
        let record = ComponentLeaseRecord {
            schema: COMPONENT_SCHEMA,
            lease_id: lease_id.clone(),
            component_id: id.to_owned(),
            version: version.to_owned(),
            purpose: purpose.to_owned(),
            pid: std::process::id(),
            created_at: now_millis(),
        };
        let path = leases_root.join(format!("{lease_id}.json"));
        write_json_atomic(&path, &record)?;
        Ok(ComponentLeaseGuard {
            inner: Arc::new(ComponentLeaseInner { path, record }),
        })
    }

    pub fn active_component_leases(
        &self,
        id: &str,
        version: Option<&str>,
    ) -> Result<Vec<ComponentLeaseRecord>> {
        validate_token(id, "组件标识")?;
        if let Some(version) = version {
            validate_token(version, "组件版本")?;
        }
        let leases_root = self.root.join(COMPONENT_LEASES);
        if !leases_root.is_dir() {
            return Ok(Vec::new());
        }
        let mut leases = Vec::new();
        for entry in fs::read_dir(&leases_root)?.flatten() {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|item| item.to_str()) != Some("json") {
                continue;
            }
            let Ok(record) = read_json::<ComponentLeaseRecord>(&path) else {
                let _ = fs::remove_file(&path);
                continue;
            };
            if record.schema != COMPONENT_SCHEMA || !process_is_alive(record.pid) {
                let _ = fs::remove_file(&path);
                continue;
            }
            if record.component_id == id
                && version.is_none_or(|expected| record.version == expected)
            {
                leases.push(record);
            }
        }
        leases.sort_by_key(|lease| lease.created_at);
        Ok(leases)
    }

    pub fn resolve_provider_entrypoint(
        &self,
        capability: &str,
        name: &str,
    ) -> Result<Option<(String, PathBuf)>> {
        let Some((id, selection, root)) = self.provider_for(capability)? else {
            return Ok(None);
        };
        let Some(relative) = selection.entrypoints.get(name) else {
            return Ok(None);
        };
        let path = safe_join(&root, relative)?;
        Ok(path.is_file().then_some((id, path)))
    }

    pub fn resolve_entrypoint(&self, id: &str, name: &str) -> Result<Option<PathBuf>> {
        let Some((selection, root)) = self.active_component(id)? else {
            return Ok(None);
        };
        let Some(relative) = selection.entrypoints.get(name) else {
            return Ok(None);
        };
        let path = safe_join(&root, relative)?;
        Ok(path.is_file().then_some(path))
    }

    pub fn install_component(&self, archive_path: impl AsRef<Path>) -> Result<ComponentManifest> {
        let file = File::open(archive_path)?;
        let mut archive = ZipArchive::new(file)?;
        let manifest = read_component_manifest(&mut archive)?;
        validate_component_manifest(&manifest)?;
        self.validate_requirements(&manifest)?;
        if manifest.platform != current_platform() && manifest.platform != "any" {
            return Err(InstallError::InvalidManifest(format!(
                "组件平台 {} 与当前平台 {} 不匹配",
                manifest.platform,
                current_platform()
            )));
        }
        let target = self.component_root(&manifest.id, &manifest.version)?;
        if target.is_dir() {
            let leases = self.active_component_leases(&manifest.id, Some(&manifest.version))?;
            if !leases.is_empty() {
                return Err(component_in_use_error(
                    &manifest.id,
                    &manifest.version,
                    &leases,
                ));
            }
        }
        let component_parent = target
            .parent()
            .ok_or_else(|| InstallError::InvalidManifest("组件安装路径无效".to_owned()))?;
        fs::create_dir_all(component_parent)?;
        set_component_directory_permissions(component_parent)?;
        let staging = component_parent.join(format!(".staging-{}", Uuid::new_v4().simple()));
        fs::create_dir_all(&staging)?;
        set_component_directory_permissions(&staging)?;
        let extraction = extract_component(&mut archive, &manifest, &staging);
        if let Err(error) = extraction {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        write_json_atomic(&staging.join(COMPONENT_MANIFEST), &manifest)?;
        set_component_file_permissions(&staging.join(COMPONENT_MANIFEST), false)?;
        set_component_tree_directory_permissions(&staging)?;

        let backup = component_parent.join(format!(".backup-{}", Uuid::new_v4().simple()));
        if target.exists() {
            fs::rename(&target, &backup)?;
        }
        if let Err(error) = fs::rename(&staging, &target) {
            if backup.exists() {
                let _ = fs::rename(&backup, &target);
            }
            return Err(InstallError::Io(error));
        }

        let mut state = self.read_state()?;
        let previous_state = state.clone();
        state.schema = COMPONENT_SCHEMA;
        state.active.insert(
            manifest.id.clone(),
            ComponentSelection {
                version: manifest.version.clone(),
                relative_root: relative_string(&self.root, &target)?,
                provides: manifest.provides.clone(),
                entrypoints: manifest.entrypoints.clone(),
                activated_at: now_millis(),
            },
        );
        if let Err(error) = write_json_atomic(&self.state_path(), &state) {
            let _ = fs::remove_dir_all(&target);
            if backup.exists() {
                let _ = fs::rename(&backup, &target);
            }
            return Err(error);
        }
        if manifest.id == DESKTOP_COMPONENT_ID
            && let Err(error) = self.sync_desktop_compatibility_links(&state)
        {
            let _ = write_json_atomic(&self.state_path(), &previous_state);
            let _ = fs::remove_dir_all(&target);
            if backup.exists() {
                let _ = fs::rename(&backup, &target);
            }
            let _ = self.sync_desktop_compatibility_links(&previous_state);
            return Err(error);
        }
        if backup.exists() {
            fs::remove_dir_all(backup)?;
        }
        Ok(manifest)
    }

    pub fn remove_component(&self, id: &str, purge_versions: bool) -> Result<()> {
        validate_token(id, "组件标识")?;
        let mut state = self.read_state()?;
        let previous_state = state.clone();
        let provided = state
            .active
            .get(id)
            .map(|item| item.provides.iter().cloned().collect::<BTreeSet<_>>())
            .unwrap_or_default();
        for (other_id, selection) in &state.active {
            if other_id == id {
                continue;
            }
            let root = safe_join(&self.root, &selection.relative_root)?;
            let manifest: ComponentManifest = read_json(&root.join(COMPONENT_MANIFEST))?;
            if let Some(capability) = manifest
                .requires
                .keys()
                .find(|item| provided.contains(*item))
            {
                return Err(InstallError::Verification(format!(
                    "组件 {other_id} 仍依赖 {capability}，不能移除 {id}"
                )));
            }
        }
        let selection = state
            .active
            .remove(id)
            .ok_or_else(|| InstallError::ComponentNotFound(id.to_owned()))?;
        let leases = self.active_component_leases(
            id,
            if purge_versions {
                None
            } else {
                Some(&selection.version)
            },
        )?;
        if !leases.is_empty() {
            return Err(component_in_use_error(id, &selection.version, &leases));
        }
        let version_root = safe_join(&self.root, &selection.relative_root)?;
        let removal_root = if purge_versions {
            self.root.join("components").join(id)
        } else {
            version_root
        };
        let quarantine = removal_root
            .parent()
            .map(|parent| parent.join(format!(".removing-{}-{}", id, Uuid::new_v4().simple())));
        let quarantined = if removal_root.is_dir() {
            let quarantine = quarantine
                .ok_or_else(|| InstallError::InvalidManifest("组件卸载暂存路径无效".to_owned()))?;
            fs::rename(&removal_root, &quarantine)?;
            Some(quarantine)
        } else {
            None
        };
        if let Err(error) = write_json_atomic(&self.state_path(), &state) {
            if let Some(quarantine) = &quarantined {
                let _ = fs::rename(quarantine, &removal_root);
            }
            return Err(error);
        }
        if id == DESKTOP_COMPONENT_ID
            && let Err(error) = self.sync_desktop_compatibility_links(&state)
        {
            let _ = write_json_atomic(&self.state_path(), &previous_state);
            if let Some(quarantine) = &quarantined {
                let _ = fs::rename(quarantine, &removal_root);
            }
            let _ = self.sync_desktop_compatibility_links(&previous_state);
            return Err(error);
        }
        if let Some(quarantine) = &quarantined
            && let Err(remove_error) = fs::remove_dir_all(quarantine)
        {
            let state_restore = write_json_atomic(&self.state_path(), &previous_state);
            let directory_restore = fs::rename(quarantine, &removal_root);
            if id == DESKTOP_COMPONENT_ID {
                let _ = self.sync_desktop_compatibility_links(&previous_state);
            }
            if state_restore.is_err() || directory_restore.is_err() {
                return Err(InstallError::Verification(format!(
                    "组件文件删除失败且回滚不完整：{remove_error}；状态恢复：{}；目录恢复：{}",
                    state_restore
                        .err()
                        .map(|error| error.to_string())
                        .unwrap_or_else(|| "成功".to_owned()),
                    directory_restore
                        .err()
                        .map(|error| error.to_string())
                        .unwrap_or_else(|| "成功".to_owned())
                )));
            }
            return Err(InstallError::Io(remove_error));
        }
        Ok(())
    }

    pub fn activate_component(&self, id: &str, version: &str) -> Result<ComponentManifest> {
        let root = self.component_root(id, version)?;
        if !root.is_dir() {
            return Err(InstallError::ComponentNotFound(format!("{id}@{version}")));
        }
        let manifest: ComponentManifest = read_json(&root.join(COMPONENT_MANIFEST))?;
        if manifest.id != id || manifest.version != version {
            return Err(InstallError::Verification(
                "组件目录与清单标识不一致".to_owned(),
            ));
        }
        self.verify_manifest_files(&root, &manifest)?;
        let mut state = self.read_state()?;
        let previous_state = state.clone();
        state.active.insert(
            id.to_owned(),
            ComponentSelection {
                version: version.to_owned(),
                relative_root: relative_string(&self.root, &root)?,
                provides: manifest.provides.clone(),
                entrypoints: manifest.entrypoints.clone(),
                activated_at: now_millis(),
            },
        );
        write_json_atomic(&self.state_path(), &state)?;
        if id == DESKTOP_COMPONENT_ID
            && let Err(error) = self.sync_desktop_compatibility_links(&state)
        {
            let _ = write_json_atomic(&self.state_path(), &previous_state);
            let _ = self.sync_desktop_compatibility_links(&previous_state);
            return Err(error);
        }
        Ok(manifest)
    }

    fn sync_desktop_compatibility_links(&self, state: &ComponentState) -> Result<()> {
        let root = state
            .active
            .get(DESKTOP_COMPONENT_ID)
            .map(|selection| safe_join(&self.root, &selection.relative_root))
            .transpose()?;
        sync_desktop_compatibility_links(&self.root, root.as_deref())
    }

    pub fn reconcile_components(
        &self,
        desired: &BTreeSet<String>,
        managed: &BTreeSet<String>,
    ) -> Result<Vec<String>> {
        let active = self.read_state()?.active;
        let mut removed = Vec::new();
        for id in active.keys() {
            if managed.contains(id) && !desired.contains(id) {
                self.remove_component(id, true)?;
                removed.push(id.clone());
            }
        }
        Ok(removed)
    }

    pub fn garbage_collect_versions(&self, id: &str, keep: usize) -> Result<Vec<String>> {
        validate_token(id, "组件标识")?;
        if keep == 0 {
            return Err(InstallError::InvalidManifest(
                "至少需要保留一个组件版本".to_owned(),
            ));
        }
        let active = self.active_component(id)?.map(|(item, _)| item.version);
        let component_root = self.root.join("components").join(id);
        if !component_root.is_dir() {
            return Ok(Vec::new());
        }
        let mut versions = fs::read_dir(&component_root)?
            .flatten()
            .filter(|entry| {
                entry.path().is_dir()
                    && !entry.file_name().to_string_lossy().starts_with('.')
                    && entry.path().join(COMPONENT_MANIFEST).is_file()
            })
            .map(|entry| {
                let modified = entry
                    .metadata()
                    .and_then(|item| item.modified())
                    .unwrap_or(UNIX_EPOCH);
                (entry.file_name().to_string_lossy().into_owned(), modified)
            })
            .collect::<Vec<_>>();
        versions.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| right.0.cmp(&left.0)));
        let mut retained = BTreeSet::new();
        if let Some(active) = &active {
            retained.insert(active.clone());
        }
        for (version, _) in &versions {
            if retained.len() >= keep {
                break;
            }
            retained.insert(version.clone());
        }
        let mut removed = Vec::new();
        for (version, _) in versions {
            if !retained.contains(&version) {
                if !self.active_component_leases(id, Some(&version))?.is_empty() {
                    continue;
                }
                fs::remove_dir_all(self.component_root(id, &version)?)?;
                removed.push(version);
            }
        }
        Ok(removed)
    }

    pub fn verify_component(&self, id: &str) -> Result<ComponentManifest> {
        let Some((_selection, root)) = self.active_component(id)? else {
            return Err(InstallError::ComponentNotFound(id.to_owned()));
        };
        let manifest: ComponentManifest = read_json(&root.join(COMPONENT_MANIFEST))?;
        validate_component_manifest(&manifest)?;
        self.verify_manifest_files(&root, &manifest)?;
        Ok(manifest)
    }

    fn verify_manifest_files(&self, root: &Path, manifest: &ComponentManifest) -> Result<()> {
        for file in &manifest.files {
            let path = safe_join(&root, &file.path)?;
            if !path.is_file() {
                return Err(InstallError::Verification(format!("缺少 {}", file.path)));
            }
            let metadata = fs::metadata(&path)?;
            if metadata.len() != file.bytes {
                return Err(InstallError::Verification(format!(
                    "{} 大小不匹配",
                    file.path
                )));
            }
            let digest = sha256_file(&path)?;
            if digest != file.sha256 {
                return Err(InstallError::Verification(format!(
                    "{} 哈希不匹配",
                    file.path
                )));
            }
        }
        Ok(())
    }

    fn validate_requirements(&self, manifest: &ComponentManifest) -> Result<()> {
        let state = self.read_state()?;
        for (capability, requirement) in &manifest.requires {
            let provider = state
                .active
                .iter()
                .find(|(_id, selection)| selection.provides.iter().any(|item| item == capability))
                .ok_or_else(|| {
                    InstallError::Verification(format!(
                        "组件 {} 缺少能力依赖 {capability}",
                        manifest.id
                    ))
                })?;
            let version = Version::parse(&provider.1.version).map_err(|error| {
                InstallError::Verification(format!("依赖提供者 {} 的版本无效：{error}", provider.0))
            })?;
            let requirement = VersionReq::parse(if requirement.trim().is_empty() {
                "*"
            } else {
                requirement
            })
            .map_err(|error| {
                InstallError::InvalidManifest(format!("能力 {capability} 的版本约束无效：{error}"))
            })?;
            if !requirement.matches(&version) {
                return Err(InstallError::Verification(format!(
                    "组件 {} 需要 {capability} {requirement}，当前提供者 {} 为 {version}",
                    manifest.id, provider.0
                )));
            }
        }
        Ok(())
    }
}

fn component_in_use_error(
    id: &str,
    version: &str,
    leases: &[ComponentLeaseRecord],
) -> InstallError {
    let purposes = leases
        .iter()
        .take(4)
        .map(|lease| format!("{} (PID {})", lease.purpose, lease.pid))
        .collect::<Vec<_>>()
        .join("、");
    InstallError::ComponentInUse(format!("{id}@{version} 正被 {purposes} 使用"))
}

#[cfg(windows)]
fn process_is_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut exit_code = 0;
        let ok = GetExitCodeProcess(handle, &mut exit_code) != 0;
        let _ = CloseHandle(handle);
        ok && exit_code == STILL_ACTIVE as u32
    }
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    result == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(any(windows, unix)))]
fn process_is_alive(pid: u32) -> bool {
    pid == std::process::id()
}

pub fn register_installation(layout: &InstallLayout) -> Result<PathBuf> {
    let path = locator_index_path()?;
    let mut index: LocatorIndex = if path.is_file() {
        read_json(&path)?
    } else {
        LocatorIndex {
            schema: LOCATOR_SCHEMA,
            installations: Vec::new(),
        }
    };
    index.schema = LOCATOR_SCHEMA;
    index
        .installations
        .retain(|entry| entry.install_id != layout.marker.install_id);
    index.installations.insert(
        0,
        LocatorEntry {
            install_id: layout.marker.install_id.clone(),
            root: layout.root.display().to_string(),
            channel: layout.marker.channel.clone(),
            last_seen_at: now_millis(),
        },
    );
    write_json_atomic(&path, &index)?;
    Ok(path)
}

pub fn unregister_installation(install_id: &str) -> Result<()> {
    validate_token(install_id, "安装标识")?;
    let path = locator_index_path()?;
    if !path.is_file() {
        return Ok(());
    }
    let mut index: LocatorIndex = read_json(&path)?;
    index
        .installations
        .retain(|entry| entry.install_id != install_id);
    write_json_atomic(&path, &index)
}

pub fn discover_installation(explicit: Option<&Path>) -> Result<InstallLayout> {
    if let Some(path) = explicit {
        return InstallLayout::open(path);
    }
    if let Some(path) = env::var_os("DRPA_INSTALL_ROOT") {
        return InstallLayout::open(PathBuf::from(path));
    }
    if let Ok(executable) = env::current_exe() {
        for root in executable.ancestors().skip(1).take(5) {
            if root.join(INSTALL_MARKER).is_file() {
                return InstallLayout::open(root);
            }
        }
    }
    let locator = locator_index_path()?;
    if locator.is_file() {
        let index: LocatorIndex = read_json(&locator)?;
        for entry in index.installations {
            if let Ok(layout) = InstallLayout::open(&entry.root) {
                return Ok(layout);
            }
        }
    }
    Err(InstallError::InstallationNotFound)
}

pub fn build_component_pack(
    source_root: impl AsRef<Path>,
    output: impl AsRef<Path>,
    mut manifest: ComponentManifest,
) -> Result<ComponentManifest> {
    validate_token(&manifest.id, "组件标识")?;
    validate_token(&manifest.version, "组件版本")?;
    let source_root = absolute_path(source_root.as_ref())?;
    let mut paths = Vec::new();
    collect_files(&source_root, &source_root, &mut paths)?;
    if paths.len() > MAX_COMPONENT_FILES {
        return Err(InstallError::InvalidManifest(
            "组件文件数量超过上限".to_owned(),
        ));
    }
    manifest.schema = COMPONENT_SCHEMA;
    manifest.files.clear();
    let output = output.as_ref();
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = File::create(output)?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut total = 0_u64;
    for path in paths {
        let relative = relative_string(&source_root, &path)?;
        let metadata = fs::metadata(&path)?;
        total = total.saturating_add(metadata.len());
        if total > MAX_COMPONENT_BYTES {
            return Err(InstallError::InvalidManifest("组件大小超过上限".to_owned()));
        }
        manifest.files.push(ComponentFile {
            path: relative.clone(),
            bytes: metadata.len(),
            sha256: sha256_file(&path)?,
            executable: is_executable(&metadata),
        });
        writer.start_file(format!("payload/{relative}"), options)?;
        let mut source = File::open(path)?;
        io::copy(&mut source, &mut writer)?;
    }
    validate_component_manifest(&manifest)?;
    writer.start_file(COMPONENT_MANIFEST, options)?;
    writer.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
    writer.finish()?;
    Ok(manifest)
}

pub fn verify_component_pack(archive_path: impl AsRef<Path>) -> Result<ComponentManifest> {
    let file = File::open(archive_path)?;
    let mut archive = ZipArchive::new(file)?;
    let manifest = read_component_manifest(&mut archive)?;
    validate_component_manifest(&manifest)?;
    let expected = manifest
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<BTreeSet<_>>();
    let mut verified = BTreeSet::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.is_dir() || entry.name() == COMPONENT_MANIFEST {
            continue;
        }
        let entry_name = entry.name().to_owned();
        let Some(relative) = entry_name.strip_prefix("payload/") else {
            return Err(InstallError::Verification(format!(
                "组件包包含未知条目 {entry_name}"
            )));
        };
        if !expected.contains(relative) || !verified.insert(relative.to_owned()) {
            return Err(InstallError::Verification(format!(
                "组件包包含未声明或重复文件 {relative}"
            )));
        }
        let file = manifest
            .files
            .iter()
            .find(|item| item.path == relative)
            .expect("set verified");
        if entry.size() != file.bytes {
            return Err(InstallError::Verification(format!("{relative} 大小不匹配")));
        }
        let mut hasher = Sha256::new();
        let mut copied = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = entry.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            copied = copied.saturating_add(read as u64);
            if copied > file.bytes {
                return Err(InstallError::Verification(format!("{relative} 内容超长")));
            }
            hasher.update(&buffer[..read]);
        }
        if copied != file.bytes || hex::encode(hasher.finalize()) != file.sha256 {
            return Err(InstallError::Verification(format!("{relative} 哈希不匹配")));
        }
    }
    if verified.len() != expected.len() {
        let missing = expected
            .into_iter()
            .find(|path| !verified.contains(*path))
            .unwrap_or("unknown");
        return Err(InstallError::Verification(format!("组件包缺少 {missing}")));
    }
    Ok(manifest)
}

pub fn current_platform() -> &'static str {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "windows-x86_64"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "linux-x86_64"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "macos-arm64"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "macos-x86_64"
    } else {
        "unsupported"
    }
}

fn read_component_manifest<R: Read + io::Seek>(
    archive: &mut ZipArchive<R>,
) -> Result<ComponentManifest> {
    let mut entry = archive
        .by_name(COMPONENT_MANIFEST)
        .map_err(|_| InstallError::InvalidManifest("组件包根目录缺少 component.json".to_owned()))?;
    if entry.size() > MAX_COMPONENT_MANIFEST_BYTES {
        return Err(InstallError::InvalidManifest(
            "component.json 过大".to_owned(),
        ));
    }
    let mut source = String::new();
    entry.read_to_string(&mut source)?;
    Ok(serde_json::from_str(&source)?)
}

fn extract_component<R: Read + io::Seek>(
    archive: &mut ZipArchive<R>,
    manifest: &ComponentManifest,
    staging: &Path,
) -> Result<()> {
    let expected = manifest
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<BTreeSet<_>>();
    let mut extracted = BTreeSet::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.is_dir() || entry.name() == COMPONENT_MANIFEST {
            continue;
        }
        let entry_name = entry.name().to_owned();
        let Some(relative) = entry_name.strip_prefix("payload/") else {
            return Err(InstallError::Verification(format!(
                "组件包包含未知条目 {}",
                entry_name
            )));
        };
        let relative = relative.to_owned();
        if !expected.contains(relative.as_str()) || !extracted.insert(relative.clone()) {
            return Err(InstallError::Verification(format!(
                "组件包包含未声明或重复文件 {relative}"
            )));
        }
        let file = manifest
            .files
            .iter()
            .find(|item| item.path == relative)
            .expect("set verified");
        if entry.size() != file.bytes {
            return Err(InstallError::Verification(format!("{relative} 大小不匹配")));
        }
        let target = safe_join(staging, &relative)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = File::create(&target)?;
        let mut hasher = Sha256::new();
        let mut copied = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = entry.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            copied = copied.saturating_add(read as u64);
            if copied > file.bytes {
                return Err(InstallError::Verification(format!("{relative} 内容超长")));
            }
            hasher.update(&buffer[..read]);
            output.write_all(&buffer[..read])?;
        }
        if copied != file.bytes || hex::encode(hasher.finalize()) != file.sha256 {
            return Err(InstallError::Verification(format!("{relative} 哈希不匹配")));
        }
        set_component_file_permissions(&target, file.executable)?;
    }
    if extracted.len() != expected.len() {
        let missing = expected
            .into_iter()
            .find(|path| !extracted.contains(*path))
            .unwrap_or("unknown");
        return Err(InstallError::Verification(format!("组件包缺少 {missing}")));
    }
    Ok(())
}

fn validate_component_manifest(manifest: &ComponentManifest) -> Result<()> {
    if manifest.schema != COMPONENT_SCHEMA {
        return Err(InstallError::InvalidManifest(format!(
            "不支持 schema {}",
            manifest.schema
        )));
    }
    validate_token(&manifest.id, "组件标识")?;
    validate_token(&manifest.version, "组件版本")?;
    if manifest.display_name.trim().is_empty() || manifest.display_name.chars().count() > 120 {
        return Err(InstallError::InvalidManifest("组件名称无效".to_owned()));
    }
    if manifest.description.chars().count() > 1_000 || manifest.description.contains('\0') {
        return Err(InstallError::InvalidManifest("组件描述无效".to_owned()));
    }
    if manifest.files.len() > MAX_COMPONENT_FILES {
        return Err(InstallError::InvalidManifest(
            "组件文件数量超过上限".to_owned(),
        ));
    }
    let mut seen = BTreeSet::new();
    let mut total = 0_u64;
    for file in &manifest.files {
        safe_relative(&file.path)?;
        if !seen.insert(&file.path) {
            return Err(InstallError::InvalidManifest(format!(
                "重复文件 {}",
                file.path
            )));
        }
        if file.sha256.len() != 64 || !file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(InstallError::InvalidManifest(format!(
                "{} 的 sha256 无效",
                file.path
            )));
        }
        total = total.saturating_add(file.bytes);
    }
    if total > MAX_COMPONENT_BYTES {
        return Err(InstallError::InvalidManifest("组件大小超过上限".to_owned()));
    }
    for relative in manifest.entrypoints.values() {
        safe_relative(relative)?;
        if !seen.contains(relative) {
            return Err(InstallError::InvalidManifest(format!(
                "入口点未包含在文件清单：{relative}"
            )));
        }
    }
    Ok(())
}

fn validate_marker(marker: &InstallMarker) -> Result<()> {
    if marker.schema != INSTALL_SCHEMA || marker.layout_version != 1 {
        return Err(InstallError::NotAnInstallation(
            "不支持的安装布局版本".to_owned(),
        ));
    }
    validate_token(&marker.install_id, "安装标识")?;
    safe_relative(&marker.data_root)?;
    Ok(())
}

fn validate_token<'a>(value: &'a str, label: &str) -> Result<&'a str> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        Err(InstallError::InvalidManifest(format!(
            "{label}无效：{value}"
        )))
    } else {
        Ok(value)
    }
}

fn safe_relative(relative: &str) -> Result<PathBuf> {
    let normalized = relative.replace('\\', "/");
    let path = Path::new(&normalized);
    if normalized.is_empty()
        || path.is_absolute()
        || normalized.contains(':')
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(InstallError::InvalidManifest(format!(
            "不安全路径：{relative}"
        )));
    }
    Ok(path.to_path_buf())
}

fn validate_core_file_path(relative: &str) -> Result<()> {
    safe_relative(relative)?;
    let folded = relative.replace('\\', "/").to_ascii_lowercase();
    if folded == INSTALL_MARKER.to_ascii_lowercase()
        || folded.starts_with("data/")
        || folded.starts_with("components/")
        || folded.starts_with("state/")
    {
        Err(InstallError::InvalidManifest(format!(
            "核心清单包含受保护路径：{relative}"
        )))
    } else {
        Ok(())
    }
}

fn remove_empty_parents(root: &Path, mut parent: Option<&Path>) {
    while let Some(path) = parent {
        if path == root || !path.starts_with(root) {
            break;
        }
        if fs::remove_dir(path).is_err() {
            break;
        }
        parent = path.parent();
    }
}

fn safe_join(root: &Path, relative: &str) -> Result<PathBuf> {
    Ok(root.join(safe_relative(relative)?))
}

fn relative_string(root: &Path, path: &Path) -> Result<String> {
    let relative = path.strip_prefix(root).map_err(|_| {
        InstallError::InvalidManifest(format!("{} 不在 {} 内", path.display(), root.display()))
    })?;
    let value = relative.to_string_lossy().replace('\\', "/");
    safe_relative(&value)?;
    Ok(value)
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(env::current_dir()?.join(path))
    }
}

fn locator_index_path() -> Result<PathBuf> {
    if let Some(root) = env::var_os("DRPA_LOCATOR_HOME") {
        return Ok(PathBuf::from(root).join("installations-v1.json"));
    }
    #[cfg(windows)]
    let root = env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let root = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")));
    root.map(|path| path.join("DRPA/installations-v1.json"))
        .ok_or_else(|| InstallError::NotAnInstallation("无法确定用户状态目录".to_owned()))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    Ok(serde_json::from_reader(File::open(path)?)?)
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("tmp-{}", Uuid::new_v4().simple()));
    let mut file = File::create(&temporary)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    set_component_file_permissions(&temporary, false)?;
    let backup = path.with_extension(format!("backup-{}", Uuid::new_v4().simple()));
    if path.exists() {
        fs::rename(path, &backup)?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if backup.exists() {
            let _ = fs::rename(&backup, path);
        }
        return Err(InstallError::Io(error));
    }
    if backup.exists() {
        fs::remove_file(backup)?;
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn collect_files(root: &Path, current: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(InstallError::InvalidManifest(format!(
                "组件源目录不能包含符号链接：{}",
                path.display()
            )));
        }
        if metadata.is_dir() {
            collect_files(root, &path, output)?;
        } else if metadata.is_file() {
            path.strip_prefix(root)
                .map_err(|_| InstallError::InvalidManifest("组件源路径越界".to_owned()))?;
            output.push(path);
        }
    }
    output.sort();
    Ok(())
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(unix)]
fn set_component_file_permissions(path: &Path, executable: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(if executable { 0o755 } else { 0o644 });
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_component_file_permissions(_path: &Path, _executable: bool) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_component_directory_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_component_directory_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_component_tree_directory_permissions(root: &Path) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if fs::symlink_metadata(&path)?.is_dir() {
            set_component_tree_directory_permissions(&path)?;
            set_component_directory_permissions(&path)?;
        }
    }
    set_component_directory_permissions(root)
}

#[cfg(not(unix))]
fn set_component_tree_directory_permissions(_root: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn sync_desktop_compatibility_links(
    install_root: &Path,
    component_root: Option<&Path>,
) -> Result<()> {
    use std::os::unix::fs::symlink;

    const LINKS: [&str; 2] = ["usr", "uos-runtime"];
    for name in LINKS {
        let link = install_root.join(name);
        match fs::symlink_metadata(&link) {
            Ok(metadata) if !metadata.file_type().is_symlink() => {
                return Err(InstallError::Verification(format!(
                    "Desktop 兼容路径不是受管软链接：{}",
                    link.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(InstallError::Io(error)),
        }
        if let Some(root) = component_root {
            let source = root.join(name);
            if !source.is_dir() {
                return Err(InstallError::Verification(format!(
                    "Desktop 组件缺少兼容目录：{}",
                    source.display()
                )));
            }
        }
    }

    for name in LINKS {
        let link = install_root.join(name);
        if let Some(root) = component_root {
            let temporary = install_root.join(format!(
                ".desktop-compat-{name}-{}",
                Uuid::new_v4().simple()
            ));
            symlink(root.join(name), &temporary)?;
            if link.symlink_metadata().is_ok() {
                fs::remove_file(&link)?;
            }
            if let Err(error) = fs::rename(&temporary, &link) {
                let _ = fs::remove_file(&temporary);
                return Err(InstallError::Io(error));
            }
        } else if link.symlink_metadata().is_ok() {
            fs::remove_file(link)?;
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_desktop_compatibility_links(
    _install_root: &Path,
    _component_root: Option<&Path>,
) -> Result<()> {
    Ok(())
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn manifest() -> ComponentManifest {
        ComponentManifest {
            schema: COMPONENT_SCHEMA,
            id: "org.drpa.test".to_owned(),
            version: "1.0.0".to_owned(),
            platform: current_platform().to_owned(),
            display_name: "测试组件".to_owned(),
            description: String::new(),
            provides: vec!["test.capability".to_owned()],
            requires: BTreeMap::new(),
            entrypoints: BTreeMap::from([("main".to_owned(), "bin/tool.txt".to_owned())]),
            files: Vec::new(),
        }
    }

    #[test]
    fn installs_verifies_and_removes_component_without_registry() {
        let temporary = TempDir::new().unwrap();
        let source = temporary.path().join("source");
        fs::create_dir_all(source.join("bin")).unwrap();
        fs::write(source.join("bin/tool.txt"), b"component payload").unwrap();
        let pack = temporary.path().join("test.drpac");
        build_component_pack(&source, &pack, manifest()).unwrap();
        assert_eq!(verify_component_pack(&pack).unwrap().id, "org.drpa.test");
        let layout = InstallLayout::initialize(temporary.path().join("install"), "stable").unwrap();
        layout.install_component(&pack).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let component_root = layout.component_root("org.drpa.test", "1.0.0").unwrap();
            assert_eq!(
                fs::metadata(&component_root).unwrap().permissions().mode() & 0o777,
                0o755
            );
            assert_eq!(
                fs::metadata(component_root.join("bin"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o755
            );
            assert_eq!(
                fs::metadata(component_root.join("bin/tool.txt"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o644
            );
            assert_eq!(
                fs::metadata(layout.state_path())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o644
            );
        }
        assert_eq!(
            layout.verify_component("org.drpa.test").unwrap().version,
            "1.0.0"
        );
        assert_eq!(
            fs::read_to_string(
                layout
                    .resolve_entrypoint("org.drpa.test", "main")
                    .unwrap()
                    .unwrap()
            )
            .unwrap(),
            "component payload"
        );
        layout.remove_component("org.drpa.test", true).unwrap();
        assert!(layout.active_component("org.drpa.test").unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn desktop_activation_maintains_versioned_legacy_runtime_links() {
        let temporary = TempDir::new().unwrap();
        let source = temporary.path().join("desktop-source");
        fs::create_dir_all(source.join("usr/bin")).unwrap();
        fs::create_dir_all(source.join("uos-runtime")).unwrap();
        fs::write(source.join("AppRun"), b"desktop").unwrap();
        fs::write(source.join("usr/bin/drpa-desktop"), b"desktop-bin").unwrap();
        fs::write(source.join("uos-runtime/ld-linux-x86-64.so.2"), b"loader").unwrap();
        let mut desktop = manifest();
        desktop.id = DESKTOP_COMPONENT_ID.to_owned();
        desktop.provides = vec!["desktop.ui".to_owned()];
        desktop.entrypoints = BTreeMap::from([("desktop".to_owned(), "AppRun".to_owned())]);
        let first_pack = temporary.path().join("desktop-v1.drpac");
        build_component_pack(&source, &first_pack, desktop.clone()).unwrap();
        let layout = InstallLayout::initialize(temporary.path().join("install"), "stable").unwrap();
        layout.install_component(&first_pack).unwrap();
        assert_eq!(
            fs::read_link(layout.root().join("usr")).unwrap(),
            layout
                .component_root(DESKTOP_COMPONENT_ID, "1.0.0")
                .unwrap()
                .join("usr")
        );

        desktop.version = "2.0.0".to_owned();
        let second_pack = temporary.path().join("desktop-v2.drpac");
        build_component_pack(&source, &second_pack, desktop).unwrap();
        layout.install_component(&second_pack).unwrap();
        assert_eq!(
            fs::read_link(layout.root().join("uos-runtime")).unwrap(),
            layout
                .component_root(DESKTOP_COMPONENT_ID, "2.0.0")
                .unwrap()
                .join("uos-runtime")
        );

        layout.remove_component(DESKTOP_COMPONENT_ID, true).unwrap();
        assert!(fs::symlink_metadata(layout.root().join("usr")).is_err());
        assert!(fs::symlink_metadata(layout.root().join("uos-runtime")).is_err());
    }

    #[test]
    fn rejects_traversal_in_component_manifest() {
        let mut manifest = manifest();
        manifest.files.push(ComponentFile {
            path: "../escape".to_owned(),
            bytes: 0,
            sha256: "0".repeat(64),
            executable: false,
        });
        assert!(validate_component_manifest(&manifest).is_err());
    }

    #[test]
    fn accepts_large_manifests_for_high_file_count_runtime_components() {
        let temporary = TempDir::new().unwrap();
        let pack = temporary.path().join("large-manifest.drpac");
        let file = File::create(&pack).unwrap();
        let mut writer = ZipWriter::new(file);
        writer
            .start_file(
                COMPONENT_MANIFEST,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
        let value = serde_json::json!({
            "schema": COMPONENT_SCHEMA,
            "id": "org.drpa.large-runtime",
            "version": "1.0.0",
            "platform": current_platform(),
            "displayName": "大文件清单运行时",
            "provides": [],
            "requires": {},
            "entrypoints": {},
            "files": [],
            "padding": "x".repeat(3 * 1024 * 1024),
        });
        writer
            .write_all(&serde_json::to_vec(&value).unwrap())
            .unwrap();
        writer.finish().unwrap();

        assert_eq!(
            verify_component_pack(pack).unwrap().id,
            "org.drpa.large-runtime"
        );
    }

    #[test]
    fn supports_rollback_bounded_history_and_desired_state_removal() {
        let temporary = TempDir::new().unwrap();
        let source = temporary.path().join("source");
        fs::create_dir_all(source.join("bin")).unwrap();
        fs::write(source.join("bin/tool.txt"), b"v1").unwrap();
        let first_pack = temporary.path().join("first.drpac");
        build_component_pack(&source, &first_pack, manifest()).unwrap();
        let layout = InstallLayout::initialize(temporary.path().join("install"), "stable").unwrap();
        layout.install_component(first_pack).unwrap();

        fs::write(source.join("bin/tool.txt"), b"v2").unwrap();
        let mut second = manifest();
        second.version = "2.0.0".to_owned();
        let second_pack = temporary.path().join("second.drpac");
        build_component_pack(&source, &second_pack, second).unwrap();
        layout.install_component(second_pack).unwrap();
        layout.activate_component("org.drpa.test", "1.0.0").unwrap();
        assert_eq!(
            layout
                .active_component("org.drpa.test")
                .unwrap()
                .unwrap()
                .0
                .version,
            "1.0.0"
        );
        assert_eq!(
            layout.garbage_collect_versions("org.drpa.test", 1).unwrap(),
            vec!["2.0.0"]
        );

        let managed = BTreeSet::from(["org.drpa.test".to_owned()]);
        assert_eq!(
            layout
                .reconcile_components(&BTreeSet::new(), &managed)
                .unwrap(),
            vec!["org.drpa.test"]
        );
        assert!(!layout.root().join("components/org.drpa.test").exists());
    }

    #[test]
    fn core_reconcile_deletes_only_previously_managed_obsolete_files() {
        let temporary = TempDir::new().unwrap();
        let layout = InstallLayout::initialize(temporary.path().join("install"), "stable").unwrap();
        fs::create_dir_all(layout.root().join("bin")).unwrap();
        fs::write(layout.root().join("bin/old.exe"), b"old").unwrap();
        layout
            .reconcile_core_files(&BTreeSet::from(["bin/old.exe".to_owned()]))
            .unwrap();
        fs::write(layout.root().join("new.exe"), b"new").unwrap();
        fs::write(layout.root().join("user-note.txt"), b"keep").unwrap();
        let removed = layout
            .reconcile_core_files(&BTreeSet::from(["new.exe".to_owned()]))
            .unwrap();
        assert_eq!(removed, vec!["bin/old.exe"]);
        assert!(!layout.root().join("bin/old.exe").exists());
        assert!(layout.root().join("user-note.txt").is_file());
        assert!(
            layout
                .reconcile_core_files(&BTreeSet::from(["data/do-not-touch".to_owned()]))
                .is_err()
        );
    }

    #[test]
    fn enforces_component_capability_dependencies() {
        let temporary = TempDir::new().unwrap();
        let source = temporary.path().join("source");
        fs::create_dir_all(source.join("bin")).unwrap();
        fs::write(source.join("bin/tool.txt"), b"payload").unwrap();
        let layout = InstallLayout::initialize(temporary.path().join("install"), "stable").unwrap();

        let provider_pack = temporary.path().join("provider.drpac");
        build_component_pack(&source, &provider_pack, manifest()).unwrap();
        layout.install_component(provider_pack).unwrap();

        let mut dependent = manifest();
        dependent.id = "org.drpa.dependent".to_owned();
        dependent.provides.clear();
        dependent.requires = BTreeMap::from([("test.capability".to_owned(), "^1".to_owned())]);
        let dependent_pack = temporary.path().join("dependent.drpac");
        build_component_pack(&source, &dependent_pack, dependent).unwrap();
        layout.install_component(dependent_pack).unwrap();

        assert!(layout.remove_component("org.drpa.test", true).is_err());
        layout.remove_component("org.drpa.dependent", true).unwrap();
        layout.remove_component("org.drpa.test", true).unwrap();
    }

    #[test]
    fn leases_protect_running_component_versions_until_last_clone_drops() {
        let temporary = TempDir::new().unwrap();
        let source = temporary.path().join("source");
        fs::create_dir_all(source.join("bin")).unwrap();
        fs::write(source.join("bin/tool.txt"), b"payload").unwrap();
        let pack = temporary.path().join("provider.drpac");
        build_component_pack(&source, &pack, manifest()).unwrap();
        let layout = InstallLayout::initialize(temporary.path().join("install"), "stable").unwrap();
        layout.install_component(pack).unwrap();

        let lease = layout
            .acquire_component_lease("org.drpa.test", "1.0.0", "unit-test")
            .unwrap();
        let lease_clone = lease.clone();
        assert_eq!(
            layout
                .active_component_leases("org.drpa.test", Some("1.0.0"))
                .unwrap()
                .len(),
            1
        );
        assert!(matches!(
            layout.remove_component("org.drpa.test", true),
            Err(InstallError::ComponentInUse(_))
        ));
        drop(lease);
        assert!(layout.remove_component("org.drpa.test", true).is_err());
        drop(lease_clone);
        layout.remove_component("org.drpa.test", true).unwrap();
    }

    #[test]
    fn lists_all_active_capability_providers() {
        let temporary = TempDir::new().unwrap();
        let source = temporary.path().join("source");
        fs::create_dir_all(source.join("bin")).unwrap();
        fs::write(source.join("bin/tool.txt"), b"payload").unwrap();
        let layout = InstallLayout::initialize(temporary.path().join("install"), "stable").unwrap();
        for (id, version) in [
            ("org.drpa.runtime.full", "3.11"),
            ("org.drpa.runtime.min", "3.14"),
        ] {
            let mut runtime = manifest();
            runtime.id = id.to_owned();
            runtime.version = version.to_owned();
            runtime.provides = vec!["runtime.python".to_owned()];
            let pack = temporary.path().join(format!("{id}.drpac"));
            build_component_pack(&source, &pack, runtime).unwrap();
            layout.install_component(pack).unwrap();
        }

        let providers = layout.providers_for("runtime.python").unwrap();
        assert_eq!(providers.len(), 2);
        assert_eq!(providers[0].0, "org.drpa.runtime.full");
        assert_eq!(providers[1].0, "org.drpa.runtime.min");
    }
}
