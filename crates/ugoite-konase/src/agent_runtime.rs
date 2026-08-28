use crate::{ContextCapsule, JobSpec, McpResult};
use serde::{Deserialize, Serialize};
use std::fmt;

pub type AgentAction = crate::EffectKind;

/// Replaceable model/tool execution boundary owned by a host adapter.
///
/// Implementations may use a provider framework internally, but the
/// framework's types and serialized state must not cross this boundary.
pub trait AgentRuntime {
    fn start(
        &mut self,
        job: JobSpec,
        context: ContextCapsule,
    ) -> Result<AgentAction, AgentRuntimeError>;

    fn resume(&mut self, input: AgentRuntimeInput) -> Result<AgentAction, AgentRuntimeError>;
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentRuntimeInput {
    McpCompleted(McpResult),
    ConfirmationCompleted(crate::ConfirmationResult),
    HostFailed(crate::HostError),
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
