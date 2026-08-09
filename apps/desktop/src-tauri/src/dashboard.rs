use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::AppPaths;

const DASHBOARD_SCHEMA: u32 = 1;
const MAX_DASHBOARDS: usize = 24;
const MAX_WIDGETS_PER_DASHBOARD: usize = 120;
const MAX_SQL_BYTES: usize = 64 * 1024;
const MAX_TEXT_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DashboardDocument {
    schema: u32,
    active_dashboard_id: String,
    dashboards: Vec<DashboardDefinition>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardDefinition {
    id: String,
    title: String,
    description: String,
    columns: u8,
    row_height: u16,
    widgets: Vec<DashboardWidget>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardWidget {
    id: String,
    title: String,
    kind: String,
    layout: DashboardLayout,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<DashboardSource>,
    #[serde(default)]
    encoding: DashboardEncoding,
    #[serde(default)]
    options: DashboardOptions,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardLayout {
    x: u8,
    y: u16,
    w: u8,
    h: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardSource {
    kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dataset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sql: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardEncoding {
    #[serde(default)]
    category_field: String,
    #[serde(default)]
    value_field: String,
    #[serde(default)]
    series_field: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardOptions {
    #[serde(default)]
    text: String,
    #[serde(default = "default_number_format")]
    number_format: String,
    #[serde(default = "default_color")]
    color: String,
    #[serde(default = "default_true")]
    show_legend: bool,
    #[serde(default)]
    refresh_seconds: u32,
}

impl Default for DashboardOptions {
    fn default() -> Self {
        Self {
            text: String::new(),
            number_format: default_number_format(),
            color: default_color(),
            show_legend: true,
            refresh_seconds: 0,
        }
    }
}

fn default_number_format() -> String {
    "number".to_owned()
}

fn default_color() -> String {
    "#4f6bed".to_owned()
}

fn default_true() -> bool {
    true
}

#[tauri::command(async)]
pub(crate) fn get_bi_dashboard(paths: State<'_, AppPaths>) -> Result<DashboardDocument, String> {
    load_document(&paths.workspace_root)
}

#[tauri::command(async)]
pub(crate) fn save_bi_dashboard(
    document: DashboardDocument,
    paths: State<'_, AppPaths>,
) -> Result<DashboardDocument, String> {
    save_document(&paths.workspace_root, document)
}

#[tauri::command(async)]
pub(crate) fn reset_bi_dashboard(paths: State<'_, AppPaths>) -> Result<DashboardDocument, String> {
    save_document(&paths.workspace_root, default_document())
}

fn dashboard_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join("dashboard").join("home.json")
}

fn load_document(workspace_root: &Path) -> Result<DashboardDocument, String> {
    let path = dashboard_path(workspace_root);
    if !path.exists() {
        return save_document(workspace_root, default_document());
    }
    let source =
        fs::read_to_string(&path).map_err(|error| format!("读取 BI 仪表盘失败：{error}"))?;
    let document = serde_json::from_str::<DashboardDocument>(&source)
        .map_err(|error| format!("BI 仪表盘配置格式无效：{error}"))?;
    validate_document(&document)?;
    Ok(document)
}

fn save_document(
    workspace_root: &Path,
    mut document: DashboardDocument,
) -> Result<DashboardDocument, String> {
    document.schema = DASHBOARD_SCHEMA;
    validate_document(&document)?;
    let path = dashboard_path(workspace_root);
    let parent = path
        .parent()
        .ok_or_else(|| "BI 仪表盘目录无效".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| format!("创建 BI 仪表盘目录失败：{error}"))?;
    let source = serde_json::to_vec_pretty(&document)
        .map_err(|error| format!("序列化 BI 仪表盘失败：{error}"))?;
    let temporary = parent.join(format!(".home-{}.tmp", Uuid::new_v4().simple()));
    let backup = parent.join(format!(".home-{}.bak", Uuid::new_v4().simple()));
    fs::write(&temporary, [&source[..], b"\n"].concat())
        .map_err(|error| format!("写入 BI 仪表盘临时文件失败：{error}"))?;
    let had_existing = path.exists();
    if had_existing {
        fs::rename(&path, &backup).map_err(|error| {
            let _ = fs::remove_file(&temporary);
            format!("备份 BI 仪表盘失败：{error}")
        })?;
    }
    if let Err(error) = fs::rename(&temporary, &path) {
        if had_existing {
            let _ = fs::rename(&backup, &path);
        }
        let _ = fs::remove_file(&temporary);
        return Err(format!("提交 BI 仪表盘失败：{error}"));
    }
    if had_existing {
        let _ = fs::remove_file(backup);
    }
    Ok(document)
}

fn validate_document(document: &DashboardDocument) -> Result<(), String> {
    if document.schema != DASHBOARD_SCHEMA {
        return Err(format!("不支持的 BI 仪表盘版本：{}", document.schema));
    }
    if document.dashboards.is_empty() || document.dashboards.len() > MAX_DASHBOARDS {
        return Err(format!("BI 仪表盘数量应为 1 到 {MAX_DASHBOARDS} 个"));
    }
    if !document
        .dashboards
        .iter()
        .any(|dashboard| dashboard.id == document.active_dashboard_id)
    {
        return Err("当前 BI 仪表盘不存在".to_owned());
    }
    let mut dashboard_ids = std::collections::HashSet::new();
    for dashboard in &document.dashboards {
        validate_id(&dashboard.id, "仪表盘")?;
        if !dashboard_ids.insert(&dashboard.id) {
            return Err("BI 仪表盘标识重复".to_owned());
        }
        validate_text(&dashboard.title, 1, 80, "仪表盘名称")?;
        validate_text(&dashboard.description, 0, 300, "仪表盘描述")?;
        if !(4..=24).contains(&dashboard.columns) {
            return Err("BI 仪表盘列数应为 4 到 24".to_owned());
        }
        if !(36..=160).contains(&dashboard.row_height) {
            return Err("BI 仪表盘行高应为 36 到 160".to_owned());
        }
        if dashboard.widgets.len() > MAX_WIDGETS_PER_DASHBOARD {
            return Err(format!(
                "单个仪表盘最多包含 {MAX_WIDGETS_PER_DASHBOARD} 个组件"
            ));
        }
        let mut widget_ids = std::collections::HashSet::new();
        for widget in &dashboard.widgets {
            validate_widget(widget, dashboard.columns)?;
            if !widget_ids.insert(&widget.id) {
                return Err("BI 组件标识重复".to_owned());
            }
        }
    }
    Ok(())
}

fn validate_widget(widget: &DashboardWidget, columns: u8) -> Result<(), String> {
    validate_id(&widget.id, "组件")?;
    validate_text(&widget.title, 1, 100, "组件标题")?;
    if !matches!(
        widget.kind.as_str(),
        "metric" | "line" | "bar" | "pie" | "table" | "markdown"
    ) {
        return Err(format!("不支持的 BI 组件类型：{}", widget.kind));
    }
    if widget.layout.w == 0
        || widget.layout.h == 0
        || widget.layout.w > columns
        || widget.layout.x >= columns
        || widget.layout.x.saturating_add(widget.layout.w) > columns
        || widget.layout.h > 24
        || widget.layout.y > 4_000
    {
        return Err(format!("BI 组件 {} 的栅格位置无效", widget.id));
    }
    validate_text(&widget.encoding.category_field, 0, 120, "分类字段")?;
    validate_text(&widget.encoding.value_field, 0, 120, "数值字段")?;
    validate_text(&widget.encoding.series_field, 0, 120, "系列字段")?;
    if widget.options.text.len() > MAX_TEXT_BYTES {
        return Err("Markdown 组件内容过大".to_owned());
    }
    if widget.options.refresh_seconds > 86_400 {
        return Err("BI 组件刷新周期不能超过一天".to_owned());
    }
    if widget.kind == "markdown" {
        return Ok(());
    }
    let source = widget
        .source
        .as_ref()
        .ok_or_else(|| format!("BI 组件 {} 缺少数据源", widget.id))?;
    match source.kind.as_str() {
        "builtin" => {
            let dataset = source.dataset.as_deref().unwrap_or_default();
            if !matches!(
                dataset,
                "workspaceSummary" | "runHistory" | "runStatus" | "packages"
            ) {
                return Err(format!("不支持的 DRPA 内置数据集：{dataset}"));
            }
        }
        "database" => {
            let profile_id = source.profile_id.as_deref().unwrap_or_default();
            if profile_id != "workspace" {
                validate_id(profile_id, "数据源")?;
            }
            let sql = source.sql.as_deref().unwrap_or_default().trim();
            if sql.is_empty() || sql.len() > MAX_SQL_BYTES {
                return Err("数据库 BI 组件的 SQL 长度无效".to_owned());
            }
        }
        other => return Err(format!("不支持的 BI 数据源类型：{other}")),
    }
    Ok(())
}

fn validate_id(value: &str, label: &str) -> Result<(), String> {
    if value.len() < 2
        || value.len() > 96
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(format!("{label}标识无效"));
    }
    Ok(())
}

fn validate_text(value: &str, minimum: usize, maximum: usize, label: &str) -> Result<(), String> {
    let length = value.chars().count();
    if length < minimum || length > maximum {
        return Err(format!("{label}长度应为 {minimum} 到 {maximum} 个字符"));
    }
    Ok(())
}

fn default_document() -> DashboardDocument {
    DashboardDocument {
        schema: DASHBOARD_SCHEMA,
        active_dashboard_id: "home".to_owned(),
        dashboards: vec![DashboardDefinition {
            id: "home".to_owned(),
            title: "业务总览".to_owned(),
            description: "把 DRPA 运行数据与数据工作台查询组合成一个可编辑的 BI 主页。".to_owned(),
            columns: 12,
            row_height: 58,
            widgets: vec![
                builtin_metric(
                    "active-runs",
                    "活动任务",
                    0,
                    "activeRuns",
                    "number",
                    "#4f6bed",
                ),
                builtin_metric(
                    "success-rate",
                    "30 天成功率",
                    3,
                    "successRate",
                    "percent",
                    "#159570",
                ),
                builtin_metric(
                    "package-count",
                    "RPAZ 包",
                    6,
                    "packages",
                    "number",
                    "#8b5cf6",
                ),
                builtin_metric(
                    "saved-hours",
                    "预计节省时间",
                    9,
                    "savedHours",
                    "hours",
                    "#d97706",
                ),
                DashboardWidget {
                    id: "run-duration".to_owned(),
                    title: "最近运行耗时".to_owned(),
                    kind: "line".to_owned(),
                    layout: DashboardLayout {
                        x: 0,
                        y: 2,
                        w: 8,
                        h: 5,
                    },
                    source: Some(builtin_source("runHistory")),
                    encoding: DashboardEncoding {
                        category_field: "startedAt".to_owned(),
                        value_field: "durationSeconds".to_owned(),
                        series_field: String::new(),
                    },
                    options: DashboardOptions {
                        color: "#4f6bed".to_owned(),
                        ..Default::default()
                    },
                },
                DashboardWidget {
                    id: "run-status".to_owned(),
                    title: "运行状态分布".to_owned(),
                    kind: "pie".to_owned(),
                    layout: DashboardLayout {
                        x: 8,
                        y: 2,
                        w: 4,
                        h: 5,
                    },
                    source: Some(builtin_source("runStatus")),
                    encoding: DashboardEncoding {
                        category_field: "status".to_owned(),
                        value_field: "count".to_owned(),
                        series_field: String::new(),
                    },
                    options: DashboardOptions::default(),
                },
                DashboardWidget {
                    id: "recent-runs".to_owned(),
                    title: "最近运行".to_owned(),
                    kind: "table".to_owned(),
                    layout: DashboardLayout {
                        x: 0,
                        y: 7,
                        w: 12,
                        h: 5,
                    },
                    source: Some(builtin_source("runHistory")),
                    encoding: DashboardEncoding::default(),
                    options: DashboardOptions::default(),
                },
            ],
        }],
    }
}

fn builtin_metric(
    id: &str,
    title: &str,
    x: u8,
    value_field: &str,
    number_format: &str,
    color: &str,
) -> DashboardWidget {
    DashboardWidget {
        id: id.to_owned(),
        title: title.to_owned(),
        kind: "metric".to_owned(),
        layout: DashboardLayout {
            x,
            y: 0,
            w: 3,
            h: 2,
        },
        source: Some(builtin_source("workspaceSummary")),
        encoding: DashboardEncoding {
            category_field: String::new(),
            value_field: value_field.to_owned(),
            series_field: String::new(),
        },
        options: DashboardOptions {
            number_format: number_format.to_owned(),
            color: color.to_owned(),
            ..Default::default()
        },
    }
}

fn builtin_source(dataset: &str) -> DashboardSource {
    DashboardSource {
        kind: "builtin".to_owned(),
        dataset: Some(dataset.to_owned()),
        profile_id: None,
        sql: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_root() -> PathBuf {
        std::env::temp_dir().join(format!("drpa-dashboard-test-{}", Uuid::new_v4().simple()))
    }

    #[test]
    fn default_dashboard_is_valid_and_round_trips() {
        let root = temporary_root();
        let saved = save_document(&root, default_document()).unwrap();
        let loaded = load_document(&root).unwrap();
        assert_eq!(saved.active_dashboard_id, "home");
        assert_eq!(loaded.dashboards[0].widgets.len(), 7);
        assert!(dashboard_path(&root).is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_widget_layout_is_rejected() {
        let mut document = default_document();
        document.dashboards[0].widgets[0].layout.x = 11;
        document.dashboards[0].widgets[0].layout.w = 3;
        assert!(validate_document(&document).is_err());
    }

    #[test]
    fn database_widgets_require_a_bounded_query() {
        let mut document = default_document();
        document.dashboards[0].widgets[0].source = Some(DashboardSource {
            kind: "database".to_owned(),
            dataset: None,
            profile_id: Some("workspace".to_owned()),
            sql: Some(String::new()),
        });
        assert!(validate_document(&document).is_err());
    }
}
