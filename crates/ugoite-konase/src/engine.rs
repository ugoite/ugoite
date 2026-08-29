use crate::{AgentAction, ContextBuilder, ModelMessage, ModelRequest, ModelTool, ModelToolCall};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;

pub const MAX_STATE_OBSERVATIONS: usize = 64;
pub const MAX_STATE_JSON_BYTES: usize = 64 * 1024;
const MAX_STATE_SUMMARY_CHARS: usize = 1_024;
const MAX_STATE_FACTS: usize = 32;
const MAX_STATE_FACT_CHARS: usize = 512;
const MAX_STATE_RESOURCE_REFERENCES: usize = 16;
const MAX_IDENTIFIER_CHARS: usize = 256;
const MAX_GOAL_CHARS: usize = 4_096;
const MAX_ERROR_KIND_CHARS: usize = 128;
const MAX_ERROR_MESSAGE_CHARS: usize = 2_048;
const MAX_MCP_ARGUMENTS: usize = 32;
const MAX_MCP_VALUE_BYTES: usize = 8 * 1024;
const MAX_SCHEMA_BYTES: usize = 16 * 1024;
const MAX_STATE_ERROR_RESERVE_BYTES: usize = 4 * 1024;
const MAX_MODEL_HISTORY: usize = 32;
const MAX_MODEL_CONTENT_CHARS: usize = 8 * 1024;
const MAX_MODEL_TOOLS: usize = 32;
const MAX_MODEL_TOOL_CALLS: usize = 16;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Work {
    pub id: String,
    pub goal: String,
    pub status: WorkStatus,
    pub job_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_outcome: Option<JobOutcome>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkStatus {
    Working,
    Completed,
    Failed,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct JobSpec {
    pub id: String,
    pub work_id: String,
    pub goal: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_response_schema: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Job {
    pub spec: JobSpec,
    pub status: JobStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy_summary: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    Running,
    WaitingForHost,
    Completed,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JobOutcome {
    pub job_id: String,
    pub summary: String,
    pub meaningful: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationKind {
    User,
    Model,
    Mcp,
    Host,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Observation {
    pub id: String,
    pub kind: ObservationKind,
    pub summary: String,
    #[serde(default)]
    pub facts: BTreeMap<String, String>,
    #[serde(default)]
    pub resource_references: Vec<ResourceReference>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ResourceReference {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ResourceContent {
    pub uri: String,
    pub content: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Capability {
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ContextCapsule {
    pub work_goal: String,
    pub job_goal: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_strategy_summary: Option<String>,
    pub relevant_observations: Vec<Observation>,
    pub available_capabilities: Vec<Capability>,
    pub selected_resource_contents: Vec<ResourceContent>,
    pub safety_hints: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_response_schema: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct UserRequest {
    pub work_id: String,
    pub job_id: String,
    pub goal: String,
    #[serde(default)]
    pub available_capabilities: Vec<Capability>,
    #[serde(default)]
    pub safety_hints: Vec<String>,
    #[serde(default)]
    pub expected_response_schema: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AgentProgress {
    pub job_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation: Option<Observation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<AgentAction>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct McpRequest {
    pub request_id: String,
    pub server: String,
    pub operation: String,
    #[serde(default)]
    pub arguments: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct McpResult {
    pub request_id: String,
    pub operation: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation: Option<Observation>,
    #[serde(default)]
    pub resources: Vec<ResourceReference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConfirmationRequest {
    pub request_id: String,
    pub reason: String,
    pub operation: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConfirmationResult {
    pub request_id: String,
    pub approved: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HostError {
    pub kind: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KonaseEvent {
    UserSubmitted(UserRequest),
    AgentProgress(AgentProgress),
    JobCompleted(JobOutcome),
    McpCompleted(McpResult),
    ConfirmationCompleted(ConfirmationResult),
    HostFailed(HostError),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KonaseEffect {
    StartJob(Box<JobRequest>),
    CallModel(ModelRequest),
    CallMcp(McpRequest),
    AskConfirmation(ConfirmationRequest),
    Emit(KonaseOutput),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct JobRequest {
    pub job: JobSpec,
    pub context: ContextCapsule,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingEffect {
    StartJob,
    CallModel { request_id: String },
    CallMcp { request_id: String },
    AskConfirmation { request_id: String },
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    #[default]
    Idle,
    Working,
    Completed,
    Failed,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct KonaseState {
    pub status: SessionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work: Option<Work>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<Job>,
    #[serde(default)]
    pub observations: Vec<Observation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_effect: Option<PendingEffect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_output: Option<KonaseOutput>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum KonaseOutput {
    WorkStarted { work_id: String, job_id: String },
    ObservationRecorded { observation_id: String },
    McpCompleted { request_id: String, success: bool },
    ConfirmationRequired { request_id: String },
    ConfirmationResolved { request_id: String, approved: bool },
    JobCompleted(JobOutcome),
    HostFailed(HostError),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct KonaseError {
    pub kind: String,
    pub message: String,
}

impl KonaseError {
    fn new(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for KonaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for KonaseError {}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct StepResult {
    pub state: KonaseState,
    pub effects: Vec<KonaseEffect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<KonaseError>,
}

/// Apply one event without performing a host operation.
pub fn step(state: KonaseState, event: KonaseEvent) -> StepResult {
    let mut state = match normalize_state(state) {
        Ok(state) => state,
        Err(error) => return rejected_state(error),
    };
    let original_state = state.clone();
    let mut effects = Vec::new();
    let event = match normalize_event(event) {
        Ok(event) => event,
        Err(error) => return rejected_with_state(original_state, error),
    };
    let result = match event {
        KonaseEvent::UserSubmitted(request) => submit(&mut state, request, &mut effects),
        KonaseEvent::AgentProgress(progress) => progress_event(&mut state, progress, &mut effects),
        KonaseEvent::JobCompleted(outcome) => complete_job(&mut state, outcome, &mut effects),
        KonaseEvent::McpCompleted(result) => mcp_completed(&mut state, result, &mut effects),
        KonaseEvent::ConfirmationCompleted(result) => {
            confirmation_completed(&mut state, result, &mut effects)
        }
        KonaseEvent::HostFailed(error) => host_failed(&mut state, error, &mut effects),
    };

    match result {
        Ok(()) => {
            let result = StepResult {
                state,
                effects,
                error: None,
            };
            match serialized_size(&result) {
                Ok(size) if size <= MAX_STATE_JSON_BYTES => result,
                Ok(_) => rejected_with_state(
                    original_state,
                    KonaseError::new(
                        "output_too_large",
                        "Konase transition output exceeds the protocol limit",
                    ),
                ),
                Err(error) => {
                    rejected_with_state(original_state, KonaseError::new("serialization", error))
                }
            }
        }
        Err(error) => rejected_with_state(original_state, error),
    }
}

fn rejected_state(error: KonaseError) -> StepResult {
    rejected_with_state(KonaseState::default(), error)
}

fn rejected_with_state(state: KonaseState, error: KonaseError) -> StepResult {
    StepResult {
        state,
        effects: Vec::new(),
        error: Some(error),
    }
}

/// Normalize state loaded from an untrusted adapter into the fixed-size
/// representation used by the control plane.
pub fn normalize_state(mut state: KonaseState) -> Result<KonaseState, KonaseError> {
    validate_state(&state)?;
    state.work = state.work.map(normalize_work);
    state.job = state.job.map(normalize_job);
    state.observations = state
        .observations
        .into_iter()
        .map(normalize_observation)
        .collect();
    if state.observations.len() > MAX_STATE_OBSERVATIONS {
        let excess = state.observations.len() - MAX_STATE_OBSERVATIONS;
        state.observations.drain(..excess);
    }
    state.pending_effect = state.pending_effect.map(normalize_pending_effect);
    state.last_output = state.last_output.map(normalize_output);
    let size = serialized_size(&state).map_err(|error| KonaseError::new("serialization", error))?;
    if size > MAX_STATE_JSON_BYTES - MAX_STATE_ERROR_RESERVE_BYTES {
        return Err(KonaseError::new(
            "state_too_large",
            "Konase state exceeds the protocol limit",
        ));
    }
    Ok(state)
}

fn submit(
    state: &mut KonaseState,
    request: UserRequest,
    effects: &mut Vec<KonaseEffect>,
) -> Result<(), KonaseError> {
    validate_non_empty("work_id", &request.work_id)?;
    validate_non_empty("job_id", &request.job_id)?;
    validate_non_empty("goal", &request.goal)?;
    if state.status == SessionStatus::Working {
        return Err(KonaseError::new(
            "work_in_progress",
            "cannot submit a new request while a Work is running",
        ));
    }

    let same_work = state
        .work
        .as_ref()
        .is_some_and(|work| work.id == request.work_id);
    let job_count = if same_work {
        state
            .work
            .as_ref()
            .map_or(1, |work| work.job_count.saturating_add(1))
    } else {
        state.observations.clear();
        1
    };
    let expected_response_schema = request.expected_response_schema.clone();
    let work = Work {
        id: request.work_id.clone(),
        goal: request.goal.clone(),
        status: WorkStatus::Working,
        job_count,
        last_outcome: None,
    };
    let job = Job {
        spec: JobSpec {
            id: request.job_id.clone(),
            work_id: request.work_id.clone(),
            goal: request.goal,
            expected_response_schema: expected_response_schema.clone(),
        },
        status: JobStatus::Pending,
        strategy_summary: None,
    };
    let context = ContextBuilder::default().build(crate::ContextBuildRequest {
        work_goal: work.goal.clone(),
        job_goal: job.spec.goal.clone(),
        current_strategy_summary: None,
        observations: state.observations.clone(),
        available_capabilities: request.available_capabilities,
        selected_resource_contents: vec![],
        safety_hints: request.safety_hints,
        expected_response_schema,
        limits: None,
    });

    state.status = SessionStatus::Working;
    state.work = Some(work);
    state.job = Some(job.clone());
    state.pending_effect = Some(PendingEffect::StartJob);
    let output = KonaseOutput::WorkStarted {
        work_id: request.work_id,
        job_id: request.job_id,
    };
    state.last_output = Some(output.clone());
    effects.push(KonaseEffect::StartJob(Box::new(JobRequest {
        job: job.spec,
        context,
    })));
    effects.push(KonaseEffect::Emit(output));
    Ok(())
}

fn progress_event(
    state: &mut KonaseState,
    progress: AgentProgress,
    effects: &mut Vec<KonaseEffect>,
) -> Result<(), KonaseError> {
    let job_id = progress.job_id.clone();
    let has_action = progress.action.is_some();
    {
        let job = progress_job(state, &job_id, has_action)?;
        job.status = JobStatus::Running;
        job.strategy_summary = progress.strategy_summary;
    }

    if let Some(observation) = progress.observation {
        let output = record_observation(state, observation)?;
        state.last_output = Some(output.clone());
        effects.push(KonaseEffect::Emit(output));
    }
    match progress.action {
        Some(AgentAction::CallModel(request)) => {
            let request_id = request.request_id.clone();
            current_job(state, &job_id)?.status = JobStatus::WaitingForHost;
            state.pending_effect = Some(PendingEffect::CallModel { request_id });
            effects.push(KonaseEffect::CallModel(request));
        }
        Some(AgentAction::CallMcp(request)) => {
            let request_id = request.request_id.clone();
            current_job(state, &job_id)?.status = JobStatus::WaitingForHost;
            state.pending_effect = Some(PendingEffect::CallMcp { request_id });
            effects.push(KonaseEffect::CallMcp(request));
        }
        Some(AgentAction::AskConfirmation(request)) => {
            let request_id = request.request_id.clone();
            current_job(state, &job_id)?.status = JobStatus::WaitingForHost;
            state.pending_effect = Some(PendingEffect::AskConfirmation { request_id });
            let output = KonaseOutput::ConfirmationRequired {
                request_id: request.request_id.clone(),
            };
            state.last_output = Some(output.clone());
            effects.push(KonaseEffect::AskConfirmation(request));
            effects.push(KonaseEffect::Emit(output));
        }
        Some(AgentAction::Complete(outcome)) => complete_job(state, outcome, effects)?,
        None => {
            state.pending_effect = None;
        }
    }
    Ok(())
}

fn complete_job(
    state: &mut KonaseState,
    outcome: JobOutcome,
    effects: &mut Vec<KonaseEffect>,
) -> Result<(), KonaseError> {
    active_job(state, &outcome.job_id)?.status = JobStatus::Completed;
    let work = state
        .work
        .as_mut()
        .ok_or_else(|| KonaseError::new("missing_work", "cannot complete a Job without a Work"))?;
    work.status = WorkStatus::Completed;
    work.last_outcome = Some(outcome.clone());
    state.status = SessionStatus::Completed;
    state.pending_effect = None;
    let output = KonaseOutput::JobCompleted(outcome);
    state.last_output = Some(output.clone());
    effects.push(KonaseEffect::Emit(output));
    Ok(())
}

fn mcp_completed(
    state: &mut KonaseState,
    result: McpResult,
    effects: &mut Vec<KonaseEffect>,
) -> Result<(), KonaseError> {
    waiting_for_host(state)?;
    match state.pending_effect.as_ref() {
        Some(PendingEffect::CallMcp { request_id }) if request_id == &result.request_id => {}
        _ => {
            return Err(KonaseError::new(
                "unexpected_mcp_result",
                format!(
                    "MCP result {} does not match the pending effect",
                    result.request_id
                ),
            ))
        }
    }
    if let Some(mut observation) = result.observation {
        observation.resource_references.extend(result.resources);
        let output = record_observation(state, observation)?;
        effects.push(KonaseEffect::Emit(output.clone()));
        state.last_output = Some(output);
    }
    state
        .job
        .as_mut()
        .expect("waiting_for_host verified")
        .status = JobStatus::Running;
    state.pending_effect = None;
    let output = KonaseOutput::McpCompleted {
        request_id: result.request_id,
        success: result.success,
    };
    state.last_output = Some(output.clone());
    effects.push(KonaseEffect::Emit(output));
    Ok(())
}

fn confirmation_completed(
    state: &mut KonaseState,
    result: ConfirmationResult,
    effects: &mut Vec<KonaseEffect>,
) -> Result<(), KonaseError> {
    waiting_for_host(state)?;
    match state.pending_effect.as_ref() {
        Some(PendingEffect::AskConfirmation { request_id }) if request_id == &result.request_id => {
        }
        _ => {
            return Err(KonaseError::new(
                "unexpected_confirmation",
                format!(
                    "confirmation result {} does not match the pending effect",
                    result.request_id
                ),
            ))
        }
    }
    state
        .job
        .as_mut()
        .expect("waiting_for_host verified")
        .status = JobStatus::Running;
    state.pending_effect = None;
    let output = KonaseOutput::ConfirmationResolved {
        request_id: result.request_id,
        approved: result.approved,
    };
    state.last_output = Some(output.clone());
    effects.push(KonaseEffect::Emit(output));
    Ok(())
}

fn host_failed(
    state: &mut KonaseState,
    error: HostError,
    effects: &mut Vec<KonaseEffect>,
) -> Result<(), KonaseError> {
    ensure_working_session(state)?;
    let job = state.job.as_ref().ok_or_else(|| {
        KonaseError::new(
            "missing_job",
            "received a Host failure without an active Job",
        )
    })?;
    if matches!(job.status, JobStatus::Completed | JobStatus::Failed) {
        return Err(KonaseError::new(
            "terminal_job",
            "received a Host failure for a terminal Job",
        ));
    }
    match (&state.pending_effect, error.request_id.as_deref()) {
        (Some(PendingEffect::CallModel { request_id }), Some(actual))
        | (Some(PendingEffect::CallMcp { request_id }), Some(actual))
        | (Some(PendingEffect::AskConfirmation { request_id }), Some(actual))
            if request_id == actual => {}
        (Some(PendingEffect::StartJob), None) | (None, None) => {}
        (Some(PendingEffect::CallModel { .. }), None)
        | (Some(PendingEffect::CallMcp { .. }), None)
        | (Some(PendingEffect::AskConfirmation { .. }), None)
        | (Some(PendingEffect::StartJob), Some(_))
        | (None, Some(_)) => {
            return Err(KonaseError::new(
                "unexpected_host_failure",
                "host failure does not match the pending effect",
            ))
        }
        (Some(PendingEffect::CallModel { .. }), Some(_))
        | (Some(PendingEffect::CallMcp { .. }), Some(_))
        | (Some(PendingEffect::AskConfirmation { .. }), Some(_)) => {
            return Err(KonaseError::new(
                "unexpected_host_failure",
                "host failure request does not match the pending effect",
            ))
        }
    }
    state.job.as_mut().expect("active job verified").status = JobStatus::Failed;
    if let Some(work) = state.work.as_mut() {
        work.status = WorkStatus::Failed;
    }
    state.status = SessionStatus::Failed;
    state.pending_effect = None;
    let output = KonaseOutput::HostFailed(error);
    state.last_output = Some(output.clone());
    effects.push(KonaseEffect::Emit(output));
    Ok(())
}

fn current_job<'a>(state: &'a mut KonaseState, job_id: &str) -> Result<&'a mut Job, KonaseError> {
    let job = state
        .job
        .as_mut()
        .ok_or_else(|| KonaseError::new("missing_job", "event requires an active Job"))?;
    if job.spec.id != job_id {
        return Err(KonaseError::new(
            "stale_job",
            format!(
                "event references Job {job_id}, but the active Job is {}",
                job.spec.id
            ),
        ));
    }
    Ok(job)
}

fn ensure_working_session(state: &KonaseState) -> Result<(), KonaseError> {
    if state.status != SessionStatus::Working {
        return Err(KonaseError::new(
            "inactive_session",
            "event requires a working session",
        ));
    }
    if state
        .work
        .as_ref()
        .is_none_or(|work| work.status != WorkStatus::Working)
    {
        return Err(KonaseError::new(
            "inactive_work",
            "event requires an active Work",
        ));
    }
    Ok(())
}

fn active_job<'a>(state: &'a mut KonaseState, job_id: &str) -> Result<&'a mut Job, KonaseError> {
    if state.job.is_none() {
        return Err(KonaseError::new(
            "missing_job",
            "event requires an active Job",
        ));
    }
    if state
        .job
        .as_ref()
        .is_some_and(|job| matches!(job.status, JobStatus::Completed | JobStatus::Failed))
    {
        return Err(KonaseError::new(
            "terminal_job",
            "event references a terminal Job",
        ));
    }
    ensure_working_session(state)?;
    let job = current_job(state, job_id)?;
    match job.status {
        JobStatus::Pending | JobStatus::Running => Ok(job),
        JobStatus::WaitingForHost => Err(KonaseError::new(
            "job_waiting_for_host",
            "event cannot advance a Job while a host effect is pending",
        )),
        JobStatus::Completed | JobStatus::Failed => Err(KonaseError::new(
            "terminal_job",
            "event references a terminal Job",
        )),
    }
}

/// Accept the next runtime decision after a host has completed a model or
/// other host effect. Model results are intentionally runtime inputs rather
/// than Konase events, so the following AgentProgress carries the next action
/// and advances the waiting Job to its next pending effect.
fn progress_job<'a>(
    state: &'a mut KonaseState,
    job_id: &str,
    has_action: bool,
) -> Result<&'a mut Job, KonaseError> {
    if state
        .job
        .as_ref()
        .is_some_and(|job| matches!(job.status, JobStatus::Completed | JobStatus::Failed))
    {
        return Err(KonaseError::new(
            "terminal_job",
            "event references a terminal Job",
        ));
    }
    ensure_working_session(state)?;
    let job = current_job(state, job_id)?;
    match job.status {
        JobStatus::Pending | JobStatus::Running => Ok(job),
        JobStatus::WaitingForHost if has_action => Ok(job),
        JobStatus::WaitingForHost => Err(KonaseError::new(
            "job_waiting_for_host",
            "runtime progress must carry the next action while a host effect is pending",
        )),
        JobStatus::Completed | JobStatus::Failed => Err(KonaseError::new(
            "terminal_job",
            "event references a terminal Job",
        )),
    }
}

fn waiting_for_host(state: &KonaseState) -> Result<(), KonaseError> {
    ensure_working_session(state)?;
    if state
        .job
        .as_ref()
        .is_none_or(|job| job.status != JobStatus::WaitingForHost)
    {
        return Err(KonaseError::new(
            "job_not_waiting_for_host",
            "event requires a Job waiting for a host effect",
        ));
    }
    Ok(())
}

fn validate_state(state: &KonaseState) -> Result<(), KonaseError> {
    if state.observations.len() > MAX_STATE_OBSERVATIONS {
        return Err(KonaseError::new(
            "state_too_large",
            "Konase state contains too many observations",
        ));
    }
    if let Some(work) = &state.work {
        validate_identifier("work.id", &work.id)?;
        validate_text("work.goal", &work.goal, MAX_GOAL_CHARS)?;
        if let Some(outcome) = &work.last_outcome {
            validate_job_outcome(outcome)?;
        }
    }
    if let Some(job) = &state.job {
        validate_job_spec(&job.spec)?;
        if let Some(summary) = &job.strategy_summary {
            validate_text("job.strategy_summary", summary, MAX_STATE_SUMMARY_CHARS)?;
        }
    }
    for observation in &state.observations {
        validate_observation(observation)?;
    }
    if let Some(pending) = &state.pending_effect {
        validate_pending_effect(pending)?;
    }
    if let Some(output) = &state.last_output {
        validate_output(output)?;
    }

    match state.status {
        SessionStatus::Idle => {
            if state.work.is_some() || state.job.is_some() || state.pending_effect.is_some() {
                return invalid_state("idle state must not contain active execution data");
            }
        }
        SessionStatus::Working => {
            let (Some(work), Some(job)) = (&state.work, &state.job) else {
                return invalid_state("working state requires a Work and Job");
            };
            if work.status != WorkStatus::Working {
                return invalid_state("working state requires a working Work");
            }
            if job.spec.work_id != work.id {
                return invalid_state("Job does not belong to the active Work");
            }
            match (&job.status, &state.pending_effect) {
                (JobStatus::Pending, Some(PendingEffect::StartJob))
                | (JobStatus::Running, None)
                | (JobStatus::WaitingForHost, Some(PendingEffect::CallModel { .. }))
                | (JobStatus::WaitingForHost, Some(PendingEffect::CallMcp { .. }))
                | (JobStatus::WaitingForHost, Some(PendingEffect::AskConfirmation { .. })) => {}
                _ => return invalid_state("Job status and pending effect are inconsistent"),
            }
        }
        SessionStatus::Completed => {
            if state
                .work
                .as_ref()
                .is_none_or(|work| work.status != WorkStatus::Completed)
                || state
                    .job
                    .as_ref()
                    .is_none_or(|job| job.status != JobStatus::Completed)
                || state.pending_effect.is_some()
            {
                return invalid_state("completed state contains non-terminal execution data");
            }
        }
        SessionStatus::Failed => {
            if state
                .work
                .as_ref()
                .is_none_or(|work| work.status != WorkStatus::Failed)
                || state
                    .job
                    .as_ref()
                    .is_none_or(|job| job.status != JobStatus::Failed)
                || state.pending_effect.is_some()
            {
                return invalid_state("failed state contains non-terminal execution data");
            }
        }
    }
    Ok(())
}

fn validate_event(event: &KonaseEvent) -> Result<(), KonaseError> {
    match event {
        KonaseEvent::UserSubmitted(request) => {
            validate_identifier("work_id", &request.work_id)?;
            validate_identifier("job_id", &request.job_id)?;
            validate_text("goal", &request.goal, MAX_GOAL_CHARS)?;
            validate_capabilities(&request.available_capabilities)?;
            validate_hints(&request.safety_hints)?;
            validate_schema(&request.expected_response_schema)?;
        }
        KonaseEvent::AgentProgress(progress) => {
            validate_identifier("job_id", &progress.job_id)?;
            if let Some(summary) = &progress.strategy_summary {
                validate_text("strategy_summary", summary, MAX_STATE_SUMMARY_CHARS)?;
            }
            if let Some(observation) = &progress.observation {
                validate_observation(observation)?;
            }
            if let Some(action) = &progress.action {
                validate_agent_action(action)?;
            }
        }
        KonaseEvent::JobCompleted(outcome) => validate_job_outcome(outcome)?,
        KonaseEvent::McpCompleted(result) => validate_mcp_result(result)?,
        KonaseEvent::ConfirmationCompleted(result) => {
            validate_identifier("confirmation.request_id", &result.request_id)?;
        }
        KonaseEvent::HostFailed(error) => validate_host_error(error)?,
    }
    Ok(())
}

fn validate_job_spec(spec: &JobSpec) -> Result<(), KonaseError> {
    validate_identifier("job.id", &spec.id)?;
    validate_identifier("job.work_id", &spec.work_id)?;
    validate_text("job.goal", &spec.goal, MAX_GOAL_CHARS)?;
    validate_schema(&spec.expected_response_schema)
}

fn validate_job_outcome(outcome: &JobOutcome) -> Result<(), KonaseError> {
    validate_identifier("outcome.job_id", &outcome.job_id)?;
    validate_text("outcome.summary", &outcome.summary, MAX_STATE_SUMMARY_CHARS)
}

fn validate_observation(observation: &Observation) -> Result<(), KonaseError> {
    validate_identifier("observation.id", &observation.id)?;
    validate_text(
        "observation.summary",
        &observation.summary,
        MAX_STATE_SUMMARY_CHARS,
    )?;
    if observation.facts.len() > MAX_STATE_FACTS {
        return invalid_state("observation contains too many facts");
    }
    for (key, value) in &observation.facts {
        validate_identifier("observation.fact.key", key)?;
        validate_text("observation.fact.value", value, MAX_STATE_FACT_CHARS)?;
    }
    if observation.resource_references.len() > MAX_STATE_RESOURCE_REFERENCES {
        return invalid_state("observation contains too many resource references");
    }
    for reference in &observation.resource_references {
        validate_resource_reference(reference)?;
    }
    Ok(())
}

fn validate_resource_reference(reference: &ResourceReference) -> Result<(), KonaseError> {
    validate_text("resource.uri", &reference.uri, MAX_STATE_SUMMARY_CHARS)?;
    if let Some(label) = &reference.label {
        validate_identifier("resource.label", label)?;
    }
    Ok(())
}

fn validate_capabilities(capabilities: &[Capability]) -> Result<(), KonaseError> {
    if capabilities.len() > 32 {
        return invalid_state("request contains too many capabilities");
    }
    for capability in capabilities {
        validate_identifier("capability.name", &capability.name)?;
        validate_text(
            "capability.description",
            &capability.description,
            MAX_STATE_SUMMARY_CHARS,
        )?;
    }
    Ok(())
}

fn validate_hints(hints: &[String]) -> Result<(), KonaseError> {
    if hints.len() > 8 {
        return invalid_state("request contains too many safety hints");
    }
    for hint in hints {
        validate_text("safety_hint", hint, MAX_STATE_SUMMARY_CHARS)?;
    }
    Ok(())
}

fn validate_schema(schema: &Option<Value>) -> Result<(), KonaseError> {
    if let Some(schema) = schema {
        let size =
            serialized_size(schema).map_err(|error| KonaseError::new("serialization", error))?;
        if size > MAX_SCHEMA_BYTES {
            return invalid_state("response schema exceeds the protocol limit");
        }
    }
    Ok(())
}

fn validate_agent_action(action: &AgentAction) -> Result<(), KonaseError> {
    match action {
        AgentAction::CallModel(request) => validate_model_request(request),
        AgentAction::CallMcp(request) => validate_mcp_request(request),
        AgentAction::AskConfirmation(request) => validate_confirmation_request(request),
        AgentAction::Complete(outcome) => validate_job_outcome(outcome),
    }
}

fn validate_model_request(request: &ModelRequest) -> Result<(), KonaseError> {
    validate_identifier("model.request_id", &request.request_id)?;
    validate_text("model.prompt", &request.prompt, MAX_MODEL_CONTENT_CHARS)?;
    if request.history.len() > MAX_MODEL_HISTORY {
        return invalid_state("model request contains too much history");
    }
    for message in &request.history {
        validate_model_message(message)?;
    }
    if request.tools.len() > MAX_MODEL_TOOLS {
        return invalid_state("model request contains too many tools");
    }
    for tool in &request.tools {
        validate_model_tool(tool)?;
    }
    Ok(())
}

fn validate_model_message(message: &ModelMessage) -> Result<(), KonaseError> {
    match message {
        ModelMessage::System { content }
        | ModelMessage::User { content }
        | ModelMessage::Tool { content, .. } => {
            validate_text("model.message.content", content, MAX_MODEL_CONTENT_CHARS)?;
        }
        ModelMessage::Assistant {
            content,
            tool_calls,
        } => {
            validate_text("model.message.content", content, MAX_MODEL_CONTENT_CHARS)?;
            if tool_calls.len() > MAX_MODEL_TOOL_CALLS {
                return invalid_state("model message contains too many tool calls");
            }
            for call in tool_calls {
                validate_model_tool_call(call)?;
            }
        }
    }
    if let ModelMessage::Tool { call_id, name, .. } = message {
        validate_identifier("model.message.call_id", call_id)?;
        validate_identifier("model.message.name", name)?;
    }
    Ok(())
}

fn validate_model_tool(tool: &ModelTool) -> Result<(), KonaseError> {
    validate_identifier("model.tool.name", &tool.name)?;
    validate_text(
        "model.tool.description",
        &tool.description,
        MAX_MODEL_CONTENT_CHARS,
    )?;
    validate_schema(&Some(tool.input_schema.clone()))
}

fn validate_model_tool_call(call: &ModelToolCall) -> Result<(), KonaseError> {
    validate_identifier("model.tool_call.id", &call.id)?;
    validate_identifier("model.tool_call.name", &call.name)?;
    if serialized_size(&call.arguments).map_err(|error| KonaseError::new("serialization", error))?
        > MAX_MCP_VALUE_BYTES
    {
        return invalid_state("model tool call arguments exceed the protocol limit");
    }
    Ok(())
}

fn validate_mcp_request(request: &McpRequest) -> Result<(), KonaseError> {
    validate_identifier("mcp.request_id", &request.request_id)?;
    validate_identifier("mcp.server", &request.server)?;
    validate_identifier("mcp.operation", &request.operation)?;
    if request.arguments.len() > MAX_MCP_ARGUMENTS {
        return invalid_state("MCP request contains too many arguments");
    }
    for (key, value) in &request.arguments {
        validate_identifier("mcp.argument.key", key)?;
        if serialized_size(value).map_err(|error| KonaseError::new("serialization", error))?
            > MAX_MCP_VALUE_BYTES
        {
            return invalid_state("MCP argument exceeds the protocol limit");
        }
    }
    Ok(())
}

fn validate_mcp_result(result: &McpResult) -> Result<(), KonaseError> {
    validate_identifier("mcp.result.request_id", &result.request_id)?;
    validate_identifier("mcp.result.operation", &result.operation)?;
    if let Some(observation) = &result.observation {
        validate_observation(observation)?;
    }
    if result.resources.len() > MAX_STATE_RESOURCE_REFERENCES {
        return invalid_state("MCP result contains too many resource references");
    }
    for resource in &result.resources {
        validate_resource_reference(resource)?;
    }
    if let Some(error) = &result.error {
        validate_text("mcp.result.error", error, MAX_ERROR_MESSAGE_CHARS)?;
    }
    Ok(())
}

fn validate_confirmation_request(request: &ConfirmationRequest) -> Result<(), KonaseError> {
    validate_identifier("confirmation.request_id", &request.request_id)?;
    validate_text(
        "confirmation.reason",
        &request.reason,
        MAX_ERROR_MESSAGE_CHARS,
    )?;
    validate_identifier("confirmation.operation", &request.operation)
}

fn validate_host_error(error: &HostError) -> Result<(), KonaseError> {
    validate_text("host_error.kind", &error.kind, MAX_ERROR_KIND_CHARS)?;
    validate_text(
        "host_error.message",
        &error.message,
        MAX_ERROR_MESSAGE_CHARS,
    )?;
    if let Some(request_id) = &error.request_id {
        validate_identifier("host_error.request_id", request_id)?;
    }
    Ok(())
}

fn validate_pending_effect(pending: &PendingEffect) -> Result<(), KonaseError> {
    match pending {
        PendingEffect::StartJob => Ok(()),
        PendingEffect::CallModel { request_id }
        | PendingEffect::CallMcp { request_id }
        | PendingEffect::AskConfirmation { request_id } => {
            validate_identifier("pending.request_id", request_id)
        }
    }
}

fn validate_output(output: &KonaseOutput) -> Result<(), KonaseError> {
    match output {
        KonaseOutput::WorkStarted { work_id, job_id } => {
            validate_identifier("output.work_id", work_id)?;
            validate_identifier("output.job_id", job_id)
        }
        KonaseOutput::ObservationRecorded { observation_id } => {
            validate_identifier("output.observation_id", observation_id)
        }
        KonaseOutput::McpCompleted { request_id, .. }
        | KonaseOutput::ConfirmationRequired { request_id }
        | KonaseOutput::ConfirmationResolved { request_id, .. } => {
            validate_identifier("output.request_id", request_id)
        }
        KonaseOutput::JobCompleted(outcome) => validate_job_outcome(outcome),
        KonaseOutput::HostFailed(error) => validate_host_error(error),
    }
}

fn validate_identifier(field: &str, value: &str) -> Result<(), KonaseError> {
    validate_text(field, value, MAX_IDENTIFIER_CHARS)
}

fn validate_text(field: &str, value: &str, max_chars: usize) -> Result<(), KonaseError> {
    if value.chars().count() > max_chars {
        return Err(KonaseError::new(
            "input_too_large",
            format!("{field} exceeds the {max_chars}-character limit"),
        ));
    }
    Ok(())
}

fn invalid_state(message: &str) -> Result<(), KonaseError> {
    Err(KonaseError::new("invalid_state", message))
}

fn serialized_size<T: Serialize>(value: &T) -> Result<usize, String> {
    serde_json::to_vec(value)
        .map(|serialized| serialized.len())
        .map_err(|error| error.to_string())
}

fn normalize_event(event: KonaseEvent) -> Result<KonaseEvent, KonaseError> {
    validate_event(&event)?;
    Ok(match event {
        KonaseEvent::UserSubmitted(request) => {
            KonaseEvent::UserSubmitted(normalize_user_request(request))
        }
        KonaseEvent::AgentProgress(progress) => {
            KonaseEvent::AgentProgress(normalize_agent_progress(progress))
        }
        KonaseEvent::JobCompleted(outcome) => {
            KonaseEvent::JobCompleted(normalize_job_outcome(outcome))
        }
        KonaseEvent::McpCompleted(result) => {
            KonaseEvent::McpCompleted(normalize_mcp_result(result))
        }
        KonaseEvent::ConfirmationCompleted(result) => {
            KonaseEvent::ConfirmationCompleted(ConfirmationResult {
                request_id: bound(result.request_id, MAX_IDENTIFIER_CHARS),
                approved: result.approved,
            })
        }
        KonaseEvent::HostFailed(error) => KonaseEvent::HostFailed(normalize_host_error(error)),
    })
}

fn normalize_user_request(mut request: UserRequest) -> UserRequest {
    request.work_id = bound(request.work_id, MAX_IDENTIFIER_CHARS);
    request.job_id = bound(request.job_id, MAX_IDENTIFIER_CHARS);
    request.goal = bound(request.goal, MAX_GOAL_CHARS);
    request.available_capabilities = request
        .available_capabilities
        .into_iter()
        .map(normalize_capability)
        .take(32)
        .collect();
    request.safety_hints = bound_vec(request.safety_hints, 8, MAX_STATE_SUMMARY_CHARS);
    request.expected_response_schema =
        bound_value(request.expected_response_schema, MAX_SCHEMA_BYTES);
    request
}

fn normalize_agent_progress(mut progress: AgentProgress) -> AgentProgress {
    progress.job_id = bound(progress.job_id, MAX_IDENTIFIER_CHARS);
    progress.strategy_summary = progress
        .strategy_summary
        .map(|summary| bound(summary, MAX_STATE_SUMMARY_CHARS));
    progress.observation = progress.observation.map(normalize_observation);
    progress.action = progress.action.map(normalize_agent_action);
    progress
}

fn normalize_agent_action(action: AgentAction) -> AgentAction {
    match action {
        AgentAction::CallModel(request) => AgentAction::CallModel(normalize_model_request(request)),
        AgentAction::CallMcp(request) => AgentAction::CallMcp(normalize_mcp_request(request)),
        AgentAction::AskConfirmation(request) => {
            AgentAction::AskConfirmation(normalize_confirmation_request(request))
        }
        AgentAction::Complete(outcome) => AgentAction::Complete(normalize_job_outcome(outcome)),
    }
}

fn normalize_model_request(mut request: ModelRequest) -> ModelRequest {
    request.request_id = bound(request.request_id, MAX_IDENTIFIER_CHARS);
    request.prompt = bound(request.prompt, MAX_MODEL_CONTENT_CHARS);
    request.history = request
        .history
        .into_iter()
        .take(MAX_MODEL_HISTORY)
        .map(normalize_model_message)
        .collect();
    request.tools = request
        .tools
        .into_iter()
        .take(MAX_MODEL_TOOLS)
        .map(normalize_model_tool)
        .collect();
    request
}

fn normalize_model_message(message: ModelMessage) -> ModelMessage {
    match message {
        ModelMessage::System { content } => ModelMessage::System {
            content: bound(content, MAX_MODEL_CONTENT_CHARS),
        },
        ModelMessage::User { content } => ModelMessage::User {
            content: bound(content, MAX_MODEL_CONTENT_CHARS),
        },
        ModelMessage::Assistant {
            content,
            tool_calls,
        } => ModelMessage::Assistant {
            content: bound(content, MAX_MODEL_CONTENT_CHARS),
            tool_calls: tool_calls
                .into_iter()
                .take(MAX_MODEL_TOOL_CALLS)
                .map(normalize_model_tool_call)
                .collect(),
        },
        ModelMessage::Tool {
            call_id,
            name,
            content,
        } => ModelMessage::Tool {
            call_id: bound(call_id, MAX_IDENTIFIER_CHARS),
            name: bound(name, MAX_IDENTIFIER_CHARS),
            content: bound(content, MAX_MODEL_CONTENT_CHARS),
        },
    }
}

fn normalize_model_tool(mut tool: ModelTool) -> ModelTool {
    tool.name = bound(tool.name, MAX_IDENTIFIER_CHARS);
    tool.description = bound(tool.description, MAX_MODEL_CONTENT_CHARS);
    tool.input_schema = bound_value(Some(tool.input_schema), MAX_SCHEMA_BYTES)
        .unwrap_or_else(|| Value::Object(Default::default()));
    tool
}

fn normalize_model_tool_call(mut call: ModelToolCall) -> ModelToolCall {
    call.id = bound(call.id, MAX_IDENTIFIER_CHARS);
    call.name = bound(call.name, MAX_IDENTIFIER_CHARS);
    call.arguments = bound_value(Some(call.arguments), MAX_MCP_VALUE_BYTES)
        .unwrap_or_else(|| Value::Object(Default::default()));
    call
}

fn normalize_mcp_request(mut request: McpRequest) -> McpRequest {
    request.request_id = bound(request.request_id, MAX_IDENTIFIER_CHARS);
    request.server = bound(request.server, MAX_IDENTIFIER_CHARS);
    request.operation = bound(request.operation, MAX_IDENTIFIER_CHARS);
    request.arguments = request
        .arguments
        .into_iter()
        .take(MAX_MCP_ARGUMENTS)
        .filter_map(|(key, value)| {
            bound_value(Some(value), MAX_MCP_VALUE_BYTES)
                .map(|value| (bound(key, MAX_IDENTIFIER_CHARS), value))
        })
        .collect();
    request
}

fn normalize_mcp_result(mut result: McpResult) -> McpResult {
    result.request_id = bound(result.request_id, MAX_IDENTIFIER_CHARS);
    result.operation = bound(result.operation, MAX_IDENTIFIER_CHARS);
    result.observation = result.observation.map(normalize_observation);
    result.resources = result
        .resources
        .into_iter()
        .map(normalize_resource_reference)
        .take(MAX_STATE_RESOURCE_REFERENCES)
        .collect();
    result.error = result
        .error
        .map(|error| bound(error, MAX_ERROR_MESSAGE_CHARS));
    result
}

fn normalize_confirmation_request(mut request: ConfirmationRequest) -> ConfirmationRequest {
    request.request_id = bound(request.request_id, MAX_IDENTIFIER_CHARS);
    request.reason = bound(request.reason, MAX_ERROR_MESSAGE_CHARS);
    request.operation = bound(request.operation, MAX_IDENTIFIER_CHARS);
    request
}

fn normalize_host_error(mut error: HostError) -> HostError {
    error.kind = bound(error.kind, MAX_ERROR_KIND_CHARS);
    error.message = bound(error.message, MAX_ERROR_MESSAGE_CHARS);
    error.request_id = error
        .request_id
        .map(|request_id| bound(request_id, MAX_IDENTIFIER_CHARS));
    error
}

fn normalize_work(mut work: Work) -> Work {
    work.id = bound(work.id, MAX_IDENTIFIER_CHARS);
    work.goal = bound(work.goal, MAX_GOAL_CHARS);
    work.last_outcome = work.last_outcome.map(normalize_job_outcome);
    work
}

fn normalize_job(mut job: Job) -> Job {
    job.spec = normalize_job_spec(job.spec);
    job.strategy_summary = job
        .strategy_summary
        .map(|summary| bound(summary, MAX_STATE_SUMMARY_CHARS));
    job
}

fn normalize_job_spec(mut spec: JobSpec) -> JobSpec {
    spec.id = bound(spec.id, MAX_IDENTIFIER_CHARS);
    spec.work_id = bound(spec.work_id, MAX_IDENTIFIER_CHARS);
    spec.goal = bound(spec.goal, MAX_GOAL_CHARS);
    spec.expected_response_schema = bound_value(spec.expected_response_schema, MAX_SCHEMA_BYTES);
    spec
}

fn normalize_job_outcome(mut outcome: JobOutcome) -> JobOutcome {
    outcome.job_id = bound(outcome.job_id, MAX_IDENTIFIER_CHARS);
    outcome.summary = bound(outcome.summary, MAX_STATE_SUMMARY_CHARS);
    outcome
}

fn normalize_pending_effect(pending: PendingEffect) -> PendingEffect {
    match pending {
        PendingEffect::StartJob => PendingEffect::StartJob,
        PendingEffect::CallModel { request_id } => PendingEffect::CallModel {
            request_id: bound(request_id, MAX_IDENTIFIER_CHARS),
        },
        PendingEffect::CallMcp { request_id } => PendingEffect::CallMcp {
            request_id: bound(request_id, MAX_IDENTIFIER_CHARS),
        },
        PendingEffect::AskConfirmation { request_id } => PendingEffect::AskConfirmation {
            request_id: bound(request_id, MAX_IDENTIFIER_CHARS),
        },
    }
}

fn normalize_output(output: KonaseOutput) -> KonaseOutput {
    match output {
        KonaseOutput::WorkStarted { work_id, job_id } => KonaseOutput::WorkStarted {
            work_id: bound(work_id, MAX_IDENTIFIER_CHARS),
            job_id: bound(job_id, MAX_IDENTIFIER_CHARS),
        },
        KonaseOutput::ObservationRecorded { observation_id } => KonaseOutput::ObservationRecorded {
            observation_id: bound(observation_id, MAX_IDENTIFIER_CHARS),
        },
        KonaseOutput::McpCompleted {
            request_id,
            success,
        } => KonaseOutput::McpCompleted {
            request_id: bound(request_id, MAX_IDENTIFIER_CHARS),
            success,
        },
        KonaseOutput::ConfirmationRequired { request_id } => KonaseOutput::ConfirmationRequired {
            request_id: bound(request_id, MAX_IDENTIFIER_CHARS),
        },
        KonaseOutput::ConfirmationResolved {
            request_id,
            approved,
        } => KonaseOutput::ConfirmationResolved {
            request_id: bound(request_id, MAX_IDENTIFIER_CHARS),
            approved,
        },
        KonaseOutput::JobCompleted(outcome) => {
            KonaseOutput::JobCompleted(normalize_job_outcome(outcome))
        }
        KonaseOutput::HostFailed(error) => KonaseOutput::HostFailed(normalize_host_error(error)),
    }
}

fn normalize_observation(mut observation: Observation) -> Observation {
    observation.id = bound(observation.id, MAX_IDENTIFIER_CHARS);
    observation.summary = bound(observation.summary, MAX_STATE_SUMMARY_CHARS);
    observation.facts = observation
        .facts
        .into_iter()
        .take(MAX_STATE_FACTS)
        .map(|(key, value)| {
            (
                bound(key, MAX_IDENTIFIER_CHARS),
                bound(value, MAX_STATE_FACT_CHARS),
            )
        })
        .collect();
    observation.resource_references = observation
        .resource_references
        .into_iter()
        .map(normalize_resource_reference)
        .take(MAX_STATE_RESOURCE_REFERENCES)
        .collect();
    observation
}

fn normalize_resource_reference(mut reference: ResourceReference) -> ResourceReference {
    reference.uri = bound(reference.uri, MAX_STATE_SUMMARY_CHARS);
    reference.label = reference
        .label
        .map(|label| bound(label, MAX_IDENTIFIER_CHARS));
    reference
}

fn normalize_capability(mut capability: Capability) -> Capability {
    capability.name = bound(capability.name, MAX_IDENTIFIER_CHARS);
    capability.description = bound(capability.description, MAX_STATE_SUMMARY_CHARS);
    capability
}

fn bound_vec(values: Vec<String>, max_count: usize, max_chars: usize) -> Vec<String> {
    values
        .into_iter()
        .take(max_count)
        .map(|value| bound(value, max_chars))
        .collect()
}

fn bound(value: String, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn bound_value(value: Option<Value>, max_bytes: usize) -> Option<Value> {
    value.filter(|value| {
        serde_json::to_vec(value)
            .map(|serialized| serialized.len() <= max_bytes)
            .unwrap_or(false)
    })
}

fn record_observation(
    state: &mut KonaseState,
    observation: Observation,
) -> Result<KonaseOutput, KonaseError> {
    let observation = normalize_observation(observation);
    validate_non_empty("observation.id", &observation.id)?;
    state.observations.push(observation.clone());
    if state.observations.len() > MAX_STATE_OBSERVATIONS {
        let excess = state.observations.len() - MAX_STATE_OBSERVATIONS;
        state.observations.drain(..excess);
    }
    Ok(KonaseOutput::ObservationRecorded {
        observation_id: observation.id,
    })
}

fn validate_non_empty(field: &str, value: &str) -> Result<(), KonaseError> {
    if value.trim().is_empty() {
        Err(KonaseError::new(
            "invalid_input",
            format!("{field} must not be empty"),
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> UserRequest {
        UserRequest {
            work_id: "work-1".into(),
            job_id: "job-1".into(),
            goal: "find and summarize notes".into(),
            available_capabilities: vec![Capability {
                name: "ugoite.search".into(),
                description: "search knowledge".into(),
            }],
            safety_hints: vec!["save only with confirmation".into()],
            expected_response_schema: None,
        }
    }

    fn observation(id: &str) -> Observation {
        Observation {
            id: id.into(),
            kind: ObservationKind::Mcp,
            summary: "found a matching entry".into(),
            facts: BTreeMap::new(),
            resource_references: vec![ResourceReference {
                uri: "ugoite://entry/1".into(),
                label: Some("Note".into()),
            }],
        }
    }

    #[test]
    fn event_sequence_is_deterministic_and_keeps_io_in_effects() {
        let first = step(
            KonaseState::default(),
            KonaseEvent::UserSubmitted(request()),
        );
        let second = step(
            KonaseState::default(),
            KonaseEvent::UserSubmitted(request()),
        );
        assert_eq!(first, second);
        assert_eq!(first.state.status, SessionStatus::Working);
        assert!(matches!(
            first.effects.first(),
            Some(KonaseEffect::StartJob(_))
        ));
        assert!(first.state.observations.is_empty());
    }

    #[test]
    fn mcp_result_becomes_bounded_observation_and_job_can_complete() {
        let started = step(
            KonaseState::default(),
            KonaseEvent::UserSubmitted(request()),
        );
        let progress = step(
            started.state,
            KonaseEvent::AgentProgress(AgentProgress {
                job_id: "job-1".into(),
                strategy_summary: Some("search then read one resource".into()),
                observation: None,
                action: Some(AgentAction::CallMcp(McpRequest {
                    request_id: "mcp-1".into(),
                    server: "ugoite".into(),
                    operation: "ugoite.search".into(),
                    arguments: BTreeMap::new(),
                })),
            }),
        );
        assert!(matches!(
            progress.effects.first(),
            Some(KonaseEffect::CallMcp(McpRequest { request_id, .. })) if request_id == "mcp-1"
        ));
        let result = step(
            progress.state,
            KonaseEvent::McpCompleted(McpResult {
                request_id: "mcp-1".into(),
                operation: "ugoite.search".into(),
                success: true,
                observation: Some(observation("observation-1")),
                resources: vec![],
                error: None,
            }),
        );
        assert_eq!(result.state.observations.len(), 1);
        assert_eq!(
            result.state.job.as_ref().unwrap().status,
            JobStatus::Running
        );
        let completed = step(
            result.state,
            KonaseEvent::JobCompleted(JobOutcome {
                job_id: "job-1".into(),
                summary: "one note found".into(),
                meaningful: true,
            }),
        );
        assert_eq!(completed.state.status, SessionStatus::Completed);
        assert_eq!(completed.state.work.unwrap().status, WorkStatus::Completed);
    }

    #[test]
    fn state_observations_are_bounded() {
        let mut state = step(
            KonaseState::default(),
            KonaseEvent::UserSubmitted(request()),
        )
        .state;
        for id in 0..(MAX_STATE_OBSERVATIONS + 10) {
            state = step(
                state,
                KonaseEvent::AgentProgress(AgentProgress {
                    job_id: "job-1".into(),
                    strategy_summary: None,
                    observation: Some(observation(&format!("observation-{id}"))),
                    action: None,
                }),
            )
            .state;
        }
        assert_eq!(state.observations.len(), MAX_STATE_OBSERVATIONS);
        assert_eq!(state.observations[0].id, "observation-10");
    }

    #[test]
    fn invalid_event_returns_diagnostic_without_mutating_state() {
        let state = KonaseState::default();
        let result = step(
            state.clone(),
            KonaseEvent::JobCompleted(JobOutcome {
                job_id: "missing".into(),
                summary: "never started".into(),
                meaningful: false,
            }),
        );
        assert_eq!(result.state, state);
        assert_eq!(result.effects, Vec::new());
        assert_eq!(result.error.unwrap().kind, "missing_job");
    }

    #[test]
    fn terminal_jobs_reject_stale_progress() {
        let started = step(
            KonaseState::default(),
            KonaseEvent::UserSubmitted(request()),
        );
        let completed = step(
            started.state,
            KonaseEvent::JobCompleted(JobOutcome {
                job_id: "job-1".into(),
                summary: "done".into(),
                meaningful: true,
            }),
        );
        let result = step(
            completed.state.clone(),
            KonaseEvent::AgentProgress(AgentProgress {
                job_id: "job-1".into(),
                strategy_summary: Some("stale".into()),
                observation: Some(observation("stale-observation")),
                action: None,
            }),
        );

        assert_eq!(result.state, completed.state);
        assert_eq!(result.effects, Vec::new());
        assert_eq!(result.error.unwrap().kind, "terminal_job");
    }

    #[test]
    fn host_failures_must_match_the_pending_effect() {
        let started = step(
            KonaseState::default(),
            KonaseEvent::UserSubmitted(request()),
        );
        let waiting = step(
            started.state,
            KonaseEvent::AgentProgress(AgentProgress {
                job_id: "job-1".into(),
                strategy_summary: None,
                observation: None,
                action: Some(AgentAction::CallMcp(McpRequest {
                    request_id: "mcp-1".into(),
                    server: "ugoite".into(),
                    operation: "ugoite.search".into(),
                    arguments: BTreeMap::new(),
                })),
            }),
        );
        let rejected = step(
            waiting.state.clone(),
            KonaseEvent::HostFailed(HostError {
                kind: "network".into(),
                message: "wrong request".into(),
                request_id: Some("mcp-other".into()),
            }),
        );
        assert_eq!(rejected.state, waiting.state);
        assert_eq!(rejected.error.unwrap().kind, "unexpected_host_failure");

        let failed = step(
            waiting.state,
            KonaseEvent::HostFailed(HostError {
                kind: "network".into(),
                message: "request failed".into(),
                request_id: Some("mcp-1".into()),
            }),
        );
        assert_eq!(failed.state.status, SessionStatus::Failed);
        assert_eq!(failed.state.job.unwrap().status, JobStatus::Failed);
    }

    #[test]
    fn loaded_state_rejects_oversized_identity_before_transition() {
        let oversized = "x".repeat(MAX_GOAL_CHARS * 2);
        let state = KonaseState {
            status: SessionStatus::Completed,
            work: Some(Work {
                id: oversized.clone(),
                goal: oversized.clone(),
                status: WorkStatus::Completed,
                job_count: u32::MAX,
                last_outcome: Some(JobOutcome {
                    job_id: oversized.clone(),
                    summary: oversized.clone(),
                    meaningful: true,
                }),
            }),
            job: None,
            observations: vec![observation(&oversized)],
            pending_effect: None,
            last_output: None,
        };

        let result = step(
            state,
            KonaseEvent::JobCompleted(JobOutcome {
                job_id: "missing".into(),
                summary: "ignored".into(),
                meaningful: false,
            }),
        );
        assert_eq!(result.state, KonaseState::default());
        assert_eq!(result.effects, Vec::new());
        assert_eq!(result.error.unwrap().kind, "input_too_large");
    }
}
