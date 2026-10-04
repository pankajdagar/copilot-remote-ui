use github_copilot_sdk::SessionEvent;
use serde::Serialize;
use serde_json::Value;

use super::display;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionDetail {
    pub label: String,
    pub value: String,
}

#[derive(Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AgentEvent {
    UserMessage {
        event_id: String,
        message_id: String,
        content: String,
    },
    AssistantDelta {
        event_id: String,
        message_id: String,
        content: String,
    },
    AssistantMessage {
        event_id: String,
        message_id: String,
        content: String,
    },
    Intent {
        event_id: String,
        content: String,
    },
    ReasoningDelta {
        event_id: String,
        reasoning_id: String,
        content: String,
    },
    Reasoning {
        event_id: String,
        reasoning_id: String,
        content: String,
    },
    ModelChanged {
        event_id: String,
        model_id: String,
        context_tier: Option<String>,
        reasoning_effort: Option<String>,
    },
    ToolStarted {
        event_id: String,
        tool_call_id: String,
        tool_name: String,
        description: Option<String>,
        command: Option<String>,
        arguments: Option<Vec<PermissionDetail>>,
        arguments_warning: Option<String>,
    },
    ToolOutput {
        event_id: String,
        tool_call_id: String,
        output: String,
    },
    ToolCompleted {
        event_id: String,
        tool_call_id: String,
        success: bool,
        result: Option<String>,
    },
    ToolProgress {
        event_id: String,
        tool_call_id: String,
        message: String,
    },
    SubagentStarted {
        event_id: String,
        agent_id: Option<String>,
        tool_call_id: String,
        agent_name: String,
        display_name: String,
        description: Option<String>,
        model: Option<String>,
    },
    SubagentCompleted {
        event_id: String,
        agent_id: Option<String>,
        tool_call_id: String,
        agent_name: String,
        display_name: String,
        cancelled: bool,
    },
    SubagentFailed {
        event_id: String,
        agent_id: Option<String>,
        tool_call_id: String,
        agent_name: String,
        display_name: String,
        error: String,
    },
    TaskComplete {
        event_id: String,
        summary: Option<String>,
        outcome: Option<String>,
        success: Option<bool>,
        reason: Option<String>,
        truncated: bool,
    },
    SubagentEvent {
        event_id: String,
        agent_id: Option<String>,
        parent_tool_call_id: Option<String>,
        event: Box<AgentEvent>,
    },
    PermissionRequested {
        event_id: String,
        request_id: String,
        kind: String,
        command: Option<String>,
        description: Option<String>,
        warning: Option<String>,
        details: Vec<PermissionDetail>,
        approvable: bool,
        working_directory: String,
    },
    PermissionAutoApproved {
        event_id: String,
        request_id: String,
        kind: String,
        command: String,
        working_directory: String,
        source: String,
    },
    Working {
        event_id: String,
    },
    Idle {
        event_id: String,
        aborted: bool,
    },
    SleepStatus {
        event_id: String,
        active: bool,
        error: Option<String>,
    },
    Error {
        event_id: String,
        message: String,
    },
    Disconnected {
        event_id: String,
        message: String,
    },
}

fn text<'a>(data: &'a Value, key: &str) -> Option<&'a str> {
    data.get(key).and_then(Value::as_str)
}

fn result_text(value: &Value) -> Option<String> {
    let content = match value {
        Value::String(content) => Some(content.clone()),
        Value::Object(object) => ["content", "text", "stdout", "summary", "message"]
            .into_iter()
            .find_map(|key| object.get(key).and_then(result_text)),
        Value::Array(items) => {
            let pieces: Vec<_> = items.iter().filter_map(result_text).collect();
            (!pieces.is_empty()).then(|| pieces.join("\n"))
        }
        _ => None,
    }?;
    Some(content.chars().take(500).collect())
}

fn tool_command(data: &Value) -> Option<String> {
    let from_object = |arguments: &Value| {
        ["fullCommandText", "command", "shellCommand"]
            .into_iter()
            .find_map(|key| text(arguments, key))
            .map(str::to_owned)
    };
    let command = data
        .get("arguments")
        .and_then(|arguments| match arguments {
            Value::String(raw) => serde_json::from_str::<Value>(raw)
                .ok()
                .and_then(|parsed| from_object(&parsed))
                .or_else(|| {
                    text(data, "toolName")
                        .filter(|tool| {
                            matches!(
                                tool.to_ascii_lowercase().as_str(),
                                "shell" | "bash" | "local_shell" | "powershell"
                            )
                        })
                        .map(|_| raw.to_owned())
                }),
            other => from_object(other),
        })
        .or_else(|| text(data, "command").map(str::to_owned))?;
    Some(command.chars().take(2048).collect())
}

fn tool_arguments(data: &Value) -> (Option<Vec<PermissionDetail>>, Option<String>) {
    let Some(arguments) = data.get("arguments") else {
        return (None, None);
    };
    let parsed = match arguments {
        Value::String(raw) => match serde_json::from_str::<Value>(raw) {
            Ok(parsed) => parsed,
            Err(_) => {
                return (
                    None,
                    (!matches!(
                        text(data, "toolName")
                            .map(|name| name.to_ascii_lowercase())
                            .as_deref(),
                        Some("shell" | "bash" | "local_shell" | "powershell")
                    ))
                    .then(|| "Copilot did not supply structured tool arguments.".into()),
                );
            }
        },
        other => other.clone(),
    };
    let Some(rendered) = display::argument_details(&parsed) else {
        return (
            None,
            Some(
                "Tool arguments exceed the safe display limit or have an unsupported shape.".into(),
            ),
        );
    };
    let fields = rendered
        .fields
        .into_iter()
        .map(|(label, value)| PermissionDetail { label, value })
        .collect();
    (
        Some(fields),
        (!rendered.complete).then(|| "Sensitive argument values were redacted.".into()),
    )
}

pub fn child_scope(event: &SessionEvent) -> Option<(Option<String>, Option<String>)> {
    let agent_id = event
        .agent_id
        .clone()
        .or_else(|| text(&event.data, "agentId").map(str::to_owned));
    let parent_tool_call_id = text(&event.data, "parentToolCallId").map(str::to_owned);
    (agent_id.is_some() || parent_tool_call_id.is_some()).then_some((agent_id, parent_tool_call_id))
}

pub fn normalize(event: &SessionEvent) -> Option<AgentEvent> {
    let id = event.id.clone();
    let data = &event.data;
    let malformed = || AgentEvent::Error {
        event_id: id.clone(),
        message: format!("Copilot sent an incomplete {} event", event.event_type),
    };
    let normalized = match event.event_type.as_str() {
        "user.message" => Some(AgentEvent::UserMessage {
            event_id: id.clone(),
            message_id: text(data, "messageId").unwrap_or(&event.id).to_owned(),
            content: match text(data, "content") {
                Some(content) => content.to_owned(),
                None => return Some(malformed()),
            },
        }),
        "assistant.message_delta" => Some(AgentEvent::AssistantDelta {
            event_id: id.clone(),
            message_id: match text(data, "messageId") {
                Some(value) => value.to_owned(),
                None => return Some(malformed()),
            },
            content: match text(data, "deltaContent") {
                Some(value) => value.to_owned(),
                None => return Some(malformed()),
            },
        }),
        "assistant.message" => Some(AgentEvent::AssistantMessage {
            event_id: id.clone(),
            message_id: match text(data, "messageId") {
                Some(value) => value.to_owned(),
                None => return Some(malformed()),
            },
            content: match text(data, "content") {
                Some(value) => value.to_owned(),
                None => return Some(malformed()),
            },
        }),
        "assistant.intent" => Some(AgentEvent::Intent {
            event_id: id.clone(),
            content: match text(data, "intent") {
                Some(value) => value.to_owned(),
                None => return Some(malformed()),
            },
        }),
        "assistant.reasoning_delta" => Some(AgentEvent::ReasoningDelta {
            event_id: id.clone(),
            reasoning_id: match text(data, "reasoningId") {
                Some(value) => value.to_owned(),
                None => return Some(malformed()),
            },
            content: match text(data, "deltaContent") {
                Some(value) => value.to_owned(),
                None => return Some(malformed()),
            },
        }),
        "assistant.reasoning" => Some(AgentEvent::Reasoning {
            event_id: id.clone(),
            reasoning_id: match text(data, "reasoningId") {
                Some(value) => value.to_owned(),
                None => return Some(malformed()),
            },
            content: match text(data, "content") {
                Some(value) => value.to_owned(),
                None => return Some(malformed()),
            },
        }),
        "session.model_change" => Some(AgentEvent::ModelChanged {
            event_id: id.clone(),
            model_id: match text(data, "newModel") {
                Some(value) => value.to_owned(),
                None => return Some(malformed()),
            },
            context_tier: text(data, "contextTier").map(|tier| {
                if matches!(tier, "default" | "long_context") {
                    tier.to_owned()
                } else {
                    "unknown".into()
                }
            }),
            reasoning_effort: text(data, "reasoningEffort").map(str::to_owned),
        }),
        "tool.execution_start" => {
            let (arguments, arguments_warning) = tool_arguments(data);
            Some(AgentEvent::ToolStarted {
                event_id: id.clone(),
                tool_call_id: text(data, "toolCallId").unwrap_or(&id).to_owned(),
                tool_name: text(data, "toolName").unwrap_or("Tool").to_owned(),
                description: text(data, "description").map(str::to_owned),
                command: tool_command(data),
                arguments,
                arguments_warning,
            })
        }
        "tool.execution_partial_result" => Some(AgentEvent::ToolOutput {
            event_id: id.clone(),
            tool_call_id: match text(data, "toolCallId") {
                Some(value) => value.to_owned(),
                None => return Some(malformed()),
            },
            output: match data.get("partialOutput").and_then(|value| match value {
                Value::String(text) => Some(text.chars().take(4096).collect()),
                other => result_text(other),
            }) {
                Some(value) => value,
                None => return Some(malformed()),
            },
        }),
        "tool.execution_complete" => {
            let success = match data.get("success").and_then(Value::as_bool) {
                Some(success) => success,
                None if data.get("error").is_some_and(|error| !error.is_null()) => false,
                None => return Some(malformed()),
            };
            Some(AgentEvent::ToolCompleted {
                event_id: id.clone(),
                tool_call_id: text(data, "toolCallId").unwrap_or(&id).to_owned(),
                success,
                result: data
                    .get("result")
                    .and_then(result_text)
                    .or_else(|| data.get("error").and_then(result_text)),
            })
        }
        "tool.execution_progress" => Some(AgentEvent::ToolProgress {
            event_id: id.clone(),
            tool_call_id: match text(data, "toolCallId") {
                Some(value) => value.to_owned(),
                None => return Some(malformed()),
            },
            message: match text(data, "progressMessage") {
                Some(value) => value.to_owned(),
                None => return Some(malformed()),
            },
        }),
        "subagent.started" => Some(AgentEvent::SubagentStarted {
            event_id: id.clone(),
            agent_id: event.agent_id.clone(),
            tool_call_id: match text(data, "toolCallId") {
                Some(value) => value.to_owned(),
                None => return Some(malformed()),
            },
            agent_name: match text(data, "agentName") {
                Some(value) => value.to_owned(),
                None => return Some(malformed()),
            },
            display_name: match text(data, "agentDisplayName") {
                Some(value) => value.to_owned(),
                None => return Some(malformed()),
            },
            description: text(data, "agentDescription").map(str::to_owned),
            model: text(data, "model").map(str::to_owned),
        }),
        "subagent.completed" => Some(AgentEvent::SubagentCompleted {
            event_id: id.clone(),
            agent_id: event.agent_id.clone(),
            tool_call_id: match text(data, "toolCallId") {
                Some(value) => value.to_owned(),
                None => return Some(malformed()),
            },
            agent_name: match text(data, "agentName") {
                Some(value) => value.to_owned(),
                None => return Some(malformed()),
            },
            display_name: match text(data, "agentDisplayName") {
                Some(value) => value.to_owned(),
                None => return Some(malformed()),
            },
            cancelled: data
                .get("cancelled")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }),
        "subagent.failed" => Some(AgentEvent::SubagentFailed {
            event_id: id.clone(),
            agent_id: event.agent_id.clone(),
            tool_call_id: match text(data, "toolCallId") {
                Some(value) => value.to_owned(),
                None => return Some(malformed()),
            },
            agent_name: match text(data, "agentName") {
                Some(value) => value.to_owned(),
                None => return Some(malformed()),
            },
            display_name: match text(data, "agentDisplayName") {
                Some(value) => value.to_owned(),
                None => return Some(malformed()),
            },
            error: match text(data, "error") {
                Some(value) => value.to_owned(),
                None => return Some(malformed()),
            },
        }),
        "session.task_complete" => {
            let summary = text(data, "summary").filter(|summary| !summary.trim().is_empty());
            Some(AgentEvent::TaskComplete {
                event_id: id.clone(),
                summary: summary.map(|summary| summary.chars().take(64_000).collect()),
                truncated: summary.is_some_and(|summary| summary.chars().count() > 64_000),
                outcome: text(data, "outcome").map(str::to_owned),
                success: data.get("success").and_then(Value::as_bool),
                reason: text(data, "reason").map(|reason| reason.chars().take(1000).collect()),
            })
        }
        "assistant.turn_start" => Some(AgentEvent::Working {
            event_id: id.clone(),
        }),
        "session.idle" => Some(AgentEvent::Idle {
            event_id: id.clone(),
            aborted: data
                .get("aborted")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }),
        "session.error" => Some(AgentEvent::Error {
            event_id: id.clone(),
            message: text(data, "message")
                .unwrap_or("Copilot reported an error")
                .to_owned(),
        }),
        "abort" => Some(AgentEvent::Idle {
            event_id: id.clone(),
            aborted: true,
        }),
        _ => None,
    };
    if matches!(
        event.event_type.as_str(),
        "subagent.started" | "subagent.completed" | "subagent.failed"
    ) {
        return normalized;
    }
    match (child_scope(event), normalized) {
        (Some((agent_id, parent_tool_call_id)), Some(inner)) => Some(AgentEvent::SubagentEvent {
            event_id: id,
            agent_id,
            parent_tool_call_id,
            event: Box::new(inner),
        }),
        (_, other) => other,
    }
}

pub fn history(events: &[SessionEvent]) -> Vec<AgentEvent> {
    let completed: std::collections::HashSet<_> = events
        .iter()
        .filter(|event| event.event_type == "assistant.message")
        .filter_map(|event| text(&event.data, "messageId").map(|id| (child_scope(event), id)))
        .collect();
    let completed_reasoning: std::collections::HashSet<_> = events
        .iter()
        .filter(|event| event.event_type == "assistant.reasoning")
        .filter_map(|event| text(&event.data, "reasoningId").map(|id| (child_scope(event), id)))
        .collect();
    events
        .iter()
        .filter(|event| match event.event_type.as_str() {
            "assistant.message_delta" => text(&event.data, "messageId")
                .is_none_or(|id| !completed.contains(&(child_scope(event), id))),
            "assistant.reasoning_delta" => text(&event.data, "reasoningId")
                .is_none_or(|id| !completed_reasoning.contains(&(child_scope(event), id))),
            "assistant.intent" | "tool.execution_progress" | "tool.execution_partial_result" => {
                false
            }
            _ => true,
        })
        .filter_map(normalize)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: &str, data: Value) -> SessionEvent {
        serde_json::from_value(serde_json::json!({
            "id": "event-1", "timestamp": "2026-09-25T00:00:00Z", "type": kind, "data": data
        }))
        .expect("valid SDK event fixture")
    }

    #[test]
    fn normalizes_messages_without_exposing_raw_sdk_data() {
        let start = event("assistant.turn_start", serde_json::json!({}));
        let delta = event(
            "assistant.message_delta",
            serde_json::json!({
                "messageId": "m1", "deltaContent": "Hello "
            }),
        );
        let complete = event(
            "assistant.message",
            serde_json::json!({
                "messageId": "m1", "content": "Hello world", "apiCallId": "private"
            }),
        );
        let messages = history(&[start, delta, complete]);
        assert_eq!(messages.len(), 2);
        let rendered = serde_json::to_string(&messages).expect("serializable normalized messages");
        assert!(rendered.contains("Hello world"));
        assert!(rendered.contains("messageId"));
        assert!(!rendered.contains("private"));
        assert!(!rendered.contains("deltaContent"));
    }

    #[test]
    fn tool_result_extracts_text_without_rendering_raw_json() {
        let completed = event(
            "tool.execution_complete",
            serde_json::json!({
                "toolCallId": "tool-1", "success": true,
                "result": { "content": [{ "text": "42 tests passed" }] }
            }),
        );
        let rendered =
            serde_json::to_value(normalize(&completed)).expect("serializable tool event");
        assert_eq!(rendered["result"], "42 tests passed");
        assert!(rendered.get("content").is_none());
    }

    #[test]
    fn exposes_only_shell_command_and_bounded_live_output() {
        let started = event(
            "tool.execution_start",
            serde_json::json!({
                "toolCallId": "shell-1", "toolName": "Shell",
                "arguments": { "command": "npm run build", "internal": "not shown" }
            }),
        );
        let start = serde_json::to_value(normalize(&started)).expect("start event");
        assert_eq!(start["command"], "npm run build");
        assert_eq!(start["arguments"][0]["label"], "command");
        assert_eq!(start["arguments"][1]["value"], "not shown");
        let serialized = serde_json::to_value(normalize(&event(
            "tool.execution_start",
            serde_json::json!({
                "toolCallId": "shell-2", "toolName": "Bash",
                "arguments": "{\"command\":\"cargo test\"}"
            }),
        )))
        .expect("string arguments parsed");
        assert_eq!(serialized["command"], "cargo test");
        let partial = event(
            "tool.execution_partial_result",
            serde_json::json!({ "toolCallId": "shell-1", "partialOutput": "x".repeat(5_000) }),
        );
        let rendered = serde_json::to_value(normalize(&partial)).expect("partial output");
        assert_eq!(rendered["type"], "toolOutput");
        assert_eq!(
            rendered["output"].as_str().expect("output string").len(),
            4096
        );
        assert!(history(&[started, partial])
            .iter()
            .all(|event| !matches!(event, AgentEvent::ToolOutput { .. })));
    }

    #[test]
    fn tool_inputs_show_nested_values_without_forwarding_secret_fields_or_opaque_payloads() {
        let started = event(
            "tool.execution_start",
            serde_json::json!({
                "toolCallId": "mcp-1", "toolName": "mcp.search",
                "arguments": {
                    "query": {"keywords": ["oauth", "callback"], "apiKey": "private-marker"},
                    "limit": 5
                },
                "encryptedContent": "opaque"
            }),
        );
        let rendered = serde_json::to_value(normalize(&started)).expect("safe tool inputs");
        assert_eq!(rendered["type"], "toolStarted");
        assert_eq!(
            rendered["argumentsWarning"],
            "Sensitive argument values were redacted."
        );
        let display = rendered.to_string();
        assert!(display.contains("oauth"));
        assert!(display.contains("[redacted]"));
        assert!(!display.contains("private-marker"));
        assert!(!display.contains("opaque"));
        let oversized = event(
            "tool.execution_start",
            serde_json::json!({
                "toolCallId": "mcp-2", "toolName": "mcp.search",
                "arguments": {"query": "x".repeat(4097)}
            }),
        );
        let rendered = serde_json::to_value(normalize(&oversized)).expect("bounded tool inputs");
        assert!(rendered["arguments"].is_null());
        assert!(rendered["argumentsWarning"]
            .as_str()
            .is_some_and(|warning| warning.contains("display limit")));
    }

    #[test]
    fn normalizes_only_public_progress_and_reasoning_summaries() {
        let intent = serde_json::to_value(normalize(&event(
            "assistant.intent",
            serde_json::json!({"intent": "Exploring the repository", "private": "hidden"}),
        )))
        .expect("serializable intent");
        assert_eq!(intent["type"], "intent");
        assert_eq!(intent["content"], "Exploring the repository");
        assert!(intent.get("private").is_none());

        let summary = serde_json::to_value(normalize(&event(
            "assistant.reasoning",
            serde_json::json!({
                "reasoningId": "reason-1", "content": "Checking the error path",
                "reasoningOpaque": "not for display", "encryptedContent": "not for display"
            }),
        )))
        .expect("serializable reasoning summary");
        assert_eq!(summary["type"], "reasoning");
        assert_eq!(summary["reasoningId"], "reason-1");
        assert_eq!(summary["content"], "Checking the error path");
        assert!(summary.get("reasoningOpaque").is_none());
        assert!(summary.get("encryptedContent").is_none());

        let progress = serde_json::to_value(normalize(&event(
            "tool.execution_progress",
            serde_json::json!({"toolCallId": "tool-1", "progressMessage": "Reading files"}),
        )))
        .expect("serializable tool progress");
        assert_eq!(progress["type"], "toolProgress");
        assert_eq!(progress["message"], "Reading files");

        let subagent = serde_json::to_value(normalize(&event(
            "subagent.started",
            serde_json::json!({
                "agentName": "code-review", "agentDisplayName": "Code review",
                "agentDescription": "Checking changes", "toolCallId": "task-1"
            }),
        )))
        .expect("serializable subagent activity");
        assert_eq!(subagent["type"], "subagentStarted");
        assert_eq!(subagent["displayName"], "Code review");
        assert_eq!(subagent["toolCallId"], "task-1");
    }

    #[test]
    fn history_drops_completed_reasoning_fragments_and_ephemeral_status() {
        let events = [
            event(
                "assistant.reasoning_delta",
                serde_json::json!({"reasoningId": "r1", "deltaContent": "Checking"}),
            ),
            event(
                "assistant.reasoning",
                serde_json::json!({"reasoningId": "r1", "content": "Checking the code"}),
            ),
            event(
                "assistant.intent",
                serde_json::json!({"intent": "Reading files"}),
            ),
            event(
                "tool.execution_progress",
                serde_json::json!({"toolCallId": "t1", "progressMessage": "Reading"}),
            ),
            event(
                "subagent.completed",
                serde_json::json!({
                    "agentName": "review", "agentDisplayName": "Review",
                    "toolCallId": "task-1"
                }),
            ),
        ];
        let replay = history(&events);
        let types: Vec<_> = replay
            .iter()
            .map(|event| {
                serde_json::to_value(event).expect("serializable event")["type"]
                    .as_str()
                    .expect("type string")
                    .to_owned()
            })
            .collect();
        assert_eq!(types, ["reasoning", "subagentCompleted"]);
    }

    #[test]
    fn separates_subagent_messages_and_legacy_child_tools_from_parent_chat() {
        let child: SessionEvent = serde_json::from_value(serde_json::json!({
            "id": "child-message", "timestamp": "2026-09-25T00:00:00Z",
            "agentId": "agent-1", "type": "assistant.message",
            "data": { "messageId": "m-1", "content": "Child findings" }
        }))
        .expect("valid child event");
        let wrapped = serde_json::to_value(normalize(&child)).expect("child message serialized");
        assert_eq!(wrapped["type"], "subagentEvent");
        assert_eq!(wrapped["agentId"], "agent-1");
        assert_eq!(wrapped["event"]["type"], "assistantMessage");
        assert_eq!(wrapped["event"]["content"], "Child findings");

        let legacy = event(
            "tool.execution_start",
            serde_json::json!({
                "toolCallId": "read-1", "toolName": "Read",
                "parentToolCallId": "task-1"
            }),
        );
        let routed = serde_json::to_value(normalize(&legacy)).expect("legacy child serialized");
        assert_eq!(routed["type"], "subagentEvent");
        assert_eq!(routed["parentToolCallId"], "task-1");
        assert_eq!(routed["event"]["type"], "toolStarted");
        assert!(child_scope(&event(
            "assistant.message",
            serde_json::json!({"messageId":"main","content":"Main reply"})
        ))
        .is_none());
        let same_message_id = event(
            "assistant.message_delta",
            serde_json::json!({"messageId": "m-1", "deltaContent": "Parent text"}),
        );
        let replay = history(&[same_message_id, child]);
        assert_eq!(replay.len(), 2);
        assert!(matches!(replay[0], AgentEvent::AssistantDelta { .. }));
    }

    #[test]
    fn task_complete_exposes_accepted_agent_summary_without_tool_payload() {
        let completed = serde_json::to_value(normalize(&event(
            "session.task_complete",
            serde_json::json!({
                "summary": "Fixed the UI and verified the tests.",
                "success": true, "outcome": "completed",
                "internal": "not user-facing"
            }),
        )))
        .expect("completion serialized");
        assert_eq!(completed["type"], "taskComplete");
        assert_eq!(completed["summary"], "Fixed the UI and verified the tests.");
        assert_eq!(completed["success"], true);
        assert!(completed.get("internal").is_none());
        let denied = serde_json::to_value(normalize(&event(
            "session.task_complete",
            serde_json::json!({
                "summary": "Premature",
                "success": false, "outcome": "continue", "reason": "More work needed"
            }),
        )))
        .expect("rejected completion serialized");
        assert_eq!(denied["success"], false);
        assert_eq!(denied["outcome"], "continue");
    }

    #[test]
    fn model_change_only_exposes_the_active_model_id() {
        let rendered = serde_json::to_value(normalize(&event(
            "session.model_change",
            serde_json::json!({
                "newModel": "claude-sonnet-5", "previousModel": "gpt-5.4",
                "contextTier": "long_context", "reasoningEffort": "high",
                "private": "not a UI field"
            }),
        )))
        .expect("serializable model change");
        assert_eq!(rendered["type"], "modelChanged");
        assert_eq!(rendered["modelId"], "claude-sonnet-5");
        assert_eq!(rendered["contextTier"], "long_context");
        assert_eq!(rendered["reasoningEffort"], "high");
        assert!(rendered.get("private").is_none());
    }
}
