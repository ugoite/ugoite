//! Portable, client-side control-plane semantics for Konase.
//!
//! This crate intentionally owns no UI, async runtime, network, filesystem,
//! storage, or model-provider implementation. Hosts execute the serializable
//! effects returned by the step function and send resulting events back into
//! the same deterministic state transition function.

mod agent_runtime;
mod context;
mod engine;

pub use agent_runtime::{
    AgentAction, AgentRuntime, AgentRuntimeError, AgentRuntimeInput, ModelMessage, ModelRequest,
    ModelResult, ModelTool, ModelToolCall,
};
pub use context::{
    ContextBuildRequest, ContextBuilder, ContextLimits, MAX_CONTEXT_CAPABILITY_JSON_BYTES,
    MAX_CONTEXT_JSON_BYTES,
};
pub use engine::{
    normalize_state, step, AgentProgress, Capability, CapabilityEffect, ConfirmationRequest,
    ConfirmationResult, ContextCapsule, HostError, Job, JobOutcome, JobRequest, JobSpec, JobStatus,
    KnowledgeOutcome, KonaseEffect, KonaseError, KonaseEvent, KonaseOutput, KonaseState,
    McpRequest, McpResult, Observation, ObservationKind, PendingEffect, ResourceContent,
    ResourceReference, SessionStatus, StepResult, UserRequest, Work, WorkStatus,
    MAX_STATE_JSON_BYTES, MAX_STATE_OBSERVATIONS,
};

/// Version for portable Konase JSON semantics, independent of Ugoite REST.
pub const KONASE_PROTOCOL_VERSION: u32 = 1;
