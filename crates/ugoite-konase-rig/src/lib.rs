//! Rig-backed [`ugoite_konase::AgentRuntime`] implementation.
//!
//! The adapter uses Rig's sans-IO `AgentRun` as a per-Job state machine. It
//! stops at model and tool boundaries so the CLI or browser host remains the
//! owner of provider and MCP I/O. Rig state is intentionally dropped when a
//! Job completes and is never part of the portable Konase state.

use rig_agent::agent::{AgentRun, AgentRunStep, ModelTurn, ModelTurnOutcome};
use rig_agent::completion::{AssistantContent, Message, Usage};
use rig_agent::core::completion::message::{ToolResultContent, UserContent};
use rig_agent::core::message::Text;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use ugoite_konase::{
    AgentAction, AgentRuntime, AgentRuntimeError, AgentRuntimeInput, ContextCapsule, JobOutcome,
    JobSpec, McpRequest, McpResult, ModelMessage, ModelRequest, ModelResult, ModelTool,
};

const MAX_TURNS: usize = 8;

#[derive(Debug)]
enum PendingStep {
    Model {
        request_id: String,
    },
    Tool {
        request_id: String,
        call_id: String,
        name: String,
    },
    Confirmation {
        request: McpRequest,
        call_id: String,
    },
}

#[derive(Debug)]
struct QueuedToolCall {
    call_id: String,
    name: String,
    arguments: BTreeMap<String, Value>,
    effect: Option<ugoite_konase::CapabilityEffect>,
}

/// A fresh Rig agent run for the active Konase Job.
#[derive(Debug, Default)]
pub struct RigAgentRuntime {
    run: Option<AgentRun>,
    job_id: Option<String>,
    tools: Vec<ModelTool>,
    pending: Option<PendingStep>,
    queued_tools: VecDeque<QueuedToolCall>,
    tool_results: Vec<UserContent>,
    mcp_sequence: u32,
}

impl RigAgentRuntime {
    fn error(kind: &str, message: impl Into<String>) -> AgentRuntimeError {
        AgentRuntimeError::new(kind, message)
    }

    fn rig_error(error: impl std::fmt::Display) -> AgentRuntimeError {
        Self::error("rig", error.to_string())
    }

    fn active_job_id(&self) -> Result<&str, AgentRuntimeError> {
        self.job_id
            .as_deref()
            .ok_or_else(|| Self::error("inactive", "Rig runtime has no active Job"))
    }

    fn clear(&mut self) {
        self.run = None;
        self.job_id = None;
        self.tools.clear();
        self.pending = None;
        self.queued_tools.clear();
        self.tool_results.clear();
        self.mcp_sequence = 0;
    }

    fn next_action(&mut self) -> Result<AgentAction, AgentRuntimeError> {
        if let Some(call) = self.queued_tools.pop_front() {
            self.mcp_sequence = self.mcp_sequence.saturating_add(1);
            let request_id = format!("{}:mcp:{}", self.active_job_id()?, self.mcp_sequence);
            let mut request = McpRequest {
                request_id,
                server: "ugoite".into(),
                operation: call.name.clone(),
                arguments: call.arguments,
                effect: call.effect,
            };
            let effect = effective_effect(&request)?;
            request.effect = Some(effect);
            match effect {
                ugoite_konase::CapabilityEffect::Read => {
                    self.pending = Some(PendingStep::Tool {
                        request_id: request.request_id.clone(),
                        call_id: call.call_id,
                        name: call.name,
                    });
                    return Ok(AgentAction::CallMcp(request));
                }
                ugoite_konase::CapabilityEffect::Write => {
                    let reason = write_summary(&request)?;
                    self.pending = Some(PendingStep::Confirmation {
                        request: request.clone(),
                        call_id: call.call_id,
                    });
                    return Ok(AgentAction::AskConfirmation(
                        ugoite_konase::ConfirmationRequest {
                            request_id: request.request_id,
                            operation: request.operation,
                            reason,
                        },
                    ));
                }
            }
        }

        let step = self
            .run
            .as_mut()
            .ok_or_else(|| Self::error("inactive", "Rig runtime has no active AgentRun"))?
            .next_step()
            .map_err(Self::rig_error)?;
        match step {
            AgentRunStep::CallModel {
                prompt,
                history,
                turn,
            } => {
                let request_id = format!("{}:model:{turn}", self.active_job_id()?);
                self.pending = Some(PendingStep::Model {
                    request_id: request_id.clone(),
                });
                Ok(AgentAction::CallModel(ModelRequest {
                    request_id,
                    prompt: message_text(&prompt),
                    history: history.into_iter().map(message_to_model).collect(),
                    tools: self.tools.clone(),
                }))
            }
            AgentRunStep::CallTools { calls } => self.queue_tool_calls(calls),
            AgentRunStep::Done(response) => {
                let job_id = self.active_job_id()?.to_owned();
                self.clear();
                Ok(AgentAction::Complete(JobOutcome {
                    job_id,
                    meaningful: !response.output.trim().is_empty(),
                    summary: response.output,
                }))
            }
        }
    }

    fn model_completed(&mut self, result: ModelResult) -> Result<AgentAction, AgentRuntimeError> {
        let PendingStep::Model { request_id } = self
            .pending
            .take()
            .ok_or_else(|| Self::error("unexpected_model_result", "no model request is pending"))?
        else {
            return Err(Self::error(
                "unexpected_model_result",
                "model result arrived while an MCP request was pending",
            ));
        };
        if request_id != result.request_id {
            self.pending = Some(PendingStep::Model { request_id });
            return Err(Self::error(
                "unexpected_model_result",
                "model result does not match the pending request",
            ));
        }
        let mut choice = Vec::new();
        if let Some(text) = result.text {
            if !text.is_empty() {
                choice.push(AssistantContent::text(text));
            }
        }
        choice.extend(
            result
                .tool_calls
                .into_iter()
                .map(|call| AssistantContent::tool_call(call.id, call.name, call.arguments)),
        );
        if choice.is_empty() {
            return Err(Self::error(
                "empty_model_result",
                "model result must contain text or a tool call",
            ));
        }
        let tool_names = self
            .tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect::<BTreeSet<_>>();
        let outcome = self
            .run
            .as_mut()
            .ok_or_else(|| Self::error("inactive", "Rig runtime has no active AgentRun"))?
            .model_response(ModelTurn::new(
                None,
                choice,
                Usage::new(),
                tool_names.clone(),
                tool_names,
            ))
            .map_err(Self::rig_error)?;
        match outcome {
            ModelTurnOutcome::Continue { .. } | ModelTurnOutcome::TurnRetried => self.next_action(),
            ModelTurnOutcome::NeedsResolution(_) => Err(Self::error(
                "unknown_tool",
                "Rig model response requested an unavailable tool",
            )),
        }
    }

    fn mcp_completed(&mut self, result: McpResult) -> Result<AgentAction, AgentRuntimeError> {
        let PendingStep::Tool {
            request_id,
            call_id,
            name,
        } = self
            .pending
            .take()
            .ok_or_else(|| Self::error("unexpected_mcp_result", "no MCP request is pending"))?
        else {
            return Err(Self::error(
                "unexpected_mcp_result",
                "MCP result arrived while a model request was pending",
            ));
        };
        if request_id != result.request_id {
            self.pending = Some(PendingStep::Tool {
                request_id,
                call_id,
                name,
            });
            return Err(Self::error(
                "unexpected_mcp_result",
                "MCP result does not match the pending request",
            ));
        }
        let content = serde_json::to_string(&result)
            .map_err(|error| Self::error("serialization", error.to_string()))?;
        self.tool_results.push(UserContent::tool_result(
            call_id,
            name,
            vec![ToolResultContent::text(content)],
        ));
        if !self.queued_tools.is_empty() {
            return self.next_action();
        }

        let results = std::mem::take(&mut self.tool_results);
        let run = self
            .run
            .as_mut()
            .ok_or_else(|| Self::error("inactive", "Rig runtime has no active AgentRun"))?;
        run.tool_results(results).map_err(Self::rig_error)?;
        self.next_action()
    }

    fn confirmation_completed(
        &mut self,
        result: ugoite_konase::ConfirmationResult,
    ) -> Result<AgentAction, AgentRuntimeError> {
        let Some(PendingStep::Confirmation { request, call_id }) = self.pending.take() else {
            return Err(Self::error(
                "unexpected_confirmation",
                "no write confirmation is pending",
            ));
        };
        if request.request_id != result.request_id {
            self.pending = Some(PendingStep::Confirmation { request, call_id });
            return Err(Self::error(
                "unexpected_confirmation",
                "confirmation does not match the pending write",
            ));
        }
        if !result.approved {
            self.clear();
            return Err(Self::error("write_denied", "Konase write was denied"));
        }
        self.pending = Some(PendingStep::Tool {
            request_id: request.request_id.clone(),
            call_id,
            name: request.operation.clone(),
        });
        Ok(AgentAction::CallMcp(request))
    }

    fn validate_tool_call(
        &self,
        call: rig_agent::agent::run::PendingToolCall,
    ) -> Result<QueuedToolCall, AgentRuntimeError> {
        // The adapter currently maps `NeedsResolution` to `unknown_tool` below
        // and does not enter Rig's invalid-tool recovery. A pre-resolved result
        // is therefore unreachable through this adapter today. Keep this guard
        // defensive without treating Rig's valid recovery content as malformed.
        if call.preresolved_result.is_some() {
            return Err(Self::error(
                "unsupported_tool_result",
                "Rig emitted a pre-resolved tool result",
            ));
        }
        let arguments = call.tool_call.function.arguments;
        let Value::Object(arguments) = arguments else {
            return Err(Self::error(
                "invalid_tool_arguments",
                "Rig tool arguments must be a JSON object",
            ));
        };
        let name = call.tool_call.function.name;
        let effect = self
            .tools
            .iter()
            .find(|tool| tool.name == name)
            .and_then(|tool| tool.effect);
        Ok(QueuedToolCall {
            call_id: call.tool_call.id.as_str().to_owned(),
            name,
            arguments: arguments.into_iter().collect(),
            effect,
        })
    }

    fn queue_tool_calls(
        &mut self,
        calls: Vec<rig_agent::agent::run::PendingToolCall>,
    ) -> Result<AgentAction, AgentRuntimeError> {
        if calls.is_empty() {
            return Err(Self::error(
                "rig_invariant",
                "Rig emitted an empty tool-call batch",
            ));
        }
        self.queued_tools = calls
            .into_iter()
            .map(|call| self.validate_tool_call(call))
            .collect::<Result<VecDeque<_>, _>>()?;
        self.next_action()
    }
}

impl AgentRuntime for RigAgentRuntime {
    fn start(
        &mut self,
        job: JobSpec,
        context: ContextCapsule,
    ) -> Result<AgentAction, AgentRuntimeError> {
        if self.run.is_some() {
            return Err(Self::error(
                "already_running",
                "Rig runtime can execute only one Job at a time",
            ));
        }
        let tools = context
            .available_capabilities
            .iter()
            .filter_map(|capability| {
                capability
                    .input_schema
                    .clone()
                    .map(|input_schema| ModelTool {
                        name: capability.name.clone(),
                        description: capability.description.clone(),
                        input_schema,
                        effect: capability.effect,
                    })
            })
            .collect::<Vec<_>>();
        let prompt = serde_json::to_string(&context)
            .map_err(|error| Self::error("serialization", error.to_string()))?;
        let prompt = format!("Job goal: {}\nContext: {prompt}", job.goal);
        self.run = Some(AgentRun::new(Message::user(prompt)).max_turns(MAX_TURNS));
        self.job_id = Some(job.id);
        self.tools = tools;
        self.pending = None;
        self.queued_tools.clear();
        self.tool_results.clear();
        self.mcp_sequence = 0;
        self.next_action()
    }

    fn resume(&mut self, input: AgentRuntimeInput) -> Result<AgentAction, AgentRuntimeError> {
        match input {
            AgentRuntimeInput::ModelCompleted(result) => self.model_completed(result),
            AgentRuntimeInput::McpCompleted(result) => self.mcp_completed(result),
            AgentRuntimeInput::ConfirmationCompleted(result) => self.confirmation_completed(result),
            AgentRuntimeInput::HostFailed(error) => {
                self.clear();
                Err(Self::error(error.kind.as_str(), error.message))
            }
        }
    }
}

fn effective_effect(
    request: &McpRequest,
) -> Result<ugoite_konase::CapabilityEffect, AgentRuntimeError> {
    use ugoite_konase::CapabilityEffect::{Read, Write};
    match request.operation.as_str() {
        "ugoite.save" | "ugoite.undo" => Ok(Write),
        _ => match request.effect {
            Some(Read) => Ok(Read),
            Some(Write) => Err(AgentRuntimeError::new(
                "unsupported_write",
                format!(
                    "write capability {} cannot be safely previewed",
                    request.operation
                ),
            )),
            None => Err(AgentRuntimeError::new(
                "unknown_effect",
                format!(
                    "capability {} has no trusted effect metadata",
                    request.operation
                ),
            )),
        },
    }
}

fn write_summary(request: &McpRequest) -> Result<String, AgentRuntimeError> {
    const MAX_SUMMARY_CHARS: usize = 1200;
    if request.operation == "ugoite.undo" {
        return Ok("Undo all changes made by this Konase Work in the authorized Space.".into());
    }
    if request.operation != "ugoite.save" {
        return Err(AgentRuntimeError::new(
            "unsupported_write",
            "write preview is unavailable",
        ));
    }
    let id = request
        .arguments
        .get("id")
        .and_then(serde_json::Value::as_str);
    let form = request
        .arguments
        .get("form")
        .and_then(serde_json::Value::as_str);
    let fields = request
        .arguments
        .get("fields")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| {
            AgentRuntimeError::new("invalid_write_preview", "save fields must be an object")
        })?;
    if id.is_some_and(str::is_empty) || form.is_some_and(str::is_empty) {
        return Err(AgentRuntimeError::new(
            "invalid_write_preview",
            "save target contains an empty identifier",
        ));
    }
    if id.is_none() && form.is_none() {
        return Err(AgentRuntimeError::new(
            "invalid_write_preview",
            "new save has no Form target",
        ));
    }
    let mut parts = vec![format!(
        "{} in the authorized Space; Form {}{}.",
        if id.is_some() { "Update" } else { "Create" },
        form.map(safe_preview_label)
            .as_deref()
            .unwrap_or("from the existing Entry"),
        id.map(|value| format!("; Entry {}", safe_preview_label(value)))
            .unwrap_or_default()
    )];
    let field_summary = fields
        .iter()
        .map(|(name, value)| {
            if is_sensitive_field(name) {
                format!("{}: [hidden]", safe_preview_label(name))
            } else {
                format!("{}: {}", safe_preview_label(name), value_summary(value))
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    parts.push(format!(
        "Fields ({}): {field_summary}.",
        if id.is_some() {
            "replace the complete structured field map"
        } else {
            "new Entry values"
        }
    ));
    if let Some(tags) = request
        .arguments
        .get("tags")
        .and_then(serde_json::Value::as_array)
    {
        if !tags.is_empty() {
            parts.push(format!("Tags: {} tag(s).", tags.len()));
        }
    }
    if let Some(extra) = request
        .arguments
        .get("extra_attributes")
        .and_then(serde_json::Value::as_object)
    {
        if !extra.is_empty() {
            parts.push("Extra attributes: present.".into());
        }
    }
    let summary = parts.join(" ");
    if summary.chars().count() <= MAX_SUMMARY_CHARS {
        return Ok(summary);
    }
    let mut clipped = summary
        .chars()
        .take(MAX_SUMMARY_CHARS - 20)
        .collect::<String>();
    clipped.push_str("… [omitted]");
    Ok(clipped)
}

fn safe_preview_label(value: &str) -> String {
    value
        .chars()
        .take(96)
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
}

fn is_sensitive_field(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        "api_key",
        "api key",
        "token",
        "credential",
        "password",
        "secret",
    ]
    .iter()
    .any(|needle| name.contains(needle))
}

fn value_summary(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "empty".into(),
        serde_json::Value::Bool(_) => "boolean".into(),
        serde_json::Value::Number(_) => "number".into(),
        serde_json::Value::String(value) => format!("text ({} chars)", value.chars().count()),
        serde_json::Value::Array(values) => format!("list ({} items)", values.len()),
        serde_json::Value::Object(values) => format!("object ({} fields)", values.len()),
    }
}

fn message_text(message: &Message) -> String {
    match message {
        Message::System { content } => content.clone(),
        Message::User { content } => content
            .iter()
            .map(|item| match item {
                UserContent::Text(Text { text, .. }) => text.clone(),
                other => serde_json::to_string(other).unwrap_or_default(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Message::Assistant { content, .. } => content
            .iter()
            .map(|item| match item {
                AssistantContent::Text(Text { text, .. }) => text.clone(),
                other => serde_json::to_string(other).unwrap_or_default(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn message_to_model(message: Message) -> ModelMessage {
    match message {
        Message::System { content } => ModelMessage::System { content },
        Message::User { content } => ModelMessage::User {
            content: content
                .into_iter()
                .map(|item| serde_json::to_string(&item).unwrap_or_default())
                .collect::<Vec<_>>()
                .join("\n"),
        },
        Message::Assistant { content, .. } => {
            let mut text = Vec::new();
            let mut tool_calls = Vec::new();
            for item in content {
                match item {
                    AssistantContent::Text(Text { text: value, .. }) => text.push(value),
                    AssistantContent::ToolCall(call) => {
                        tool_calls.push(ugoite_konase::ModelToolCall {
                            id: call.id.as_str().to_owned(),
                            name: call.function.name,
                            arguments: call.function.arguments,
                        })
                    }
                    other => text.push(serde_json::to_string(&other).unwrap_or_default()),
                }
            }
            ModelMessage::Assistant {
                content: text.join("\n"),
                tool_calls,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use ugoite_konase::{
        Capability, CapabilityEffect, Observation, ObservationKind, ResourceContent,
        ResourceReference,
    };

    fn job() -> JobSpec {
        JobSpec {
            id: "job-1".into(),
            work_id: "work-1".into(),
            goal: "find and summarize notes".into(),
            expected_response_schema: None,
        }
    }

    fn context() -> ContextCapsule {
        ContextCapsule {
            work_goal: "find and summarize notes".into(),
            job_goal: "find and summarize notes".into(),
            current_strategy_summary: None,
            relevant_observations: vec![],
            available_capabilities: vec![Capability {
                name: "ugoite.search".into(),
                description: "search entries".into(),
                input_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {"q": {"type": "string"}},
                    "required": ["q"]
                })),
                effect: Some(CapabilityEffect::Read),
            }],
            selected_resource_contents: vec![],
            safety_hints: vec![],
            expected_response_schema: None,
        }
    }

    fn context_with_writes() -> ContextCapsule {
        let mut context = context();
        context.available_capabilities.push(Capability {
            name: "ugoite.save".into(),
            description: "Save a structured Entry".into(),
            input_schema: Some(serde_json::json!({"type":"object"})),
            effect: None,
        });
        context
    }

    #[test]
    fn capability_schema_reaches_model_request_unchanged() {
        let mut runtime = RigAgentRuntime::default();
        let AgentAction::CallModel(request) = runtime.start(job(), context()).unwrap() else {
            panic!("expected initial model call");
        };

        assert_eq!(
            request.tools[0].input_schema,
            serde_json::json!({
                "type": "object",
                "properties": {"q": {"type": "string"}},
                "required": ["q"]
            })
        );
    }

    #[test]
    fn selected_context_reaches_initial_model_request_within_prompt_limit() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../ugoite-konase/fixtures/selected-context.json"
        ))
        .unwrap();
        let selected: Vec<ResourceContent> =
            serde_json::from_value(fixture["selected"].clone()).unwrap();
        let mut context = context();
        context.work_goal = "w".repeat(1_024);
        context.job_goal = "j".repeat(1_024);
        context.selected_resource_contents = selected;
        let mut job = job();
        job.goal = "g".repeat(3_500);

        let mut runtime = RigAgentRuntime::default();
        let AgentAction::CallModel(request) = runtime.start(job, context).unwrap() else {
            panic!("expected the initial model request");
        };

        assert!(request.prompt.contains("ugoite://form/"));
        assert!(request.prompt.contains("Expense"));
        assert!(request.prompt.contains("amount: 125"));
        assert!(request.prompt.chars().count() <= 8_192);
    }

    #[test]
    fn model_tool_call_becomes_mcp_call() {
        let mut runtime = RigAgentRuntime::default();
        let first = runtime.start(job(), context()).unwrap();
        let AgentAction::CallModel(request) = first else {
            panic!("expected initial model call");
        };
        let next = runtime
            .resume(AgentRuntimeInput::ModelCompleted(ModelResult {
                request_id: request.request_id,
                text: None,
                tool_calls: vec![ugoite_konase::ModelToolCall {
                    id: "call-1".into(),
                    name: "ugoite.search".into(),
                    arguments: serde_json::json!({"query": "WebAssembly"}),
                }],
            }))
            .unwrap();
        let AgentAction::CallMcp(request) = next else {
            panic!("expected MCP call");
        };
        assert_eq!(request.operation, "ugoite.search");
        assert_eq!(request.effect, Some(CapabilityEffect::Read));
        assert_eq!(request.arguments["query"], "WebAssembly");
    }

    #[test]
    fn each_queued_save_requires_a_distinct_confirmation_and_keeps_its_arguments() {
        let mut runtime = RigAgentRuntime::default();
        let AgentAction::CallModel(model) = runtime.start(job(), context_with_writes()).unwrap()
        else {
            panic!("expected initial model call");
        };
        let save_a = serde_json::json!({"form":"Note", "fields":{"title":"Private title", "api_token":"credential-value"}});
        let save_b = serde_json::json!({"id":"entry-b", "fields":{"title":"B"}});
        let AgentAction::AskConfirmation(confirmation_a) = runtime
            .resume(AgentRuntimeInput::ModelCompleted(ModelResult {
                request_id: model.request_id,
                text: None,
                tool_calls: vec![
                    ugoite_konase::ModelToolCall {
                        id: "a".into(),
                        name: "ugoite.save".into(),
                        arguments: save_a.clone(),
                    },
                    ugoite_konase::ModelToolCall {
                        id: "b".into(),
                        name: "ugoite.save".into(),
                        arguments: save_b.clone(),
                    },
                ],
            }))
            .unwrap()
        else {
            panic!("expected first write confirmation");
        };
        assert!(confirmation_a.reason.contains("Form Note"));
        assert!(confirmation_a.reason.contains("title: text (13 chars)"));
        assert!(confirmation_a.reason.contains("api_token: [hidden]"));
        assert!(!confirmation_a.reason.contains("Private title"));
        assert!(!confirmation_a.reason.contains("credential-value"));
        assert!(!confirmation_a
            .reason
            .contains("replace the complete structured field map"));
        let AgentAction::CallMcp(first) = runtime
            .resume(AgentRuntimeInput::ConfirmationCompleted(
                ugoite_konase::ConfirmationResult {
                    request_id: confirmation_a.request_id.clone(),
                    approved: true,
                },
            ))
            .unwrap()
        else {
            panic!("approved save should produce the held MCP request")
        };
        assert_eq!(
            serde_json::Value::Object(first.arguments.clone().into_iter().collect()),
            save_a
        );
        let AgentAction::AskConfirmation(confirmation_b) = runtime
            .resume(AgentRuntimeInput::McpCompleted(McpResult {
                request_id: first.request_id,
                operation: first.operation,
                success: true,
                observation: None,
                resources: vec![],
                resource_contents: vec![],
                error: None,
            }))
            .unwrap()
        else {
            panic!("second write needs a new confirmation")
        };
        assert_ne!(confirmation_a.request_id, confirmation_b.request_id);
        assert!(confirmation_b.reason.contains("Entry entry-b"));
        assert!(confirmation_b
            .reason
            .contains("replace the complete structured field map"));
        assert!(runtime
            .resume(AgentRuntimeInput::ConfirmationCompleted(
                ugoite_konase::ConfirmationResult {
                    request_id: confirmation_b.request_id,
                    approved: false
                }
            ))
            .is_err());
        assert!(runtime.run.is_none());
    }

    #[test]
    fn write_effect_without_safe_preview_is_rejected() {
        let request = McpRequest {
            request_id: "id".into(),
            server: "ugoite".into(),
            operation: "other.write".into(),
            arguments: BTreeMap::new(),
            effect: Some(CapabilityEffect::Write),
        };
        assert_eq!(
            effective_effect(&request).unwrap_err().kind,
            "unsupported_write"
        );
        let mut unknown = request;
        unknown.effect = None;
        assert_eq!(
            effective_effect(&unknown).unwrap_err().kind,
            "unknown_effect"
        );
    }

    #[test]
    fn multiple_model_tool_calls_are_executed_in_order_and_results_are_batched() {
        let mut runtime = RigAgentRuntime::default();
        let AgentAction::CallModel(model) = runtime.start(job(), context()).unwrap() else {
            panic!("expected initial model call");
        };
        let AgentAction::CallMcp(first) = runtime
            .resume(AgentRuntimeInput::ModelCompleted(ModelResult {
                request_id: model.request_id,
                text: None,
                tool_calls: vec![
                    ugoite_konase::ModelToolCall {
                        id: "call-1".into(),
                        name: "ugoite.search".into(),
                        arguments: serde_json::json!({"query": "first"}),
                    },
                    ugoite_konase::ModelToolCall {
                        id: "call-2".into(),
                        name: "ugoite.search".into(),
                        arguments: serde_json::json!({"query": "second"}),
                    },
                ],
            }))
            .unwrap()
        else {
            panic!("expected first MCP call");
        };
        assert_eq!(first.arguments["query"], "first");

        let first_request_id = first.request_id.clone();
        let AgentAction::CallMcp(second) = runtime
            .resume(AgentRuntimeInput::McpCompleted(McpResult {
                request_id: first_request_id,
                operation: first.operation.clone(),
                success: false,
                observation: None,
                resources: vec![],
                resource_contents: vec![],
                error: Some("first call failed".into()),
            }))
            .unwrap()
        else {
            panic!("expected second MCP call");
        };
        assert_eq!(second.arguments["query"], "second");

        let first_request_id = first.request_id;
        let second_request_id = second.request_id.clone();
        let AgentAction::CallModel(next_model) = runtime
            .resume(AgentRuntimeInput::McpCompleted(McpResult {
                request_id: second_request_id.clone(),
                operation: second.operation,
                success: true,
                observation: None,
                resources: vec![],
                resource_contents: vec![],
                error: None,
            }))
            .unwrap()
        else {
            panic!("expected one model call after the whole batch");
        };
        assert!(next_model.prompt.contains("first call failed"));
        assert!(next_model.prompt.contains(&first_request_id));
        assert!(next_model.prompt.contains(&second_request_id));
    }

    #[test]
    fn invalid_tool_call_in_batch_fails_before_emitting_any_mcp_call() {
        let mut runtime = RigAgentRuntime::default();
        let AgentAction::CallModel(model) = runtime.start(job(), context()).unwrap() else {
            panic!("expected initial model call");
        };
        let error = runtime
            .resume(AgentRuntimeInput::ModelCompleted(ModelResult {
                request_id: model.request_id,
                text: None,
                tool_calls: vec![
                    ugoite_konase::ModelToolCall {
                        id: "call-1".into(),
                        name: "ugoite.search".into(),
                        arguments: serde_json::json!({"query": "valid"}),
                    },
                    ugoite_konase::ModelToolCall {
                        id: "call-2".into(),
                        name: "ugoite.search".into(),
                        arguments: serde_json::json!(["not", "an", "object"]),
                    },
                ],
            }))
            .expect_err("the whole batch must be validated before execution");

        assert_eq!(error.kind, "invalid_tool_arguments");
        assert!(runtime.pending.is_none());
        assert!(runtime.queued_tools.is_empty());
        assert_eq!(runtime.mcp_sequence, 0);
    }

    #[test]
    fn empty_tool_batch_is_reported_as_a_rig_invariant() {
        let mut runtime = RigAgentRuntime::default();

        let error = runtime
            .queue_tool_calls(vec![])
            .expect_err("an empty Rig tool batch must stop the adapter");

        assert_eq!(error.kind, "rig_invariant");
        assert!(runtime.queued_tools.is_empty());
        assert!(runtime.pending.is_none());
    }

    #[test]
    fn unavailable_tool_does_not_enter_rig_invalid_tool_recovery() {
        let mut runtime = RigAgentRuntime::default();
        let AgentAction::CallModel(model) = runtime.start(job(), context()).unwrap() else {
            panic!("expected initial model call");
        };

        let error = runtime
            .resume(AgentRuntimeInput::ModelCompleted(ModelResult {
                request_id: model.request_id,
                text: None,
                tool_calls: vec![ugoite_konase::ModelToolCall {
                    id: "call-1".into(),
                    name: "ugoite.unknown".into(),
                    arguments: serde_json::json!({}),
                }],
            }))
            .expect_err("the current adapter does not resolve unavailable tools");

        assert_eq!(error.kind, "unknown_tool");
        assert!(runtime.pending.is_none());
    }

    #[test]
    fn mcp_result_becomes_final_completion() {
        let mut runtime = RigAgentRuntime::default();
        let AgentAction::CallModel(request) = runtime.start(job(), context()).unwrap() else {
            panic!("expected initial model call");
        };
        let AgentAction::CallMcp(mcp) = runtime
            .resume(AgentRuntimeInput::ModelCompleted(ModelResult {
                request_id: request.request_id,
                text: None,
                tool_calls: vec![ugoite_konase::ModelToolCall {
                    id: "call-1".into(),
                    name: "ugoite.search".into(),
                    arguments: serde_json::json!({"query": "WebAssembly"}),
                }],
            }))
            .unwrap()
        else {
            panic!("expected MCP call");
        };
        let AgentAction::CallModel(next_model) = runtime
            .resume(AgentRuntimeInput::McpCompleted(McpResult {
                request_id: mcp.request_id,
                operation: mcp.operation,
                success: true,
                observation: Some(Observation {
                    id: "observation-1".into(),
                    kind: ObservationKind::Mcp,
                    summary: "one note found".into(),
                    facts: BTreeMap::new(),
                    resource_references: vec![ResourceReference {
                        uri: "ugoite://entry/1".into(),
                        label: None,
                    }],
                }),
                resources: vec![],
                resource_contents: vec![],
                error: None,
            }))
            .unwrap()
        else {
            panic!("expected second model call");
        };
        let AgentAction::Complete(outcome) = runtime
            .resume(AgentRuntimeInput::ModelCompleted(ModelResult {
                request_id: next_model.request_id,
                text: Some("Found one WebAssembly note.".into()),
                tool_calls: vec![],
            }))
            .unwrap()
        else {
            panic!("expected completion");
        };
        assert_eq!(outcome.job_id, "job-1");
        assert_eq!(outcome.summary, "Found one WebAssembly note.");
        assert!(runtime.run.is_none());
    }
}
