use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const CURRENT_SCHEMA: u16 = 2;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("manifest could not be decoded: {0}")]
    Decode(String),
    #[error("unsupported manifest schema: {0}")]
    UnsupportedSchema(u16),
    #[error("invalid package id: {0}")]
    InvalidPackageId(String),
    #[error("invalid package version: {0}")]
    InvalidVersion(String),
    #[error("path must be a contained relative path: {0}")]
    UnsafePath(String),
    #[error("Python entrypoint callable must not be empty")]
    EmptyCallable,
    #[error("package name must not be empty")]
    EmptyName,
    #[error("invalid or duplicate parameter id: {0}")]
    InvalidParameterId(String),
    #[error("parameter label must not be empty: {0}")]
    EmptyParameterLabel(String),
    #[error("archive contains too many files: {actual} exceeds {limit}")]
    TooManyFiles { actual: u64, limit: u64 },
    #[error("archive expands beyond the allowed size: {actual} exceeds {limit} bytes")]
    ExpandedSizeExceeded { actual: u64, limit: u64 },
    #[error("archive entry has a suspicious compression ratio")]
    SuspiciousCompressionRatio,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageManifest {
    pub schema: u16,
    pub id: String,
    pub name: String,
    pub version: String,
    pub entrypoint: Entrypoint,
    pub runtime: RuntimeRequirement,
    #[serde(default)]
    pub capabilities: Capabilities,
    #[serde(default)]
    pub parameters: Vec<Parameter>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "runtime", rename_all = "lowercase")]
pub enum Entrypoint {
    Python {
        module: String,
        callable: String,
    },
    Command {
        executable: String,
        #[serde(default)]
        args: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeRequirement {
    #[serde(default)]
    pub python: Option<String>,
    #[serde(default)]
    pub lock: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Capabilities {
    #[serde(default)]
    pub network: NetworkCapability,
    #[serde(default)]
    pub filesystem: FilesystemCapability,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkCapability {
    #[serde(default)]
    pub allow: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FilesystemCapability {
    #[serde(default)]
    pub read: Vec<String>,
    #[serde(default)]
    pub write: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Parameter {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub kind: ParameterKind,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParameterKind {
    String,
    Number,
    Boolean,
    Secret,
    File,
    Directory,
}

impl PackageManifest {
    pub fn from_yaml(source: &str) -> Result<Self, ManifestError> {
        let manifest: Self = serde_yaml::from_str(source)
            .map_err(|error| ManifestError::Decode(error.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.schema != CURRENT_SCHEMA {
            return Err(ManifestError::UnsupportedSchema(self.schema));
        }
        validate_package_id(&self.id)?;
        validate_version(&self.version)?;
        if self.name.trim().is_empty() {
            return Err(ManifestError::EmptyName);
        }

        match &self.entrypoint {
            Entrypoint::Python { module, callable } => {
                safe_relative_path(module)?;
                if callable.trim().is_empty() {
                    return Err(ManifestError::EmptyCallable);
                }
            }
            Entrypoint::Command { executable, .. } => {
                safe_relative_path(executable)?;
            }
        }

        if let Some(lock) = &self.runtime.lock {
            safe_relative_path(lock)?;
        }

        let mut parameter_ids = std::collections::HashSet::new();
        for parameter in &self.parameters {
            if !is_identifier(&parameter.id) || !parameter_ids.insert(parameter.id.as_str()) {
                return Err(ManifestError::InvalidParameterId(parameter.id.clone()));
            }
            if parameter
                .label
                .as_deref()
                .is_some_and(|label| label.trim().is_empty())
            {
                return Err(ManifestError::EmptyParameterLabel(parameter.id.clone()));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveLimits {
    pub max_files: u64,
    pub max_expanded_bytes: u64,
    pub max_compression_ratio: u64,
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self {
            max_files: 4_096,
            max_expanded_bytes: 512 * 1024 * 1024,
            max_compression_ratio: 100,
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ArchiveBudget {
    files: u64,
    expanded_bytes: u64,
}

impl ArchiveBudget {
    pub fn observe(
        &mut self,
        compressed_bytes: u64,
        expanded_bytes: u64,
        limits: ArchiveLimits,
    ) -> Result<(), ManifestError> {
        self.files = self.files.saturating_add(1);
        self.expanded_bytes = self.expanded_bytes.saturating_add(expanded_bytes);

        if self.files > limits.max_files {
            return Err(ManifestError::TooManyFiles {
                actual: self.files,
                limit: limits.max_files,
            });
        }
        if self.expanded_bytes > limits.max_expanded_bytes {
            return Err(ManifestError::ExpandedSizeExceeded {
                actual: self.expanded_bytes,
                limit: limits.max_expanded_bytes,
            });
        }
        if expanded_bytes > 0
            && (compressed_bytes == 0
                || expanded_bytes / compressed_bytes.max(1) > limits.max_compression_ratio)
        {
            return Err(ManifestError::SuspiciousCompressionRatio);
        }
        Ok(())
    }
}

pub fn validate_package_id(value: &str) -> Result<(), ManifestError> {
    let valid = value.len() <= 128
        && value.contains('.')
        && value.split('.').all(|part| {
            !part.is_empty()
                && part.len() <= 63
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
                && !part.starts_with('-')
                && !part.ends_with('-')
        });
    if valid {
        Ok(())
    } else {
        Err(ManifestError::InvalidPackageId(value.to_owned()))
    }
}

pub fn safe_relative_path(value: &str) -> Result<PathBuf, ManifestError> {
    let path = Path::new(value);
    let valid = !value.trim().is_empty()
        && !value.contains('\\')
        && !value.contains('\0')
        && !value.contains(':')
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
    if valid {
        Ok(path.to_path_buf())
    } else {
        Err(ManifestError::UnsafePath(value.to_owned()))
    }
}

fn is_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn validate_version(value: &str) -> Result<(), ManifestError> {
    let core = value.split_once('-').map_or(value, |(core, _)| core);
    let valid = core.split('.').count() == 3
        && core
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    if valid {
        Ok(())
    } else {
        Err(ManifestError::InvalidVersion(value.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
schema: 2
id: com.example.invoice-downloader
name: Invoice Downloader
version: 2.1.0
entrypoint:
  runtime: python
  module: main.py
  callable: main
runtime:
  python: "3.11.*"
  lock: requirements.lock
parameters:
  - id: password
    label: 登录密码
    description: 用于登录目标系统
    type: secret
    required: true
"#;

    #[test]
    fn parses_and_validates_v2_manifest() {
        let manifest = PackageManifest::from_yaml(VALID).unwrap();
        assert_eq!(manifest.id, "com.example.invoice-downloader");
        assert_eq!(manifest.parameters[0].kind, ParameterKind::Secret);
        assert_eq!(manifest.parameters[0].label.as_deref(), Some("登录密码"));
    }

    #[test]
    fn rejects_parent_directory_entrypoint() {
        let source = VALID.replace("main.py", "../main.py");
        let error = PackageManifest::from_yaml(&source).unwrap_err();
        assert!(matches!(error, ManifestError::UnsafePath(_)));
    }

    #[test]
    fn rejects_ambiguous_package_id() {
        let source = VALID.replace("com.example.invoice-downloader", "../../escape");
        let error = PackageManifest::from_yaml(&source).unwrap_err();
        assert!(matches!(error, ManifestError::InvalidPackageId(_)));
    }

    #[test]
    fn rejects_duplicate_parameter_ids() {
        let source = format!("{VALID}\n  - id: password\n    label: 备用密码\n    type: string\n");
        let error = PackageManifest::from_yaml(&source).unwrap_err();
        assert!(matches!(error, ManifestError::InvalidParameterId(_)));
    }

    #[test]
    fn rejects_empty_parameter_labels() {
        let source = VALID.replace("label: 登录密码", "label: '   '");
        let error = PackageManifest::from_yaml(&source).unwrap_err();
        assert!(matches!(error, ManifestError::EmptyParameterLabel(_)));
    }

    #[test]
    fn archive_budget_rejects_zip_bomb_ratio() {
        let limits = ArchiveLimits::default();
        let error = ArchiveBudget::default()
            .observe(1, 1_000_000, limits)
            .unwrap_err();
        assert_eq!(error, ManifestError::SuspiciousCompressionRatio);
    }
}
