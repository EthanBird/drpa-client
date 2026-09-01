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

    // System prompt, injected project context, and durable context checkpoints
    // are a fixed prefix. Dropping any one of them makes later rounds appear
    // stateless even though recent chat messages are still present.
    let fixed_prefix_len = messages
        .iter()
        .take_while(|message| message.get("role").and_then(Value::as_str) == Some("system"))
        .count();
    let fixed_prefix = &messages[..fixed_prefix_len];
    let history = &messages[fixed_prefix_len..];
    let groups = group_messages(history);
    let fixed_tokens = fixed_prefix.iter().map(estimate_tokens).sum::<u64>();
    let history_tokens = groups.iter().flatten().map(estimate_tokens).sum::<u64>();
    let available_after_fixed = input_budget.saturating_sub(fixed_tokens);
    let summary_reserve = if history_tokens > available_after_fixed {
        (available_after_fixed / 8).clamp(48, 2_048)
    } else {
        0
    };
    let selection_budget = input_budget.saturating_sub(summary_reserve);
    let mut used = fixed_tokens;
    let mut selected = Vec::<Vec<Value>>::new();
    for group in groups.iter().rev() {
        let cost = group.iter().map(estimate_tokens).sum::<u64>();
        if !selected.is_empty() && used.saturating_add(cost) > selection_budget {
            break;
        }
        used = used.saturating_add(cost);
        selected.push(group.clone());
    }
    selected.reverse();
    let selected_count = selected.iter().map(Vec::len).sum::<usize>();
    let omitted_messages = history.len().saturating_sub(selected_count);
    let omitted_group_count = groups.len().saturating_sub(selected.len());
    let mut compacted = fixed_prefix.to_vec();
    if omitted_messages > 0 {
        let summary_chars = usize::try_from(summary_reserve.saturating_mul(4))
            .unwrap_or(8_192)
            .max(256);
        let summary = summarize_omitted_groups(&groups[..omitted_group_count], summary_chars);
        compacted.push(json!({
            "role": "system",
            "content": format!(
                "上下文预算已压缩：较早的 {omitted_messages} 条内部消息已转换为以下有界执行证据。不要假定未列出的工作已经完成。\n\n{summary}"
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

fn summarize_omitted_groups(groups: &[Vec<Value>], max_chars: usize) -> String {
    let mut records = Vec::new();
    let mut used = 0usize;
    for group in groups.iter().rev() {
        let mut group_records = Vec::new();
        for message in group {
            let role = message
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let content = message
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let content = truncate_chars(content, 1_200);
            let tool_names = message
                .get("tool_calls")
                .and_then(Value::as_array)
                .map(|calls| {
                    calls
                        .iter()
                        .filter_map(|call| call.pointer("/function/name").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let tool_call_id = message
                .get("tool_call_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let label = if !tool_names.is_empty() {
                format!("{role} 调用工具 {tool_names}")
            } else if !tool_call_id.is_empty() {
                format!("{role} 结果 {tool_call_id}")
            } else {
                role.to_owned()
            };
            group_records.push(if content.is_empty() {
                format!("- {label}")
            } else {
                format!("- {label}: {content}")
            });
        }
        let record = group_records.join("\n");
        let remaining = max_chars.saturating_sub(used);
        if remaining == 0 {
            break;
        }
        let record_chars = record.chars().count();
        let record = truncate_chars(&record, remaining);
        used = used.saturating_add(record.chars().count());
        records.push(record);
        if record_chars > remaining {
            break;
        }
    }
    records.reverse();
    if records.is_empty() {
        "较早执行证据因上下文预算不足而省略。".to_owned()
    } else {
        records.join("\n")
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    truncated.push_str("…[截断]");
    truncated
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

    #[test]
    fn preserves_all_leading_system_context_and_summarizes_omitted_tool_evidence() {
        let messages = vec![
            json!({"role":"system","content":"agent policy"}),
            json!({"role":"system","content":"durable checkpoint"}),
            json!({"role":"user","content":"old request".repeat(1_000)}),
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"c1","function":{"name":"read_file","arguments":"{}"}}]}),
            json!({"role":"tool","tool_call_id":"c1","content":"manifest evidence".repeat(300)}),
            json!({"role":"user","content":"latest request"}),
        ];
        let assembly = assemble_round_context(&messages, &[], 1_500, 256);
        assert!(assembly.omitted_messages > 0);
        assert_eq!(assembly.messages[0]["content"], "agent policy");
        assert_eq!(assembly.messages[1]["content"], "durable checkpoint");
        let compacted = assembly.messages[2]["content"].as_str().unwrap();
        assert!(compacted.contains("有界执行证据"));
        assert!(compacted.contains("read_file") || compacted.contains("manifest evidence"));
        assert_eq!(
            assembly.messages.last().unwrap()["content"],
            "latest request"
        );
    }
}
