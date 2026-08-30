use crate::{ContextCapsule, JobSpec, McpResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// A provider-neutral action returned by an [`AgentRuntime`].
///
/// Model and host execution remain outside the Konase engine. A host executes
/// one returned action and feeds the result back through
/// [`AgentRuntimeInput`]. Provider/framework types must not cross this boundary.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentAction {
    CallModel(ModelRequest),
    CallMcp(crate::McpRequest),
    AskConfirmation(crate::ConfirmationRequest),
    Complete(crate::JobOutcome),
}

/// Provider-neutral input supplied to an agent runtime after a host action.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentRuntimeInput {
    ModelCompleted(ModelResult),
    McpCompleted(McpResult),
    ConfirmationCompleted(crate::ConfirmationResult),
    HostFailed(crate::HostError),
}

/// One provider-neutral model request. The host maps this to its configured
/// provider; no provider client or serialized framework state is retained here.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ModelRequest {
    pub request_id: String,
    pub prompt: String,
    #[serde(default)]
    pub history: Vec<ModelMessage>,
    #[serde(default)]
    pub tools: Vec<ModelTool>,
}

/// The small conversation vocabulary required by the host model adapter.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum ModelMessage {
    System {
        content: String,
    },
    User {
        content: String,
    },
    Assistant {
        content: String,
        #[serde(default)]
        tool_calls: Vec<ModelToolCall>,
    },
    Tool {
        call_id: String,
        name: String,
        content: String,
    },
}

/// A model-visible tool definition. The schema remains ordinary JSON so the
/// provider adapter can translate it without leaking provider types upstream.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ModelTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<crate::CapabilityEffect>,
}

/// One tool call emitted by a model response.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ModelToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// A single model turn returned by a host model adapter.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ModelResult {
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ModelToolCall>,
}

/// Replaceable model/tool execution boundary owned by a host adapter.
pub trait AgentRuntime {
    fn start(
        &mut self,
        job: JobSpec,
        context: ContextCapsule,
    ) -> Result<AgentAction, AgentRuntimeError>;

    fn resume(&mut self, input: AgentRuntimeInput) -> Result<AgentAction, AgentRuntimeError>;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentRuntimeError {
    pub kind: String,
    pub message: String,
}

impl AgentRuntimeError {
    pub fn new(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for AgentRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AgentRuntimeError {}
