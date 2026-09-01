use serde::Serialize;
use serde_json::Value;

use crate::agent::AgentToolPolicy;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ToolOrigin {
    Builtin,
    Host,
    Skill,
    Plugin,
    Extension,
    Jcode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ToolSensitivity {
    Standard,
    FilesystemRead,
    FilesystemWrite,
    ProcessExecution,
    BrowserControl,
    CredentialRead,
    CredentialWrite,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolCapabilityDescriptor {
    pub(crate) name: String,
    pub(crate) origin: ToolOrigin,
    pub(crate) sensitivity: ToolSensitivity,
    pub(crate) parameters: Value,
}

impl ToolCapabilityDescriptor {
    pub(crate) fn from_openai_definition(definition: &Value) -> Result<Self, String> {
        let name = definition
            .pointer("/function/name")
            .and_then(Value::as_str)
            .ok_or_else(|| "工具定义缺少 function.name".to_owned())?
            .to_owned();
        let parameters = definition
            .pointer("/function/parameters")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({"type":"object"}));
        Ok(Self {
            origin: origin_for(&name),
            sensitivity: sensitivity_for(&name),
            name,
            parameters,
        })
    }

    pub(crate) fn validate_arguments(&self, arguments: &Value) -> Result<(), String> {
        validate_schema(&self.parameters, arguments, "$arguments")
            .map_err(|error| format!("工具 {} 参数校验失败：{error}", self.name))
    }

    pub(crate) fn redact_persistent_output(&self) -> bool {
        matches!(self.sensitivity, ToolSensitivity::CredentialRead) || self.name == "document_read"
    }
}

pub(crate) struct CapabilityAuthority<'a> {
    policy: &'a AgentToolPolicy,
}

impl<'a> CapabilityAuthority<'a> {
    pub(crate) fn new(policy: &'a AgentToolPolicy) -> Self {
        Self { policy }
    }

    pub(crate) fn authorize(&self, descriptor: &ToolCapabilityDescriptor) -> Result<(), String> {
        self.authorize_name(&descriptor.name)
    }

    pub(crate) fn authorize_name(&self, name: &str) -> Result<(), String> {
        if !self.policy.enabled {
            return Err("AI Agent 工具已在设置中关闭".to_owned());
        }
        let allowed = match name {
            "agent_list_skills"
            | "agent_read_skill"
            | "agent_read_memory"
            | "knowledge_list_documents"
            | "knowledge_read_document"
            | "rpaz_list_files" => true,
            "browser_open" | "browser_snapshot" | "browser_click" | "browser_type"
            | "browser_wait" | "browser_screenshot" | "browser_status" => self.policy.browser,
            "rpaz_list_packages" | "rpaz_run_package" => self.policy.rpaz_runs,
            "run_list" | "run_get_detail" => self.policy.run_records,
            "vault_list_credentials" | "vault_get_credential" => self.policy.vault_read,
            "vault_upsert_credential" => self.policy.vault_write,
            "agent_write_skill"
            | "agent_write_memory"
            | "agent_remember"
            | "knowledge_write_document" => self.policy.workspace_write,
            "data_list_connections" | "data_get_schema" | "data_query" => self.policy.database_read,
            "data_create_connection" => self.policy.database_connections,
            "knowledge_base_list" | "knowledge_base_search" => self.policy.knowledge_base_read,
            "document_read" => self.policy.document_read,
            "document_create" => self.policy.document_write,
            "document_convert" => self.policy.document_convert,
            "read_file" | "find_files" | "search_text" => self.policy.arbitrary_file_read,
            "edit_file" | "rpaz_write_file" | "rpaz_validate" | "rpaz_build" => {
                self.policy.project_write
            }
            "rpaz_python" => self.policy.python,
            _ => self.policy.extensions,
        };
        if allowed {
            Ok(())
        } else {
            Err(format!("工具 {name} 已在能力策略中关闭"))
        }
    }
}

fn origin_for(name: &str) -> ToolOrigin {
    if name.starts_with("skill_") {
        ToolOrigin::Skill
    } else if name.starts_with("plugin_") {
        ToolOrigin::Plugin
    } else if name.starts_with("ext__") {
        ToolOrigin::Extension
    } else if name.starts_with("jcode:") {
        ToolOrigin::Jcode
    } else if matches!(
        name,
        "rpaz_list_packages"
            | "rpaz_run_package"
            | "run_list"
            | "run_get_detail"
            | "vault_list_credentials"
            | "vault_get_credential"
            | "vault_upsert_credential"
    ) {
        ToolOrigin::Host
    } else {
        ToolOrigin::Builtin
    }
}

fn sensitivity_for(name: &str) -> ToolSensitivity {
    match name {
        "read_file" | "find_files" | "search_text" | "document_read" => {
            ToolSensitivity::FilesystemRead
        }
        "edit_file"
        | "rpaz_write_file"
        | "rpaz_build"
        | "document_create"
        | "document_convert"
        | "knowledge_write_document"
        | "agent_write_skill"
        | "agent_write_memory" => ToolSensitivity::FilesystemWrite,
        "agent_remember" => ToolSensitivity::FilesystemWrite,
        "rpaz_python" | "rpaz_run_package" => ToolSensitivity::ProcessExecution,
        name if name.starts_with("browser_") => ToolSensitivity::BrowserControl,
        "vault_list_credentials" | "vault_get_credential" => ToolSensitivity::CredentialRead,
        "vault_upsert_credential" => ToolSensitivity::CredentialWrite,
        _ => ToolSensitivity::Standard,
    }
}

fn validate_schema(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    if let Some(expected) = schema.get("type").and_then(Value::as_str) {
        let matches = match expected {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            _ => true,
        };
        if !matches {
            return Err(format!("{path} 应为 {expected}"));
        }
    }
    if let Some(options) = schema.get("enum").and_then(Value::as_array)
        && !options.iter().any(|option| option == value)
    {
        return Err(format!("{path} 不在允许的枚举值中"));
    }
    if let Some(object) = value.as_object() {
        let properties = schema.get("properties").and_then(Value::as_object);
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for key in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(key) {
                    return Err(format!("{path}.{key} 为必填项"));
                }
            }
        }
        if schema.get("additionalProperties").and_then(Value::as_bool) == Some(false)
            && let Some(properties) = properties
            && let Some(key) = object.keys().find(|key| !properties.contains_key(*key))
        {
            return Err(format!("{path}.{key} 是未声明参数"));
        }
        if let Some(properties) = properties {
            for (key, child) in object {
                if let Some(child_schema) = properties.get(key) {
                    validate_schema(child_schema, child, &format!("{path}.{key}"))?;
                }
            }
        }
    }
    if let Some(array) = value.as_array()
        && let Some(item_schema) = schema.get("items")
    {
        for (index, item) in array.iter().enumerate() {
            validate_schema(item_schema, item, &format!("{path}[{index}]"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn schema_validation_rejects_unknown_and_missing_arguments() {
        let descriptor = ToolCapabilityDescriptor::from_openai_definition(&json!({
            "type":"function",
            "function":{
                "name":"read_file",
                "parameters":{
                    "type":"object",
                    "properties":{"path":{"type":"string"}},
                    "required":["path"],
                    "additionalProperties":false
                }
            }
        }))
        .unwrap();
        assert!(descriptor.validate_arguments(&json!({})).is_err());
        assert!(
            descriptor
                .validate_arguments(&json!({"path":"a","extra":1}))
                .is_err()
        );
        assert!(descriptor.validate_arguments(&json!({"path":"a"})).is_ok());
    }

    #[test]
    fn authority_separates_vault_read_and_write() {
        let policy = AgentToolPolicy {
            vault_read: true,
            vault_write: false,
            ..AgentToolPolicy::default()
        };
        let authority = CapabilityAuthority::new(&policy);
        assert!(authority.authorize_name("vault_get_credential").is_ok());
        assert!(authority.authorize_name("vault_upsert_credential").is_err());
    }
}
