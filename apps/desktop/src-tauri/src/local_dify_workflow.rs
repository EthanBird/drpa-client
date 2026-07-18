use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use uuid::Uuid;

pub(crate) const WORKFLOW_SCHEMA: u32 = 1;
const MAX_WORKFLOW_NODES: usize = 256;
const MAX_WORKFLOW_EDGES: usize = 1_024;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkflowViewport {
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default = "default_zoom")]
    pub zoom: f64,
}

impl Default for WorkflowViewport {
    fn default() -> Self {
        Self {
            x: 80.0,
            y: 120.0,
            zoom: 1.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkflowNode {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub x: f64,
    pub y: f64,
    #[serde(default = "default_node_width")]
    pub width: f64,
    #[serde(default = "default_node_height")]
    pub height: f64,
    #[serde(default)]
    pub config: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkflowEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub source_handle: String,
    #[serde(default)]
    pub target_handle: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub data: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkflowGraph {
    #[serde(default = "default_schema")]
    pub schema: u32,
    #[serde(default)]
    pub viewport: WorkflowViewport,
    #[serde(default)]
    pub nodes: Vec<WorkflowNode>,
    #[serde(default)]
    pub edges: Vec<WorkflowEdge>,
}

impl Default for WorkflowGraph {
    fn default() -> Self {
        Self {
            schema: WORKFLOW_SCHEMA,
            viewport: WorkflowViewport::default(),
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkflowValidationIssue {
    pub level: String,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkflowValidationReport {
    pub valid: bool,
    pub node_count: usize,
    pub edge_count: usize,
    pub issues: Vec<WorkflowValidationIssue>,
}

const fn default_schema() -> u32 {
    WORKFLOW_SCHEMA
}

const fn default_zoom() -> f64 {
    1.0
}

const fn default_node_width() -> f64 {
    220.0
}

const fn default_node_height() -> f64 {
    84.0
}

pub(crate) fn is_workflow_mode(mode: &str) -> bool {
    matches!(mode, "workflow" | "advanced-chat")
}

pub(crate) fn default_graph(mode: &str, input_key: &str) -> WorkflowGraph {
    if !is_workflow_mode(mode) {
        return WorkflowGraph::default();
    }
    let start_id = "start".to_owned();
    let llm_id = "llm".to_owned();
    let terminal_kind = if mode == "advanced-chat" {
        "answer"
    } else {
        "end"
    };
    let terminal_id = terminal_kind.to_owned();
    let mut start_config = BTreeMap::new();
    start_config.insert(
        "variables".to_owned(),
        json!([{
            "label": input_key,
            "variable": input_key,
            "type": "paragraph",
            "required": true,
            "max_length": 1_000_000
        }]),
    );
    let mut llm_config = BTreeMap::new();
    llm_config.insert(
        "prompt_template".to_owned(),
        json!([
            {"role": "system", "text": "你是一个准确、简洁的 AI 助手。"},
            {"role": "user", "text": format!("{{{{#start.{input_key}#}}}}")}
        ]),
    );
    llm_config.insert(
        "model".to_owned(),
        json!({
            "provider": "",
            "name": "",
            "mode": "chat",
            "completion_params": {"temperature": 0.2}
        }),
    );
    let mut terminal_config = BTreeMap::new();
    if terminal_kind == "answer" {
        terminal_config.insert("answer".to_owned(), json!("{{#llm.text#}}"));
    } else {
        terminal_config.insert(
            "outputs".to_owned(),
            json!([{"variable": "answer", "value_selector": ["llm", "text"]}]),
        );
    }
    WorkflowGraph {
        schema: WORKFLOW_SCHEMA,
        viewport: WorkflowViewport::default(),
        nodes: vec![
            WorkflowNode {
                id: start_id.clone(),
                kind: "start".to_owned(),
                title: "开始".to_owned(),
                x: 80.0,
                y: 210.0,
                width: default_node_width(),
                height: default_node_height(),
                config: start_config,
            },
            WorkflowNode {
                id: llm_id.clone(),
                kind: "llm".to_owned(),
                title: "LLM".to_owned(),
                x: 390.0,
                y: 210.0,
                width: default_node_width(),
                height: 104.0,
                config: llm_config,
            },
            WorkflowNode {
                id: terminal_id.clone(),
                kind: terminal_kind.to_owned(),
                title: if terminal_kind == "answer" {
                    "直接回复".to_owned()
                } else {
                    "结束".to_owned()
                },
                x: 700.0,
                y: 210.0,
                width: default_node_width(),
                height: default_node_height(),
                config: terminal_config,
            },
        ],
        edges: vec![
            WorkflowEdge {
                id: "edge-start-llm".to_owned(),
                source: start_id,
                target: llm_id.clone(),
                source_handle: "source".to_owned(),
                target_handle: "target".to_owned(),
                label: String::new(),
                data: BTreeMap::new(),
            },
            WorkflowEdge {
                id: format!("edge-llm-{terminal_id}"),
                source: llm_id,
                target: terminal_id,
                source_handle: "source".to_owned(),
                target_handle: "target".to_owned(),
                label: String::new(),
                data: BTreeMap::new(),
            },
        ],
    }
}

pub(crate) fn normalize_graph(graph: &mut WorkflowGraph) {
    graph.schema = WORKFLOW_SCHEMA;
    if !graph.viewport.zoom.is_finite() || !(0.2..=2.5).contains(&graph.viewport.zoom) {
        graph.viewport.zoom = 1.0;
    }
    for node in &mut graph.nodes {
        if !node.x.is_finite() {
            node.x = 0.0;
        }
        if !node.y.is_finite() {
            node.y = 0.0;
        }
        if !node.width.is_finite() || !(120.0..=640.0).contains(&node.width) {
            node.width = default_node_width();
        }
        if !node.height.is_finite() || !(48.0..=640.0).contains(&node.height) {
            node.height = default_node_height();
        }
        node.kind = node.kind.trim().to_owned();
        node.title = node.title.trim().to_owned();
    }
}

fn issue(
    level: &str,
    code: &str,
    message: impl Into<String>,
    node_id: Option<&str>,
    edge_id: Option<&str>,
) -> WorkflowValidationIssue {
    WorkflowValidationIssue {
        level: level.to_owned(),
        code: code.to_owned(),
        message: message.into(),
        node_id: node_id.map(str::to_owned),
        edge_id: edge_id.map(str::to_owned),
    }
}

pub(crate) fn validate_graph(graph: &WorkflowGraph, mode: &str) -> WorkflowValidationReport {
    let mut issues = Vec::new();
    if graph.nodes.len() > MAX_WORKFLOW_NODES {
        issues.push(issue(
            "error",
            "node-limit",
            format!("节点数量超过 {MAX_WORKFLOW_NODES} 个"),
            None,
            None,
        ));
    }
    if graph.edges.len() > MAX_WORKFLOW_EDGES {
        issues.push(issue(
            "error",
            "edge-limit",
            format!("连线数量超过 {MAX_WORKFLOW_EDGES} 条"),
            None,
            None,
        ));
    }
    if graph.nodes.is_empty() {
        issues.push(issue("error", "graph-empty", "工作流画布为空", None, None));
    }
    let mut ids = HashSet::new();
    for node in &graph.nodes {
        if node.id.is_empty() || !ids.insert(node.id.as_str()) {
            issues.push(issue(
                "error",
                "node-id",
                "节点 ID 为空或重复",
                Some(&node.id),
                None,
            ));
        }
        if node.title.is_empty() {
            issues.push(issue(
                "warning",
                "node-title",
                "节点标题为空",
                Some(&node.id),
                None,
            ));
        }
        if !matches!(
            node.kind.as_str(),
            "start"
                | "llm"
                | "template-transform"
                | "if-else"
                | "http-request"
                | "code"
                | "answer"
                | "end"
        ) {
            issues.push(issue(
                "warning",
                "node-runtime-support",
                format!(
                    "节点类型 {} 将原样保留到 DSL，本地执行器暂未实现",
                    node.kind
                ),
                Some(&node.id),
                None,
            ));
        }
    }
    let start_nodes: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| node.kind == "start")
        .collect();
    if start_nodes.len() != 1 {
        issues.push(issue(
            "error",
            "start-count",
            "工作流需要且只允许一个开始节点",
            None,
            None,
        ));
    }
    let terminal_kind = if mode == "advanced-chat" {
        "answer"
    } else {
        "end"
    };
    if !graph.nodes.iter().any(|node| node.kind == terminal_kind) {
        issues.push(issue(
            "error",
            "terminal-missing",
            format!("当前模式至少需要一个 {terminal_kind} 节点"),
            None,
            None,
        ));
    }
    let node_ids: HashSet<&str> = graph.nodes.iter().map(|node| node.id.as_str()).collect();
    let mut edge_ids = HashSet::new();
    let mut outgoing: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut incoming: HashMap<&str, usize> = HashMap::new();
    for edge in &graph.edges {
        if edge.id.is_empty() || !edge_ids.insert(edge.id.as_str()) {
            issues.push(issue(
                "error",
                "edge-id",
                "连线 ID 为空或重复",
                None,
                Some(&edge.id),
            ));
        }
        if edge.source == edge.target {
            issues.push(issue(
                "error",
                "self-edge",
                "节点不能连接到自身",
                Some(&edge.source),
                Some(&edge.id),
            ));
        }
        if !node_ids.contains(edge.source.as_str()) || !node_ids.contains(edge.target.as_str()) {
            issues.push(issue(
                "error",
                "dangling-edge",
                "连线引用了不存在的节点",
                None,
                Some(&edge.id),
            ));
            continue;
        }
        outgoing
            .entry(edge.source.as_str())
            .or_default()
            .push(edge.target.as_str());
        *incoming.entry(edge.target.as_str()).or_default() += 1;
    }
    for node in &graph.nodes {
        if node.kind != "start" && incoming.get(node.id.as_str()).copied().unwrap_or(0) == 0 {
            issues.push(issue(
                "warning",
                "node-disconnected",
                "节点没有输入连线",
                Some(&node.id),
                None,
            ));
        }
    }
    if let Some(start) = start_nodes.first() {
        let mut queue = VecDeque::from([start.id.as_str()]);
        let mut reachable = HashSet::new();
        while let Some(id) = queue.pop_front() {
            if !reachable.insert(id) {
                continue;
            }
            if let Some(next) = outgoing.get(id) {
                queue.extend(next.iter().copied());
            }
        }
        for node in &graph.nodes {
            if !reachable.contains(node.id.as_str()) {
                issues.push(issue(
                    "warning",
                    "node-unreachable",
                    "节点无法从开始节点到达",
                    Some(&node.id),
                    None,
                ));
            }
        }
        if !graph
            .nodes
            .iter()
            .any(|node| node.kind == terminal_kind && reachable.contains(node.id.as_str()))
        {
            issues.push(issue(
                "error",
                "terminal-unreachable",
                "从开始节点无法到达输出节点",
                None,
                None,
            ));
        }
    }
    if contains_cycle(graph) {
        issues.push(issue(
            "error",
            "graph-cycle",
            "基础工作流不允许形成循环；循环逻辑请使用后续的迭代/循环容器节点",
            None,
            None,
        ));
    }
    WorkflowValidationReport {
        valid: !issues.iter().any(|item| item.level == "error"),
        node_count: graph.nodes.len(),
        edge_count: graph.edges.len(),
        issues,
    }
}

fn contains_cycle(graph: &WorkflowGraph) -> bool {
    let mut indegree: HashMap<&str, usize> = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), 0))
        .collect();
    let mut outgoing: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in &graph.edges {
        if indegree.contains_key(edge.source.as_str())
            && indegree.contains_key(edge.target.as_str())
        {
            *indegree.entry(edge.target.as_str()).or_default() += 1;
            outgoing
                .entry(edge.source.as_str())
                .or_default()
                .push(edge.target.as_str());
        }
    }
    let mut queue: VecDeque<&str> = indegree
        .iter()
        .filter_map(|(id, count)| (*count == 0).then_some(*id))
        .collect();
    let mut visited = 0;
    while let Some(id) = queue.pop_front() {
        visited += 1;
        if let Some(targets) = outgoing.get(id) {
            for target in targets {
                if let Some(count) = indegree.get_mut(target) {
                    *count -= 1;
                    if *count == 0 {
                        queue.push_back(target);
                    }
                }
            }
        }
    }
    visited != graph.nodes.len()
}

pub(crate) fn new_node(kind: &str, x: f64, y: f64) -> WorkflowNode {
    let id = format!("node-{}", Uuid::new_v4().simple());
    let (title, config, height) = match kind {
        "start" => (
            "开始",
            BTreeMap::from([("variables".to_owned(), json!([]))]),
            84.0,
        ),
        "llm" => (
            "LLM",
            BTreeMap::from([
                (
                    "prompt_template".to_owned(),
                    json!([{"role": "user", "text": "{{#start.query#}}"}]),
                ),
                (
                    "model".to_owned(),
                    json!({"provider": "", "name": "", "mode": "chat", "completion_params": {}}),
                ),
            ]),
            104.0,
        ),
        "template-transform" => (
            "模板转换",
            BTreeMap::from([
                ("template".to_owned(), json!("{{ input }}")),
                ("variables".to_owned(), json!([])),
            ]),
            96.0,
        ),
        "if-else" => (
            "条件分支",
            BTreeMap::from([(
                "cases".to_owned(),
                json!([{"case_id": "true", "logical_operator": "and", "conditions": [{"id": "condition", "variable_selector": ["start", "query"], "comparison_operator": "contains", "value": ""}]}]),
            )]),
            112.0,
        ),
        "http-request" => (
            "HTTP 请求",
            BTreeMap::from([
                ("method".to_owned(), json!("get")),
                ("url".to_owned(), json!("https://example.com")),
                ("headers".to_owned(), json!("")),
                ("params".to_owned(), json!("")),
                ("body".to_owned(), json!({"type": "none", "data": []})),
            ]),
            108.0,
        ),
        "code" => (
            "代码执行",
            BTreeMap::from([
                ("code_language".to_owned(), json!("python3")),
                (
                    "code".to_owned(),
                    json!("def main(input: str):\n    return {'result': input}\n"),
                ),
                ("variables".to_owned(), json!([])),
                ("outputs".to_owned(), json!({"result": {"type": "string"}})),
            ]),
            112.0,
        ),
        "answer" => (
            "直接回复",
            BTreeMap::from([("answer".to_owned(), json!("{{#llm.text#}}"))]),
            84.0,
        ),
        _ => (
            "结束",
            BTreeMap::from([(
                "outputs".to_owned(),
                json!([{"variable": "answer", "value_selector": ["llm", "text"]}]),
            )]),
            84.0,
        ),
    };
    WorkflowNode {
        id,
        kind: kind.to_owned(),
        title: title.to_owned(),
        x,
        y,
        width: default_node_width(),
        height,
        config,
    }
}

pub(crate) fn graph_from_dify(value: &Value) -> Option<WorkflowGraph> {
    let graph = value.pointer("/workflow/graph")?;
    let nodes = graph
        .get("nodes")?
        .as_array()?
        .iter()
        .filter_map(|item| {
            let id = item.get("id")?.as_str()?.to_owned();
            let data = item.get("data").and_then(Value::as_object);
            let kind = data
                .and_then(|data| data.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned();
            let title = data
                .and_then(|data| data.get("title"))
                .and_then(Value::as_str)
                .unwrap_or(&kind)
                .to_owned();
            let mut config = data.cloned().unwrap_or_default();
            config.remove("type");
            config.remove("title");
            let position = item.get("position").and_then(Value::as_object);
            Some(WorkflowNode {
                id,
                kind,
                title,
                x: position
                    .and_then(|value| value.get("x"))
                    .and_then(Value::as_f64)
                    .unwrap_or_default(),
                y: position
                    .and_then(|value| value.get("y"))
                    .and_then(Value::as_f64)
                    .unwrap_or_default(),
                width: item
                    .get("width")
                    .and_then(Value::as_f64)
                    .unwrap_or_else(default_node_width),
                height: item
                    .get("height")
                    .and_then(Value::as_f64)
                    .unwrap_or_else(default_node_height),
                config: config.into_iter().collect(),
            })
        })
        .collect();
    let edges = graph
        .get("edges")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let source = item.get("source")?.as_str()?.to_owned();
            let target = item.get("target")?.as_str()?.to_owned();
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("edge-{source}-{target}"));
            let data = item
                .get("data")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .collect();
            Some(WorkflowEdge {
                id,
                source,
                target,
                source_handle: item
                    .get("sourceHandle")
                    .and_then(Value::as_str)
                    .unwrap_or("source")
                    .to_owned(),
                target_handle: item
                    .get("targetHandle")
                    .and_then(Value::as_str)
                    .unwrap_or("target")
                    .to_owned(),
                label: item
                    .get("label")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                data,
            })
        })
        .collect();
    let viewport = graph
        .get("viewport")
        .and_then(|item| serde_json::from_value(item.clone()).ok())
        .unwrap_or_default();
    let mut result = WorkflowGraph {
        schema: WORKFLOW_SCHEMA,
        viewport,
        nodes,
        edges,
    };
    normalize_graph(&mut result);
    Some(result)
}

pub(crate) fn graph_to_dify(graph: &WorkflowGraph) -> Value {
    let nodes: Vec<Value> = graph
        .nodes
        .iter()
        .map(|node| {
            let mut data: Map<String, Value> = node.config.clone().into_iter().collect();
            data.insert("title".to_owned(), json!(node.title));
            data.insert("type".to_owned(), json!(node.kind));
            json!({
                "id": node.id,
                "type": "custom",
                "position": {"x": node.x, "y": node.y},
                "positionAbsolute": {"x": node.x, "y": node.y},
                "width": node.width,
                "height": node.height,
                "data": data,
            })
        })
        .collect();
    let edges: Vec<Value> = graph
        .edges
        .iter()
        .map(|edge| {
            json!({
                "id": edge.id,
                "type": "custom",
                "source": edge.source,
                "target": edge.target,
                "sourceHandle": edge.source_handle,
                "targetHandle": edge.target_handle,
                "label": edge.label,
                "data": edge.data,
                "zIndex": 0,
            })
        })
        .collect();
    json!({
        "viewport": graph.viewport,
        "nodes": nodes,
        "edges": edges,
    })
}
