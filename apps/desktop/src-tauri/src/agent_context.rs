use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub(crate) struct ContextAssembly {
    pub(crate) messages: Vec<Value>,
    pub(crate) tools: Vec<Value>,
    pub(crate) estimated_tokens: u64,
    pub(crate) omitted_messages: usize,
    pub(crate) omitted_tools: usize,
}

/// Builds the provider payload for every model round. The canonical event
/// history remains untouched; only the transient provider projection is
/// compacted to fit the configured window.
pub(crate) fn assemble_round_context(
    messages: &[Value],
    tools: &[Value],
    context_window: u32,
    max_output_tokens: u32,
) -> ContextAssembly {
    let tool_budget = (u64::from(context_window) / 3).max(256);
    let mut selected_tools = Vec::new();
    let mut tool_tokens = 0u64;
    for tool in tools {
        let cost = estimate_tokens(tool);
        if !selected_tools.is_empty() && tool_tokens.saturating_add(cost) > tool_budget {
            break;
        }
        tool_tokens = tool_tokens.saturating_add(cost);
        selected_tools.push(tool.clone());
    }
    let omitted_tools = tools.len().saturating_sub(selected_tools.len());
    let reserve = u64::from(max_output_tokens)
        .saturating_add(u64::from(context_window) / 20)
        .saturating_add(tool_tokens);
    let input_budget = u64::from(context_window).saturating_sub(reserve).max(256);
    if messages.is_empty() {
        return ContextAssembly {
            messages: Vec::new(),
            tools: selected_tools,
            estimated_tokens: tool_tokens,
            omitted_messages: 0,
            omitted_tools,
        };
    }

    let system = messages.first().cloned();
    let history = if messages
        .first()
        .and_then(|message| message.get("role"))
        .and_then(Value::as_str)
        == Some("system")
    {
        &messages[1..]
    } else {
        messages
    };
    let groups = group_messages(history);
    let system_tokens = system.as_ref().map(estimate_tokens).unwrap_or(0);
    let mut used = system_tokens;
    let mut selected = Vec::<Vec<Value>>::new();
    for group in groups.iter().rev() {
        let cost = group.iter().map(estimate_tokens).sum::<u64>();
        if !selected.is_empty() && used.saturating_add(cost) > input_budget {
            break;
        }
        used = used.saturating_add(cost);
        selected.push(group.clone());
    }
    selected.reverse();
    let selected_count = selected.iter().map(Vec::len).sum::<usize>();
    let omitted_messages = history.len().saturating_sub(selected_count);
    let mut compacted = Vec::new();
    if let Some(system) = system {
        compacted.push(system);
    }
    if omitted_messages > 0 {
        compacted.push(json!({
            "role": "system",
            "content": format!(
                "上下文预算已压缩：省略较早的 {omitted_messages} 条内部消息；请以保留的最近对话与工具证据为准。"
            )
        }));
    }
    if omitted_tools > 0 {
        compacted.push(json!({
            "role": "system",
            "content": format!(
                "工具 schema 预算已压缩：本轮有 {omitted_tools} 个低优先级工具未暴露；请先使用当前可见工具完成任务。"
            )
        }));
    }
    compacted.extend(selected.into_iter().flatten());
    let estimated_tokens = compacted
        .iter()
        .map(estimate_tokens)
        .sum::<u64>()
        .saturating_add(tool_tokens);
    ContextAssembly {
        messages: compacted,
        tools: selected_tools,
        estimated_tokens,
        omitted_messages,
        omitted_tools,
    }
}

fn group_messages(messages: &[Value]) -> Vec<Vec<Value>> {
    let mut groups = Vec::<Vec<Value>>::new();
    let mut index = 0usize;
    while index < messages.len() {
        let mut group = vec![messages[index].clone()];
        let has_tool_calls = messages[index]
            .get("tool_calls")
            .and_then(Value::as_array)
            .is_some_and(|calls| !calls.is_empty());
        index += 1;
        if has_tool_calls {
            while index < messages.len()
                && messages[index].get("role").and_then(Value::as_str) == Some("tool")
            {
                group.push(messages[index].clone());
                index += 1;
            }
        }
        groups.push(group);
    }
    groups
}

fn estimate_tokens(value: &Value) -> u64 {
    let bytes = serde_json::to_vec(value).map_or(0, |bytes| bytes.len());
    u64::try_from(bytes.saturating_add(3) / 4)
        .unwrap_or(u64::MAX)
        .saturating_add(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_tool_call_groups_when_compacting() {
        let messages = vec![
            json!({"role":"system","content":"system"}),
            json!({"role":"user","content":"old".repeat(4_000)}),
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"c1","function":{"name":"read_file","arguments":"{}"}}]}),
            json!({"role":"tool","tool_call_id":"c1","content":"evidence"}),
            json!({"role":"user","content":"latest"}),
        ];
        let assembly = assemble_round_context(&messages, &[], 1_024, 128);
        assert!(assembly.omitted_messages > 0);
        let roles = assembly
            .messages
            .iter()
            .filter_map(|message| message.get("role").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert_eq!(roles.last(), Some(&"user"));
        let assistant_index = roles.iter().position(|role| *role == "assistant");
        let tool_index = roles.iter().position(|role| *role == "tool");
        assert_eq!(assistant_index.is_some(), tool_index.is_some());
    }

    #[test]
    fn accounts_for_tool_schema_tokens() {
        let messages = vec![json!({"role":"system","content":"system"})];
        let without_tools = assemble_round_context(&messages, &[], 4_096, 512);
        let with_tools = assemble_round_context(
            &messages,
            &[
                json!({"type":"function","function":{"name":"large","description":"x".repeat(2_000)}}),
            ],
            4_096,
            512,
        );
        assert!(with_tools.estimated_tokens > without_tools.estimated_tokens);
    }
}
