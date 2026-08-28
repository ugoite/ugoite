use crate::ContextBuilder;
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
    pub action: EffectKind,
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
pub enum EffectKind {
    CallMcp(McpRequest),
    AskConfirmation(ConfirmationRequest),
    Complete(JobOutcome),
    None,
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
    let mut state = normalize_state(state);
    let original_state = state.clone();
    let mut effects = Vec::new();
    let result = match normalize_event(event) {
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
        Ok(()) => StepResult {
            state,
            effects,
            error: None,
        },
        Err(error) => StepResult {
            state: original_state,
            effects: Vec::new(),
            error: Some(error),
        },
    }
}

/// Normalize state loaded from an untrusted adapter into the fixed-size
/// representation used by the control plane.
pub fn normalize_state(mut state: KonaseState) -> KonaseState {
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
    state
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
    {
        let job = active_job(state, &job_id)?;
        job.status = JobStatus::Running;
        job.strategy_summary = progress.strategy_summary;
    }

    if let Some(observation) = progress.observation {
        let output = record_observation(state, observation)?;
        state.last_output = Some(output.clone());
        effects.push(KonaseEffect::Emit(output));
    }
    match progress.action {
        EffectKind::CallMcp(request) => {
            let request_id = request.request_id.clone();
            current_job(state, &job_id)?.status = JobStatus::WaitingForHost;
            state.pending_effect = Some(PendingEffect::CallMcp { request_id });
            effects.push(KonaseEffect::CallMcp(request));
        }
        EffectKind::AskConfirmation(request) => {
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
        EffectKind::Complete(outcome) => complete_job(state, outcome, effects)?,
        EffectKind::None => {
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
        (Some(PendingEffect::CallMcp { request_id }), Some(actual))
        | (Some(PendingEffect::AskConfirmation { request_id }), Some(actual))
            if request_id == actual => {}
        (Some(PendingEffect::StartJob), None) | (None, None) => {}
        (Some(PendingEffect::CallMcp { .. }), None)
        | (Some(PendingEffect::AskConfirmation { .. }), None)
        | (Some(PendingEffect::StartJob), Some(_))
        | (None, Some(_)) => {
            return Err(KonaseError::new(
                "unexpected_host_failure",
                "host failure does not match the pending effect",
            ))
        }
        (Some(PendingEffect::CallMcp { .. }), Some(_))
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

fn normalize_event(event: KonaseEvent) -> KonaseEvent {
    match event {
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
    }
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
    progress.action = normalize_effect_kind(progress.action);
    progress
}

fn normalize_effect_kind(action: EffectKind) -> EffectKind {
    match action {
        EffectKind::CallMcp(request) => EffectKind::CallMcp(normalize_mcp_request(request)),
        EffectKind::AskConfirmation(request) => {
            EffectKind::AskConfirmation(normalize_confirmation_request(request))
        }
        EffectKind::Complete(outcome) => EffectKind::Complete(normalize_job_outcome(outcome)),
        EffectKind::None => EffectKind::None,
    }
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
                action: EffectKind::CallMcp(McpRequest {
                    request_id: "mcp-1".into(),
                    server: "ugoite".into(),
                    operation: "ugoite.search".into(),
                    arguments: BTreeMap::new(),
                }),
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
                    action: EffectKind::None,
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
                action: EffectKind::None,
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
                action: EffectKind::CallMcp(McpRequest {
                    request_id: "mcp-1".into(),
                    server: "ugoite".into(),
                    operation: "ugoite.search".into(),
                    arguments: BTreeMap::new(),
                }),
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
    fn loaded_state_is_normalized_before_transition() {
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

        let normalized = step(
            state,
            KonaseEvent::JobCompleted(JobOutcome {
                job_id: "missing".into(),
                summary: "ignored".into(),
                meaningful: false,
            }),
        )
        .state;
        let work = normalized.work.unwrap();
        assert_eq!(work.id.chars().count(), MAX_IDENTIFIER_CHARS);
        assert_eq!(work.goal.chars().count(), MAX_GOAL_CHARS);
        assert_eq!(
            work.last_outcome.unwrap().summary.chars().count(),
            MAX_STATE_SUMMARY_CHARS
        );
        assert_eq!(
            normalized.observations[0].id.chars().count(),
            MAX_IDENTIFIER_CHARS
        );
    }
}
