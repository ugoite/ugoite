use crate::config::non_empty_env_value;
use crate::output::{emit_diagnostic, emit_success, emit_text, Format};
use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use clap::Args;
use rmcp::{
    model::{ClientConfig, *},
    transport::{streamable_http_client::StreamableHttpClientTransportConfig, *},
    ClientLifecycleMode, ClientServiceExt, RoleClient,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    future::Future,
    io::{self, IsTerminal, Write},
    pin::Pin,
    sync::{Arc as StdArc, Mutex as StdMutex},
    time::Duration,
};
use tokio::sync::oneshot;
use ugoite_konase::{
    step, AgentAction, AgentProgress, AgentRuntime, AgentRuntimeInput, Capability,
    CapabilityEffect, HostError, JobOutcome, KnowledgeOutcome, KonaseEvent, KonaseState,
    McpRequest, McpResult, ModelMessage, ModelRequest, ModelResult, Observation, ObservationKind,
    ResourceContent, ResourceReference, UserRequest,
};
use ugoite_konase_rig::RigAgentRuntime;
use uuid::Uuid;

const DEFAULT_MODEL_TIMEOUT_SECS: u64 = 120;
const MAX_MODEL_HOST_ERROR_CHARS: usize = 1_800;
const MODEL_INTERRUPTED_KIND: &str = "model_interrupted";

type ModelInterruptFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

trait ModelInterruptSource {
    fn wait_for_model_interrupt(&self) -> ModelInterruptFuture<'_>;
}

trait ConfirmationSource: Sync {
    fn confirm(&self, request: &ugoite_konase::ConfirmationRequest) -> bool;
}

struct TtyConfirmation;

impl ConfirmationSource for TtyConfirmation {
    fn confirm(&self, request: &ugoite_konase::ConfirmationRequest) -> bool {
        request_confirmation(request)
    }
}

#[cfg(test)]
struct ApproveConfirmation;

#[cfg(test)]
impl ConfirmationSource for ApproveConfirmation {
    fn confirm(&self, _request: &ugoite_konase::ConfirmationRequest) -> bool {
        true
    }
}

#[cfg(test)]
struct NeverModelInterrupt;

#[cfg(test)]
impl ModelInterruptSource for NeverModelInterrupt {
    fn wait_for_model_interrupt(&self) -> ModelInterruptFuture<'_> {
        Box::pin(std::future::pending())
    }
}

#[derive(Clone)]
struct SignalCoordinator {
    state: StdArc<SignalState>,
}

struct SignalState {
    model_waiter: StdMutex<Option<oneshot::Sender<()>>>,
}

struct ModelWait {
    receiver: oneshot::Receiver<()>,
    state: StdArc<SignalState>,
}

impl SignalCoordinator {
    fn new() -> Self {
        Self {
            state: StdArc::new(SignalState {
                model_waiter: StdMutex::new(None),
            }),
        }
    }

    fn install() -> Result<Self> {
        let coordinator = Self::new();
        #[cfg(unix)]
        {
            let signals = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                .context("install SIGINT handler")?;
            tokio::spawn(run_unix_signal_loop(signals, coordinator.clone()));
        }
        #[cfg(not(unix))]
        {
            tokio::spawn(run_ctrl_c_signal_loop(coordinator.clone()));
        }
        Ok(coordinator)
    }

    fn interrupt_model_wait(&self) -> bool {
        let sender = self
            .state
            .model_waiter
            .lock()
            .expect("SIGINT model waiter lock poisoned")
            .take();
        sender.is_some_and(|sender| sender.send(()).is_ok())
    }

    fn handle_interrupt(&self) {
        if !self.interrupt_model_wait() {
            // SIGINT has historically terminated the CLI outside a model wait.
            // The process-wide listener owns the signal now, so preserve that
            // behavior explicitly for idle, MCP, and undo states.
            std::process::exit(130);
        }
    }
}

impl ModelInterruptSource for SignalCoordinator {
    fn wait_for_model_interrupt(&self) -> ModelInterruptFuture<'_> {
        let (sender, receiver) = oneshot::channel();
        self.state
            .model_waiter
            .lock()
            .expect("model waiter lock poisoned")
            .replace(sender);
        Box::pin(ModelWait {
            receiver,
            state: self.state.clone(),
        })
    }
}

impl Future for ModelWait {
    type Output = ();

    fn poll(
        mut self: Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        Pin::new(&mut self.receiver).poll(context).map(|_| ())
    }
}

impl Drop for ModelWait {
    fn drop(&mut self) {
        self.state
            .model_waiter
            .lock()
            .expect("model waiter lock poisoned")
            .take();
    }
}

#[cfg(unix)]
async fn run_unix_signal_loop(
    mut signals: tokio::signal::unix::Signal,
    coordinator: SignalCoordinator,
) {
    while signals.recv().await.is_some() {
        coordinator.handle_interrupt();
    }
}

#[cfg(not(unix))]
async fn run_ctrl_c_signal_loop(coordinator: SignalCoordinator) {
    loop {
        if tokio::signal::ctrl_c().await.is_err() {
            return;
        }
        coordinator.handle_interrupt();
    }
}

#[derive(Args)]
pub struct KonaseCmd {
    /// Run one request and exit instead of reading an interactive stdin loop.
    #[arg(long)]
    pub prompt: Option<String>,
    /// Read this canonical Form or Entry URI into the next Job's Context. Repeat up to four times.
    #[arg(long = "resource", value_name = "URI")]
    pub resources: Vec<String>,
    /// Explicitly send selected Context in non-interactive mode (does not approve writes).
    #[arg(long)]
    pub send_selected_context: bool,
}

#[async_trait]
trait ModelHost {
    async fn call_model(
        &mut self,
        request: ModelRequest,
    ) -> std::result::Result<ModelResult, HostError>;
}

#[async_trait]
trait McpHost {
    async fn call_mcp(&mut self, request: McpRequest, work_id: &str) -> Result<McpResult>;
    async fn capabilities(&self) -> Vec<Capability>;
}

#[derive(Clone)]
struct OpenAiModelHost {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
    timeout: Duration,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    tools: Vec<ChatTool>,
    tool_choice: String,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<ChatToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct ChatTool {
    r#type: String,
    function: ChatFunction,
}

#[derive(Debug, Serialize)]
struct ChatFunction {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Serialize)]
struct ChatToolCall {
    id: String,
    r#type: String,
    function: ChatFunctionCall,
}

#[derive(Debug, Serialize)]
struct ChatFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatResponseMessage {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ChatResponseToolCall>,
}

#[derive(Debug, Deserialize)]
struct ChatResponseToolCall {
    id: String,
    function: ChatResponseFunction,
}

#[derive(Debug, Deserialize)]
struct ChatResponseFunction {
    name: String,
    arguments: String,
}

impl OpenAiModelHost {
    fn from_env() -> Result<Self> {
        let api_key = non_empty_env_value("UGOITE_MODEL_API_KEY")
            .or_else(|| non_empty_env_value("OPENAI_API_KEY"))
            .ok_or_else(|| {
                anyhow!("set UGOITE_MODEL_API_KEY or OPENAI_API_KEY for `ugoite konase`")
            })?;
        Ok(Self {
            client: reqwest::Client::new(),
            base_url: non_empty_env_value("UGOITE_MODEL_BASE_URL")
                .unwrap_or_else(|| "https://api.openai.com/v1".into()),
            api_key,
            model: non_empty_env_value("UGOITE_MODEL_NAME").unwrap_or_else(|| "gpt-4o-mini".into()),
            timeout: parse_model_timeout(
                non_empty_env_value("UGOITE_MODEL_TIMEOUT_SECS").as_deref(),
            )?,
        })
    }

    async fn call_model_inner(
        &self,
        request: ModelRequest,
    ) -> std::result::Result<ModelResult, ModelCallError> {
        let mut messages = request
            .history
            .into_iter()
            .map(chat_message)
            .collect::<Vec<_>>();
        messages.push(ChatMessage {
            role: "user".into(),
            content: Some(request.prompt),
            tool_calls: vec![],
            tool_call_id: None,
        });
        let response = self
            .client
            .post(format!(
                "{}/chat/completions",
                self.base_url.trim_end_matches('/')
            ))
            .bearer_auth(&self.api_key)
            .json(&ChatRequest {
                model: self.model.clone(),
                messages,
                tools: request
                    .tools
                    .into_iter()
                    .map(|tool| ChatTool {
                        r#type: "function".into(),
                        function: ChatFunction {
                            name: tool.name,
                            description: tool.description,
                            parameters: tool.input_schema,
                        },
                    })
                    .collect(),
                tool_choice: "auto".into(),
            })
            .send()
            .await
            .map_err(|error| ModelCallError::Transport(error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .map_err(|error| ModelCallError::Transport(error.to_string()))?;
            return Err(ModelCallError::Provider { status, body });
        }
        let body: ChatResponse = response
            .json()
            .await
            .map_err(|error| ModelCallError::Response(error.to_string()))?;
        let choice =
            body.choices.into_iter().next().ok_or_else(|| {
                ModelCallError::Response("model provider returned no choices".into())
            })?;
        let finish_reason = choice.finish_reason;
        let message = choice.message;
        let mut tool_calls = Vec::new();
        for call in message.tool_calls {
            let arguments = serde_json::from_str(&call.function.arguments).map_err(|error| {
                ModelCallError::Response(format!(
                    "decode model tool arguments for {}: {error}",
                    call.function.name
                ))
            })?;
            tool_calls.push(ugoite_konase::ModelToolCall {
                id: call.id,
                name: call.function.name,
                arguments,
            });
        }
        let text = message.content.filter(|content| !content.trim().is_empty());
        if text.is_none() && tool_calls.is_empty() {
            let reason = if finish_reason.as_deref() == Some("length") {
                "model provider reached the output limit without producing an answer; retry with a shorter request"
            } else {
                "model provider returned an empty completion; retry the request"
            };
            return Err(ModelCallError::Response(reason.into()));
        }
        Ok(ModelResult {
            request_id: request.request_id,
            text,
            tool_calls,
        })
    }
}

#[async_trait]
impl ModelHost for OpenAiModelHost {
    async fn call_model(
        &mut self,
        request: ModelRequest,
    ) -> std::result::Result<ModelResult, HostError> {
        let request_id = request.request_id.clone();
        match tokio::time::timeout(self.timeout, self.call_model_inner(request)).await {
            Ok(result) => result.map_err(|error| error.into_host_error(request_id)),
            Err(_) => Err(HostError {
                kind: "model_timeout".into(),
                message: format!("model request timed out after {:?}", self.timeout),
                request_id: Some(request_id),
            }),
        }
    }
}

#[derive(Debug)]
enum ModelCallError {
    Transport(String),
    Provider {
        status: reqwest::StatusCode,
        body: String,
    },
    Response(String),
}

impl ModelCallError {
    fn into_host_error(self, request_id: String) -> HostError {
        let (kind, message) = match self {
            Self::Transport(message) => (
                "model_transport",
                format!("model provider request failed: {message}"),
            ),
            Self::Provider { status, body } => (
                "model_provider",
                format!("model provider returned {status}: {body}"),
            ),
            Self::Response(message) => (
                "model_response",
                format!("invalid model provider response: {message}"),
            ),
        };
        HostError {
            kind: kind.into(),
            message: bound_model_host_error(message),
            request_id: Some(request_id),
        }
    }
}

fn bound_model_host_error(message: String) -> String {
    let mut bounded = message
        .chars()
        .take(MAX_MODEL_HOST_ERROR_CHARS)
        .collect::<String>();
    if bounded.chars().count() == MAX_MODEL_HOST_ERROR_CHARS {
        bounded.push_str("...");
    }
    bounded
}

fn parse_model_timeout(value: Option<&str>) -> Result<Duration> {
    let seconds = value
        .map(str::trim)
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|_| anyhow!("UGOITE_MODEL_TIMEOUT_SECS must be a positive integer"))?
        .unwrap_or(DEFAULT_MODEL_TIMEOUT_SECS);
    if seconds == 0 {
        bail!("UGOITE_MODEL_TIMEOUT_SECS must be a positive integer");
    }
    Ok(Duration::from_secs(seconds))
}

fn chat_message(message: ModelMessage) -> ChatMessage {
    match message {
        ModelMessage::System { content } => ChatMessage {
            role: "system".into(),
            content: Some(content),
            tool_calls: vec![],
            tool_call_id: None,
        },
        ModelMessage::User { content } => ChatMessage {
            role: "user".into(),
            content: Some(content),
            tool_calls: vec![],
            tool_call_id: None,
        },
        ModelMessage::Assistant {
            content,
            tool_calls,
        } => ChatMessage {
            role: "assistant".into(),
            content: (!content.is_empty()).then_some(content),
            tool_calls: tool_calls
                .into_iter()
                .map(|call| ChatToolCall {
                    id: call.id,
                    r#type: "function".into(),
                    function: ChatFunctionCall {
                        name: call.name,
                        arguments: call.arguments.to_string(),
                    },
                })
                .collect(),
            tool_call_id: None,
        },
        ModelMessage::Tool {
            call_id,
            name: _,
            content,
        } => ChatMessage {
            role: "tool".into(),
            content: Some(content),
            tool_calls: vec![],
            tool_call_id: Some(call_id),
        },
    }
}

type McpClient = rmcp::service::RunningService<RoleClient, ClientConfig>;

struct RmcpMcpHost {
    client: McpClient,
    capabilities: Vec<Capability>,
}

fn work_meta(work_id: &str) -> RequestMetaObject {
    let mut meta = RequestMetaObject::with_client_context(
        ProtocolVersion::V_2026_07_28,
        Implementation::new("ugoite-cli", env!("CARGO_PKG_VERSION")),
        ClientCapabilities::default(),
    );
    meta.0
         .0
        .insert("ugoite/runId".into(), Value::String(work_id.to_owned()));
    meta
}

fn capability_from_tool(tool: Tool) -> Capability {
    let input_schema = tool.schema_as_json_value();
    let name = tool.name.into_owned();
    Capability {
        name: name.clone(),
        description: tool
            .description
            .map_or_else(String::new, |value| value.into_owned()),
        input_schema: Some(input_schema),
        effect: if matches!(name.as_str(), "ugoite.save" | "ugoite.undo") {
            Some(CapabilityEffect::Write)
        } else {
            tool.annotations.and_then(|annotations| {
                annotations.read_only_hint.map(|read_only| {
                    if read_only {
                        CapabilityEffect::Read
                    } else {
                        CapabilityEffect::Write
                    }
                })
            })
        },
    }
}

fn resources_read_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {"uri": {"type": "string"}},
        "required": ["uri"],
        "additionalProperties": false
    })
}

impl RmcpMcpHost {
    async fn connect(
        base_url: &str,
        connection: Option<&str>,
        credential: Option<&str>,
    ) -> Result<Self> {
        let target = crate::commands::auth::mcp_target(base_url).await?;
        let Some(connection) = connection else {
            bail!("`ugoite konase` requires a canonical connection with an MCP credential; run `ugoite auth login --for mcp`");
        };
        let session = crate::http::named_session_for_target(base_url, connection, credential)
            .await?
            .filter(|session| session.resource.as_deref() == Some(target.resource.as_str()))
            .ok_or_else(|| {
                anyhow!(
                    "`ugoite konase` requires an MCP credential; run `ugoite auth login --for mcp`"
                )
            })?;
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(target.endpoint)
                .auth_header(session.access_token),
        );
        let client = ClientConfig::default()
            .serve_with_lifecycle(
                transport,
                ClientLifecycleMode::Discover {
                    preferred_versions: vec![ProtocolVersion::V_2026_07_28],
                },
            )
            .await
            .context("connect to Ugoite MCP")?;
        let listed = client.list_tools(None).await.context("list MCP tools")?;
        let mut capabilities = listed
            .tools
            .into_iter()
            .map(capability_from_tool)
            .filter(|capability| {
                matches!(
                    capability.name.as_str(),
                    "ugoite.search" | "ugoite.save" | "ugoite.undo"
                )
            })
            .collect::<Vec<_>>();
        capabilities.push(Capability {
            name: "resources/read".into(),
            description: "Read the full content of an opaque Ugoite resource URI".into(),
            input_schema: Some(resources_read_schema()),
            effect: Some(CapabilityEffect::Read),
        });
        Ok(Self {
            client,
            capabilities,
        })
    }
}

fn write_tool_request(
    request: McpRequest,
    work_id: &str,
) -> Result<(String, CallToolRequestParams)> {
    if !matches!(request.operation.as_str(), "ugoite.save" | "ugoite.undo") {
        bail!("unsupported MCP write operation {}", request.operation);
    }
    let operation = request.operation;
    let mut params = CallToolRequestParams::new(operation.clone())
        .with_arguments(request.arguments.into_iter().collect());
    params.set_meta(work_meta(work_id));
    Ok((operation, params))
}

#[async_trait]
impl McpHost for RmcpMcpHost {
    async fn call_mcp(&mut self, request: McpRequest, work_id: &str) -> Result<McpResult> {
        if request.operation == "resources/read" {
            let uri = request
                .arguments
                .get("uri")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("resources/read requires a string uri"))?;
            let result = self
                .client
                .read_resource(ReadResourceRequestParams::new(uri).with_meta(work_meta(work_id)))
                .await
                .context("read MCP resource")?;
            if result.contents.len() != 1 {
                bail!("MCP resources/read must return exactly one resource content");
            }
            let resource_contents = result
                .contents
                .into_iter()
                .filter_map(|content| match content {
                    ResourceContents::TextResourceContents { uri, text, .. } => {
                        Some(ResourceContent { uri, content: text })
                    }
                    ResourceContents::BlobResourceContents { .. } => None,
                    _ => None,
                })
                .collect::<Vec<_>>();
            if resource_contents.len() != 1 || resource_contents[0].uri != uri {
                bail!("MCP resources/read returned an invalid or mismatched resource");
            }
            return Ok(McpResult {
                request_id: request.request_id,
                operation: request.operation,
                success: true,
                observation: None,
                resources: vec![],
                resource_contents,
                error: None,
            });
        }
        if matches!(request.operation.as_str(), "ugoite.save" | "ugoite.undo") {
            let request_id = request.request_id.clone();
            let (operation, params) = write_tool_request(request, work_id)?;
            let result = self
                .client
                .call_tool(params)
                .await
                .with_context(|| format!("call {operation} MCP tool"))?;
            let text = result
                .content
                .iter()
                .filter_map(|content| match content {
                    ContentBlock::Text(content) => Some(content.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            let valid_receipt = match operation.as_str() {
                "ugoite.save" => result
                    .structured_content
                    .as_ref()
                    .is_some_and(valid_save_receipt),
                "ugoite.undo" => result
                    .structured_content
                    .as_ref()
                    .is_some_and(|value| valid_undo_receipt(value, work_id)),
                _ => false,
            };
            if result.is_error != Some(true) && !valid_receipt {
                bail!("MCP mutation receipt is missing or invalid; success could not be confirmed");
            }
            let success = result.is_error != Some(true);
            let error = if !success { Some(text) } else { None };
            return Ok(McpResult {
                request_id,
                operation,
                success,
                observation: None,
                resources: vec![],
                resource_contents: vec![],
                error,
            });
        }
        if request.operation != "ugoite.search" {
            bail!("unsupported MCP operation {}", request.operation);
        }
        let mut params = CallToolRequestParams::new("ugoite.search")
            .with_arguments(request.arguments.into_iter().collect());
        params.set_meta(work_meta(work_id));
        let result = self
            .client
            .call_tool(params)
            .await
            .context("call Ugoite search tool")?;
        let text = result
            .content
            .iter()
            .filter_map(|content| match content {
                ContentBlock::Text(content) => Some(content.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        let resources = result
            .content
            .iter()
            .filter_map(|content| match content {
                ContentBlock::ResourceLink(resource) => Some(ResourceReference {
                    uri: resource.uri.clone(),
                    label: Some(resource.name.clone()),
                }),
                _ => None,
            })
            .collect::<Vec<_>>();
        let success = result.is_error != Some(true);
        let error = (!success).then(|| text.clone());
        Ok(McpResult {
            request_id: request.request_id,
            operation: request.operation,
            success,
            observation: Some(Observation {
                id: format!("observation-{}", Uuid::now_v7()),
                kind: ObservationKind::Mcp,
                summary: text,
                facts: Default::default(),
                resource_references: resources.clone(),
            }),
            resources,
            resource_contents: vec![],
            error,
        })
    }

    async fn capabilities(&self) -> Vec<Capability> {
        self.capabilities.clone()
    }
}

fn valid_save_receipt(value: &Value) -> bool {
    let id = value.get("id").and_then(Value::as_str);
    let uri = value.get("uri").and_then(Value::as_str);
    id.is_some_and(|id| !id.is_empty())
        && uri
            .is_some_and(|uri| Some(uri) == id.map(|id| format!("ugoite://entry/{id}")).as_deref())
        && matches!(
            value.get("status").and_then(Value::as_str),
            Some("created" | "updated")
        )
        && value.get("_untrusted_content").and_then(Value::as_bool) == Some(true)
}

fn valid_undo_receipt(value: &Value, work_id: &str) -> bool {
    value.get("run_id").and_then(Value::as_str) == Some(work_id)
        && value
            .get("reverted_change_count")
            .and_then(Value::as_u64)
            .is_some()
        && value.get("_untrusted_content").and_then(Value::as_bool) == Some(true)
}

fn requires_approval(request: &McpRequest) -> bool {
    request.effect == Some(CapabilityEffect::Write)
        || matches!(request.operation.as_str(), "ugoite.save" | "ugoite.undo")
}

fn request_confirmation(request: &ugoite_konase::ConfirmationRequest) -> bool {
    if !io::stdin().is_terminal() {
        return false;
    }
    eprintln!("\nKonase write approval required");
    eprintln!("Operation: {}", request.operation);
    eprintln!("{}", request.reason);
    eprint!("Approve this one write? [y/N] ");
    let _ = io::stderr().flush();
    let mut answer = String::new();
    if io::stdin().read_line(&mut answer).unwrap_or(0) == 0 {
        return false;
    }
    confirmation_answer_is_approved(&answer)
}

trait SelectedContextConfirmation: Sync {
    fn confirm_selected_context(&self) -> bool;
}

struct TtySelectedContextConfirmation;

impl SelectedContextConfirmation for TtySelectedContextConfirmation {
    fn confirm_selected_context(&self) -> bool {
        if !io::stdin().is_terminal() {
            return false;
        }
        eprint!("Send this selected Context to the model? [y/N] ");
        let _ = io::stderr().flush();
        let mut answer = String::new();
        if io::stdin().read_line(&mut answer).unwrap_or(0) == 0 {
            return false;
        }
        confirmation_answer_is_approved(&answer)
    }
}

fn canonical_selected_uris(resources: &[String]) -> Result<Vec<String>> {
    if resources.len() > ugoite_konase::MAX_SELECTED_RESOURCES {
        bail!("at most four --resource values may be selected for one Job");
    }
    let mut unique = Vec::new();
    for uri in resources {
        let (id, kind) = if let Some(id) = uri.strip_prefix("ugoite://form/") {
            (id, ugoite_domain::id::IdentifierKind::Form)
        } else if let Some(id) = uri.strip_prefix("ugoite://entry/") {
            (id, ugoite_domain::id::IdentifierKind::Entry)
        } else {
            bail!("--resource must be a canonical Form or Entry URI");
        };
        if id.is_empty() || id.contains(['/', '?', '#', '%']) {
            bail!("--resource must contain one opaque ID segment");
        }
        ugoite_domain::id::validate_identifier(kind, id)
            .map_err(|_| anyhow!("--resource contains an invalid opaque ID"))?;
        if !unique.contains(uri) {
            unique.push(uri.clone());
        }
    }
    Ok(unique)
}

fn validate_selected_context_mode(
    resources: &[String],
    interactive: bool,
    send: bool,
) -> Result<()> {
    if !resources.is_empty() && !interactive && !send {
        bail!("selected Context in non-interactive mode requires --send-selected-context");
    }
    if send && resources.is_empty() {
        bail!("--send-selected-context requires at least one --resource");
    }
    Ok(())
}

async fn read_selected_resources<C: McpHost>(
    mcp: &mut C,
    uris: &[String],
    work_id: &str,
) -> Result<Vec<ResourceContent>> {
    let mut selected = Vec::with_capacity(uris.len());
    for uri in uris {
        let result = mcp
            .call_mcp(
                McpRequest {
                    request_id: format!("selected-resource-{}", Uuid::now_v7()),
                    server: "ugoite".into(),
                    operation: "resources/read".into(),
                    arguments: [("uri".to_owned(), Value::String(uri.clone()))]
                        .into_iter()
                        .collect(),
                    effect: Some(CapabilityEffect::Read),
                },
                work_id,
            )
            .await
            .with_context(|| format!("read selected MCP resource {}", uri.escape_debug()))?;
        if !result.success || result.resource_contents.len() != 1 {
            bail!("selected MCP resource read was denied or returned an invalid result");
        }
        let content = result.resource_contents.into_iter().next().unwrap();
        if content.uri != *uri {
            bail!("selected MCP resource returned a different URI than requested");
        }
        selected.push(content);
    }
    Ok(selected)
}

fn show_selected_context_preview(start: &ugoite_konase::JobRequest) {
    if start.resource_admission.is_empty() {
        return;
    }
    eprintln!("\nSelected Context preview (untrusted Knowledge)");
    for admission in &start.resource_admission {
        let uri = admission.uri.escape_debug().to_string();
        let status = match admission.status {
            ugoite_konase::ResourceAdmissionStatus::Included => "included",
            ugoite_konase::ResourceAdmissionStatus::Truncated => "shortened",
            ugoite_konase::ResourceAdmissionStatus::Omitted => "omitted",
        };
        let reason = admission.reason.map_or_else(String::new, |reason| {
            let label = match reason {
                ugoite_konase::ResourceAdmissionReason::ProjectionCompacted => {
                    "projection shortened to fit the per-resource limit"
                }
                ugoite_konase::ResourceAdmissionReason::ContextByteBudget => {
                    "omitted to fit the Context byte limit"
                }
                ugoite_konase::ResourceAdmissionReason::ModelPromptCharacterLimit => {
                    "omitted to fit the model prompt limit"
                }
            };
            format!(" ({label})")
        });
        eprintln!("- {uri}: {status}{reason}");
        if let Some(content) = start
            .context
            .selected_resource_contents
            .iter()
            .find(|content| content.uri == admission.uri)
        {
            eprintln!("  {}", content.content.escape_debug());
        }
    }
}

fn confirmation_answer_is_approved(answer: &str) -> bool {
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

pub async fn run(
    cmd: KonaseCmd,
    explicit_config: Option<&std::path::Path>,
    context_override: Option<&str>,
) -> Result<()> {
    let selected_uris = canonical_selected_uris(&cmd.resources)?;
    let interactive = io::stdin().is_terminal();
    validate_selected_context_mode(&selected_uris, interactive, cmd.send_selected_context)?;
    let interrupts = SignalCoordinator::install()?;
    let target =
        crate::cli_config::resolve_command_target(explicit_config, context_override, "konase")?;
    let base_url = target.base_url().ok_or_else(|| {
        anyhow!(
            "`ugoite konase` currently requires a backend or api context with an MCP credential"
        )
    })?;
    let mut mcp =
        RmcpMcpHost::connect(base_url, target.connection_name(), target.credential_name()).await?;
    let mut model = OpenAiModelHost::from_env()?;
    let capabilities = mcp.capabilities().await;
    match cmd.prompt {
        Some(prompt) => {
            if prompt.trim().is_empty() {
                bail!("Konase prompt must not be empty");
            }
            let result = run_turn_with_selected_context(
                &mut model,
                &mut mcp,
                &interrupts,
                &TtyConfirmation,
                SelectedTurnOptions {
                    prompt: &prompt,
                    capabilities: &capabilities,
                    selected_uris: &selected_uris,
                    context_confirmation: interactive.then_some(
                        &TtySelectedContextConfirmation as &dyn SelectedContextConfirmation,
                    ),
                },
            )
            .await?;
            let interrupted = matches!(
                &result,
                TurnResult::Failed(failure) if failure.error.kind == MODEL_INTERRUPTED_KIND
            );
            let _ = report_turn(result, false);
            if interrupted {
                bail!("model request interrupted");
            }
        }
        None => {
            if interactive {
                emit_text("Konase");
            }
            let mut input = String::new();
            let mut last_work_id: Option<String> = None;
            loop {
                if interactive {
                    print!("> ");
                    io::stdout().flush()?;
                }
                input.clear();
                if io::stdin().read_line(&mut input)? == 0 {
                    break;
                }
                let prompt = input.trim();
                if prompt.is_empty() {
                    continue;
                }
                if prompt == "u" {
                    match undo_last_work(&mut mcp, &mut last_work_id).await {
                        Ok(true) => emit_success(
                            &serde_json::json!({"undone": true}),
                            &Format::Plain,
                            Some("✓ 取り消しました".to_string()),
                        ),
                        Ok(false) => emit_success(
                            &serde_json::json!({"undone": false}),
                            &Format::Plain,
                            Some("取り消せる Work はありません。".to_string()),
                        ),
                        Err(error) => {
                            emit_diagnostic(format!("Error: {error:#}"));
                        }
                    }
                    continue;
                }
                last_work_id = report_turn(
                    run_turn_with_selected_context(
                        &mut model,
                        &mut mcp,
                        &interrupts,
                        &TtyConfirmation,
                        SelectedTurnOptions {
                            prompt,
                            capabilities: &capabilities,
                            selected_uris: &selected_uris,
                            context_confirmation: interactive.then_some(
                                &TtySelectedContextConfirmation as &dyn SelectedContextConfirmation,
                            ),
                        },
                    )
                    .await?,
                    true,
                );
            }
        }
    }
    Ok(())
}

async fn undo_last_work<C: McpHost>(
    mcp: &mut C,
    last_work_id: &mut Option<String>,
) -> Result<bool> {
    let Some(work_id) = last_work_id.clone() else {
        return Ok(false);
    };
    let result = mcp
        .call_mcp(
            McpRequest {
                request_id: format!("undo-{}", Uuid::now_v7()),
                server: "ugoite".into(),
                operation: "ugoite.undo".into(),
                arguments: Default::default(),
                effect: Some(CapabilityEffect::Write),
            },
            &work_id,
        )
        .await?;
    if !result.success {
        bail!(result.error.unwrap_or_else(|| "Undo failed".into()));
    }
    *last_work_id = None;
    Ok(true)
}

struct TurnOutcome {
    outcome: JobOutcome,
    work_id: String,
    undo_available: bool,
    knowledge: KnowledgeOutcome,
}

enum TurnResult {
    Completed(TurnOutcome),
    Failed(TurnFailure),
}

struct TurnFailure {
    error: HostError,
    work_id: String,
    undo_available: bool,
    knowledge: KnowledgeOutcome,
}

struct SelectedTurnOptions<'a> {
    prompt: &'a str,
    capabilities: &'a [Capability],
    selected_uris: &'a [String],
    context_confirmation: Option<&'a dyn SelectedContextConfirmation>,
}

struct TurnInput<'a> {
    prompt: &'a str,
    capabilities: &'a [Capability],
    selected_resource_contents: &'a [ResourceContent],
    context_confirmation: Option<&'a dyn SelectedContextConfirmation>,
    work_id: String,
}

fn report_turn(result: TurnResult, show_undo_hint: bool) -> Option<String> {
    match result {
        TurnResult::Completed(turn) => {
            let knowledge = knowledge_label(turn.knowledge);
            let summary = turn.outcome.summary;
            emit_success(
                &serde_json::json!({
                    "summary": summary.clone(),
                    "knowledge": knowledge,
                }),
                &Format::Plain,
                Some(format!("{summary}\nKnowledge: {knowledge}")),
            );
            if show_undo_hint && turn.undo_available {
                emit_text("[u] 取り消す");
            }
            (show_undo_hint && turn.undo_available).then_some(turn.work_id)
        }
        TurnResult::Failed(failure) => {
            if failure.error.kind == MODEL_INTERRUPTED_KIND {
                emit_diagnostic("Model request interrupted.");
            } else if failure.error.kind == "write_denied" {
                emit_diagnostic("Write denied; no MCP mutation was sent.");
            } else if failure.error.kind == "mcp_unconfirmed" {
                emit_diagnostic(
                    "MCP mutation outcome is unconfirmed; no automatic retry was attempted.",
                );
            } else {
                emit_diagnostic(format!(
                    "Model host failed ({}): {}",
                    failure.error.kind, failure.error.message
                ));
            }
            let knowledge = knowledge_label(failure.knowledge);
            emit_success(
                &serde_json::json!({"knowledge": knowledge}),
                &Format::Plain,
                Some(format!("Knowledge: {knowledge}")),
            );
            if show_undo_hint && failure.undo_available {
                emit_text("[u] 取り消す");
            }
            (show_undo_hint && failure.undo_available).then_some(failure.work_id)
        }
    }
}

fn knowledge_label(outcome: KnowledgeOutcome) -> &'static str {
    match outcome {
        KnowledgeOutcome::Unchanged => "unchanged",
        KnowledgeOutcome::Saved => "saved",
        KnowledgeOutcome::WriteFailed => "write failed",
    }
}

#[cfg(test)]
async fn run_turn<M: ModelHost, C: McpHost>(
    model: &mut M,
    mcp: &mut C,
    prompt: &str,
    capabilities: &[Capability],
) -> Result<TurnResult> {
    run_turn_with_interrupts(
        model,
        mcp,
        prompt,
        capabilities,
        &NeverModelInterrupt,
        &ApproveConfirmation,
    )
    .await
}

#[cfg(test)]
async fn run_turn_with_interrupts<M: ModelHost, C: McpHost, I: ModelInterruptSource + ?Sized>(
    model: &mut M,
    mcp: &mut C,
    prompt: &str,
    capabilities: &[Capability],
    interrupts: &I,
    confirmations: &dyn ConfirmationSource,
) -> Result<TurnResult> {
    run_turn_with_contents(
        model,
        mcp,
        interrupts,
        confirmations,
        TurnInput {
            prompt,
            capabilities,
            selected_resource_contents: &[],
            context_confirmation: None,
            work_id: format!("work-{}", Uuid::now_v7()),
        },
    )
    .await
}

async fn run_turn_with_selected_context<
    M: ModelHost,
    C: McpHost,
    I: ModelInterruptSource + ?Sized,
>(
    model: &mut M,
    mcp: &mut C,
    interrupts: &I,
    confirmations: &dyn ConfirmationSource,
    options: SelectedTurnOptions<'_>,
) -> Result<TurnResult> {
    if options.selected_uris.is_empty() {
        return run_turn_with_contents(
            model,
            mcp,
            interrupts,
            confirmations,
            TurnInput {
                prompt: options.prompt,
                capabilities: options.capabilities,
                selected_resource_contents: &[],
                context_confirmation: options.context_confirmation,
                work_id: format!("work-{}", Uuid::now_v7()),
            },
        )
        .await;
    }
    let work_id = format!("work-{}", Uuid::now_v7());
    let selected = read_selected_resources(mcp, options.selected_uris, &work_id).await?;
    run_turn_with_contents(
        model,
        mcp,
        interrupts,
        confirmations,
        TurnInput {
            prompt: options.prompt,
            capabilities: options.capabilities,
            selected_resource_contents: &selected,
            context_confirmation: options.context_confirmation,
            work_id,
        },
    )
    .await
}

async fn run_turn_with_contents<M: ModelHost, C: McpHost, I: ModelInterruptSource + ?Sized>(
    model: &mut M,
    mcp: &mut C,
    interrupts: &I,
    confirmations: &dyn ConfirmationSource,
    input: TurnInput<'_>,
) -> Result<TurnResult> {
    let TurnInput {
        prompt,
        capabilities,
        selected_resource_contents,
        context_confirmation,
        work_id,
    } = input;
    let job_id = format!("job-{}", Uuid::now_v7());
    let initial = step(
        Default::default(),
        ugoite_konase::KonaseEvent::UserSubmitted(UserRequest {
            work_id: work_id.clone(),
            job_id: job_id.clone(),
            goal: prompt.into(),
            available_capabilities: capabilities.to_vec(),
            safety_hints: vec![
                "Use Ugoite MCP for requested reads and writes; the Host binds writes to this Work and supports undo".into(),
            ],
            selected_resource_contents: selected_resource_contents.to_vec(),
            expected_response_schema: None,
        }),
    );
    if let Some(error) = initial.error {
        bail!("Konase rejected the initial request: {}", error.message);
    }
    let mut state = initial.state;
    let start = initial
        .effects
        .into_iter()
        .find_map(|effect| match effect {
            ugoite_konase::KonaseEffect::StartJob(request) => Some(*request),
            _ => None,
        })
        .ok_or_else(|| anyhow!("Konase did not start a Job"))?;
    show_selected_context_preview(&start);
    if !start.resource_admission.is_empty()
        && context_confirmation.is_some_and(|confirmation| !confirmation.confirm_selected_context())
    {
        bail!("selected Context was not approved for model submission");
    }
    let mut runtime = RigAgentRuntime::default();
    let first_action = runtime.start(start.job, start.context)?;
    let start_effect = step(
        state,
        ugoite_konase::KonaseEvent::AgentProgress(AgentProgress {
            job_id: job_id.clone(),
            strategy_summary: None,
            observation: None,
            action: Some(first_action.clone()),
        }),
    );
    if let Some(error) = start_effect.error {
        bail!(
            "Konase rejected the initial AgentProgress: {}",
            error.message
        );
    }
    state = start_effect.state;
    let mut action = first_action;
    let mut undo_available = false;
    let mut approved_write: Option<McpRequest> = None;
    loop {
        action = match action {
            AgentAction::CallModel(request) => {
                let request_id = request.request_id.clone();
                let interrupt = interrupts.wait_for_model_interrupt();
                let result = tokio::select! {
                    biased;
                    _ = interrupt => Err(HostError {
                        kind: MODEL_INTERRUPTED_KIND.into(),
                        message: "model request interrupted by Ctrl-C".into(),
                        request_id: Some(request_id.clone()),
                    }),
                    result = model.call_model(request) => result,
                };
                let result = match result {
                    Ok(result) => result,
                    Err(error) => {
                        return finish_failed_turn(
                            state,
                            work_id,
                            undo_available,
                            error,
                            request_id,
                        );
                    }
                };
                runtime.resume(AgentRuntimeInput::ModelCompleted(result))?
            }
            AgentAction::CallMcp(request) => {
                emit_text(&request.operation);
                let requires_approval = requires_approval(&request);
                if requires_approval {
                    match approved_write.take() {
                        Some(approved) if approved == request => {}
                        _ => {
                            let request_id = request.request_id.clone();
                            return finish_failed_turn(
                                state,
                                work_id,
                                undo_available,
                                HostError {
                                    kind: "approval_required".into(),
                                    message: "write dispatch did not match a one-shot approval"
                                        .into(),
                                    request_id: Some(request_id.clone()),
                                },
                                request_id,
                            );
                        }
                    }
                } else if request.effect != Some(CapabilityEffect::Read) {
                    let request_id = request.request_id.clone();
                    return finish_failed_turn(
                        state,
                        work_id,
                        undo_available,
                        HostError {
                            kind: "unknown_effect".into(),
                            message: "MCP capability effect is unknown; dispatch was denied".into(),
                            request_id: Some(request_id.clone()),
                        },
                        request_id,
                    );
                }
                let operation = request.operation.clone();
                let is_undoable_write =
                    request.effect == Some(CapabilityEffect::Write) && operation == "ugoite.save";
                let request_id = request.request_id.clone();
                let result = match mcp.call_mcp(request, &work_id).await {
                    Ok(result) => result,
                    Err(error) => {
                        if operation == "ugoite.undo" {
                            undo_available = false;
                        }
                        return finish_failed_turn(
                            state,
                            work_id,
                            undo_available,
                            HostError {
                                kind: if error.to_string().contains("MCP mutation receipt") {
                                    "mcp_unconfirmed".into()
                                } else {
                                    "mcp_transport_failure".into()
                                },
                                message: format!("MCP request failed: {error:#}"),
                                request_id: Some(request_id.clone()),
                            },
                            request_id,
                        );
                    }
                };
                if is_undoable_write && result.success {
                    undo_available = true;
                } else if operation == "ugoite.undo" && result.success {
                    undo_available = false;
                }
                let mcp_effect = step(
                    state,
                    ugoite_konase::KonaseEvent::McpCompleted(result.clone()),
                );
                if let Some(error) = mcp_effect.error {
                    bail!("Konase MCP completion rejected: {}", error.message);
                }
                state = mcp_effect.state;
                runtime.resume(AgentRuntimeInput::McpCompleted(result))?
            }
            AgentAction::Complete(outcome) => {
                let result = step(
                    state,
                    ugoite_konase::KonaseEvent::AgentProgress(AgentProgress {
                        job_id: outcome.job_id.clone(),
                        strategy_summary: None,
                        observation: None,
                        action: Some(AgentAction::Complete(outcome.clone())),
                    }),
                );
                if let Some(error) = result.error {
                    bail!("Konase completion rejected: {}", error.message);
                }
                return Ok(TurnResult::Completed(TurnOutcome {
                    outcome,
                    work_id,
                    undo_available,
                    knowledge: result.state.knowledge,
                }));
            }
            AgentAction::AskConfirmation(request) => {
                let approved = confirmations.confirm(&request);
                let confirmation = ugoite_konase::ConfirmationResult {
                    request_id: request.request_id.clone(),
                    approved,
                };
                let completion = step(
                    state,
                    KonaseEvent::ConfirmationCompleted(confirmation.clone()),
                );
                if let Some(error) = completion.error {
                    bail!("Konase confirmation completion rejected: {}", error.message);
                }
                state = completion.state;
                match runtime.resume(AgentRuntimeInput::ConfirmationCompleted(confirmation)) {
                    Ok(action) => {
                        if let AgentAction::CallMcp(request) = &action {
                            if requires_approval(request) {
                                approved_write = Some(request.clone());
                            }
                        }
                        action
                    }
                    Err(_error) if !approved => {
                        return Ok(TurnResult::Failed(TurnFailure {
                            error: HostError {
                                kind: "write_denied".into(),
                                message: "Konase write was denied or no interactive approval was available".into(),
                                request_id: Some(request.request_id),
                            },
                            work_id,
                            undo_available,
                            knowledge: state.knowledge,
                        }));
                    }
                    Err(error) => {
                        return finish_failed_turn(
                            state,
                            work_id,
                            undo_available,
                            HostError {
                                kind: error.kind,
                                message: error.message,
                                request_id: Some(request.request_id.clone()),
                            },
                            request.request_id,
                        );
                    }
                }
            }
        };
        let progress = step(
            state,
            ugoite_konase::KonaseEvent::AgentProgress(AgentProgress {
                job_id: job_id.clone(),
                strategy_summary: None,
                observation: None,
                action: Some(action.clone()),
            }),
        );
        if let Some(error) = progress.error {
            bail!("Konase progress rejected: {}", error.message);
        }
        state = progress.state;
        if let AgentAction::Complete(outcome) = action {
            return Ok(TurnResult::Completed(TurnOutcome {
                outcome,
                work_id,
                undo_available,
                knowledge: state.knowledge,
            }));
        }
    }
}

fn finish_failed_turn(
    state: KonaseState,
    work_id: String,
    undo_available: bool,
    mut error: HostError,
    request_id: String,
) -> Result<TurnResult> {
    if error.request_id.is_none() {
        error.request_id = Some(request_id);
    }
    let failed = step(state, KonaseEvent::HostFailed(error.clone()));
    if let Some(step_error) = failed.error {
        bail!("Konase rejected model host failure: {}", step_error.message);
    }
    Ok(TurnResult::Failed(TurnFailure {
        error,
        work_id,
        undo_available,
        knowledge: failed.state.knowledge,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::State,
        http::{HeaderMap, StatusCode},
        response::{IntoResponse, Response},
        routing::post,
        Json, Router,
    };
    use serde_json::json;
    use std::{collections::HashMap, sync::Arc};
    use tokio::{
        net::TcpListener,
        sync::{Mutex, Notify},
        task::JoinHandle,
    };

    struct ScriptedModel {
        responses: Vec<std::result::Result<ModelResult, HostError>>,
    }

    #[async_trait]
    impl ModelHost for ScriptedModel {
        async fn call_model(
            &mut self,
            request: ModelRequest,
        ) -> std::result::Result<ModelResult, HostError> {
            let mut response = self.responses.remove(0)?;
            response.request_id = request.request_id;
            Ok(response)
        }
    }

    #[derive(Clone, Default)]
    struct TestInterrupt {
        notify: Arc<Notify>,
    }

    impl TestInterrupt {
        fn interrupt(&self) {
            self.notify.notify_one();
        }
    }

    impl ModelInterruptSource for TestInterrupt {
        fn wait_for_model_interrupt(&self) -> ModelInterruptFuture<'_> {
            Box::pin(self.notify.notified())
        }
    }

    struct InterruptibleModel {
        calls: usize,
        started: Arc<Notify>,
        save_before_interrupt: bool,
    }

    #[async_trait]
    impl ModelHost for InterruptibleModel {
        async fn call_model(
            &mut self,
            request: ModelRequest,
        ) -> std::result::Result<ModelResult, HostError> {
            self.calls += 1;
            let request_id = request.request_id;
            if self.save_before_interrupt && self.calls == 1 {
                return Ok(ModelResult {
                    request_id,
                    text: None,
                    tool_calls: vec![ugoite_konase::ModelToolCall {
                        id: "call-save".into(),
                        name: "ugoite.save".into(),
                        arguments: json!({"form": "Entry", "fields": {"title": "Saved"}}),
                    }],
                });
            }
            let interrupt_call = if self.save_before_interrupt { 2 } else { 1 };
            if self.calls == interrupt_call {
                self.started.notify_one();
                return std::future::pending().await;
            }
            Ok(ModelResult {
                request_id,
                text: Some("完了しました。".into()),
                tool_calls: vec![],
            })
        }
    }

    struct ScriptedMcp {
        operations: Vec<String>,
        work_ids: Vec<String>,
        fail_save: bool,
        fail_undo: bool,
    }

    struct SelectedResourceMcp {
        resources: Vec<ResourceContent>,
        operations: Vec<String>,
    }

    #[async_trait]
    impl McpHost for SelectedResourceMcp {
        async fn call_mcp(&mut self, request: McpRequest, _work_id: &str) -> Result<McpResult> {
            self.operations.push(request.operation.clone());
            let requested_uri = request.arguments.get("uri").and_then(Value::as_str);
            let selected = (request.operation == "resources/read")
                .then(|| {
                    self.resources
                        .iter()
                        .find(|resource| Some(resource.uri.as_str()) == requested_uri)
                })
                .flatten();
            Ok(McpResult {
                request_id: request.request_id,
                operation: request.operation,
                success: selected.is_some(),
                observation: None,
                resources: vec![],
                resource_contents: selected.cloned().into_iter().collect(),
                error: selected.is_none().then_some("resource read denied".into()),
            })
        }

        async fn capabilities(&self) -> Vec<Capability> {
            vec![]
        }
    }

    struct CapturingModel {
        requests: Vec<ModelRequest>,
    }

    #[async_trait]
    impl ModelHost for CapturingModel {
        async fn call_model(
            &mut self,
            request: ModelRequest,
        ) -> std::result::Result<ModelResult, HostError> {
            let request_id = request.request_id.clone();
            self.requests.push(request);
            Ok(ModelResult {
                request_id,
                text: Some("done".into()),
                tool_calls: vec![],
            })
        }
    }

    struct DenySelectedContext;

    impl SelectedContextConfirmation for DenySelectedContext {
        fn confirm_selected_context(&self) -> bool {
            false
        }
    }

    #[test]
    fn write_tool_requests_keep_model_arguments_and_bind_the_work_id() {
        let mut run_ids = Vec::new();
        for (operation, arguments) in [
            (
                "ugoite.save",
                json!({"content":"---\nform: Entry\n---\n# Saved"}),
            ),
            ("ugoite.undo", json!({})),
        ] {
            let request = McpRequest {
                request_id: "request-1".into(),
                server: "ugoite".into(),
                operation: operation.into(),
                arguments: arguments
                    .as_object()
                    .cloned()
                    .unwrap()
                    .into_iter()
                    .collect(),
                effect: Some(CapabilityEffect::Write),
            };
            let (name, params) = write_tool_request(request, "work-1").unwrap();
            assert_eq!(name, operation);
            assert_eq!(params.name, operation);
            assert_eq!(
                params.arguments.unwrap(),
                arguments.as_object().cloned().unwrap()
            );
            let run_id = params
                .meta
                .as_ref()
                .and_then(|meta| meta.0 .0.get("ugoite/runId"))
                .cloned();
            assert_eq!(run_id, Some(Value::String("work-1".into())));
            run_ids.push(run_id);
        }
        assert_eq!(
            run_ids,
            [
                Some(Value::String("work-1".into())),
                Some(Value::String("work-1".into()))
            ]
        );
    }

    #[test]
    fn listed_tool_schema_reaches_konase_capability() {
        let schema = json!({
            "type": "object",
            "properties": {"q": {"type": "string"}},
            "required": ["q"],
            "additionalProperties": false
        })
        .as_object()
        .cloned()
        .unwrap();
        let capability = capability_from_tool(
            Tool::new("ugoite.search", "Search entries", schema)
                .annotate(rmcp::model::ToolAnnotations::new().read_only(true)),
        );

        assert_eq!(capability.name, "ugoite.search");
        assert_eq!(
            capability.input_schema,
            Some(json!({
                "type": "object",
                "properties": {"q": {"type": "string"}},
                "required": ["q"],
                "additionalProperties": false
            }))
        );
        assert_eq!(capability.effect, Some(CapabilityEffect::Read));

        let write = capability_from_tool(
            Tool::new("ugoite.save", "Save an entry", serde_json::Map::new())
                .annotate(rmcp::model::ToolAnnotations::new().read_only(false)),
        );
        assert_eq!(write.effect, Some(CapabilityEffect::Write));

        let unannotated_write = capability_from_tool(Tool::new(
            "ugoite.save",
            "Save an entry",
            serde_json::Map::new(),
        ));
        assert_eq!(unannotated_write.effect, Some(CapabilityEffect::Write));
    }

    #[test]
    fn save_and_undo_receipts_must_match_the_mutation_contract() {
        assert!(valid_save_receipt(&json!({
            "id":"entry-1", "uri":"ugoite://entry/entry-1", "status":"created", "_untrusted_content":true
        })));
        for invalid in [
            json!({"id":"entry-1", "uri":"ugoite://entry/entry-1", "status":"created"}),
            json!({"id":"entry-1", "uri":"ugoite://entry/entry-1", "status":"deleted", "_untrusted_content":true}),
            json!({"id":"entry-1", "uri":"https://example.test/entry-1", "status":"updated", "_untrusted_content":true}),
        ] {
            assert!(!valid_save_receipt(&invalid));
        }
        assert!(valid_undo_receipt(
            &json!({"run_id":"work-1", "reverted_change_count":1, "_untrusted_content":true}),
            "work-1"
        ));
        assert!(!valid_undo_receipt(
            &json!({"run_id":"another-work", "reverted_change_count":1, "_untrusted_content":true}),
            "work-1"
        ));
    }

    #[test]
    fn approval_input_accepts_only_an_explicit_yes() {
        assert!(confirmation_answer_is_approved("y\n"));
        assert!(confirmation_answer_is_approved(" YES "));
        for answer in ["", "n", "no", "ok", "true"] {
            assert!(!confirmation_answer_is_approved(answer));
        }
    }

    fn selected_form_resource() -> ResourceContent {
        ResourceContent {
            uri: "ugoite://form/00000000-0000-0000-0000-0000000000a1".into(),
            content: json!({
                "id":"00000000-0000-0000-0000-0000000000a1",
                "name":"Expense",
                "description":"Expense records",
                "fields":{"amount":{"type":"number","required":true}},
                "_untrusted_content":true
            })
            .to_string(),
        }
    }

    #[test]
    fn selected_uri_validation_deduplicates_in_selection_order_and_caps_count() {
        let uri = "ugoite://form/00000000-0000-0000-0000-0000000000a1".to_owned();
        assert_eq!(
            canonical_selected_uris(&[uri.clone(), uri.clone()]).unwrap(),
            [uri]
        );
        assert!(canonical_selected_uris(&["ugoite://entry/a/b".into()]).is_err());
        assert!(canonical_selected_uris(
            &(0..5)
                .map(|_| "ugoite://entry/00000000-0000-0000-0000-0000000000b1".to_owned())
                .collect::<Vec<_>>()
        )
        .is_err());
    }

    #[test]
    fn non_interactive_selected_context_requires_explicit_send_intent() {
        let selected = vec!["ugoite://form/00000000-0000-0000-0000-0000000000a1".to_owned()];
        assert!(validate_selected_context_mode(&[], false, false).is_ok());
        assert!(validate_selected_context_mode(&selected, true, false).is_ok());
        assert!(validate_selected_context_mode(&selected, false, false).is_err());
        assert!(validate_selected_context_mode(&selected, false, true).is_ok());
        assert!(validate_selected_context_mode(&[], false, true).is_err());
    }

    #[tokio::test]
    async fn selected_resource_is_read_before_model_and_normalized_context_is_sent() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../ugoite-konase/fixtures/selected-context.json"
        ))
        .unwrap();
        let selected_resources: Vec<ResourceContent> =
            serde_json::from_value(fixture["selected"].clone()).unwrap();
        let selected_uris = selected_resources
            .iter()
            .map(|resource| resource.uri.clone())
            .collect::<Vec<_>>();
        let unselected: ResourceContent =
            serde_json::from_value(fixture["unselected"].clone()).unwrap();
        let unselected_id = unselected.uri.rsplit('/').next().unwrap().to_owned();
        let mut mcp = SelectedResourceMcp {
            resources: [selected_resources, vec![unselected]].concat(),
            operations: vec![],
        };
        let mut model = CapturingModel { requests: vec![] };
        let result = run_turn_with_selected_context(
            &mut model,
            &mut mcp,
            &NeverModelInterrupt,
            &ApproveConfirmation,
            SelectedTurnOptions {
                prompt: "Explain this Form",
                capabilities: &[],
                selected_uris: &selected_uris,
                context_confirmation: None,
            },
        )
        .await
        .unwrap();
        assert!(matches!(result, TurnResult::Completed(_)));
        assert_eq!(mcp.operations, ["resources/read", "resources/read"]);
        assert_eq!(model.requests.len(), 1);
        assert!(model.requests[0].prompt.contains("Expense"));
        assert!(model.requests[0].prompt.contains("_untrusted_content"));
        assert!(model.requests[0].prompt.contains("amount"));
        assert!(model.requests[0]
            .prompt
            .contains("00000000-0000-0000-0000-0000000000b1"));
        assert!(!model.requests[0].prompt.contains(&unselected_id));
        assert!(!model.requests[0]
            .prompt
            .contains("PRIVATE UNSELECTED VALUE"));
    }

    #[tokio::test]
    async fn selected_context_denial_happens_after_read_but_before_model_or_write() {
        let resource = selected_form_resource();
        let uri = resource.uri.clone();
        let mut mcp = SelectedResourceMcp {
            resources: vec![resource],
            operations: vec![],
        };
        let mut model = CapturingModel { requests: vec![] };
        let result = run_turn_with_selected_context(
            &mut model,
            &mut mcp,
            &NeverModelInterrupt,
            &ApproveConfirmation,
            SelectedTurnOptions {
                prompt: "Explain this Form",
                capabilities: &[],
                selected_uris: &[uri],
                context_confirmation: Some(&DenySelectedContext),
            },
        )
        .await;
        let Err(error) = result else {
            panic!("selected Context denial must stop before model submission");
        };
        assert!(error.to_string().contains("not approved"));
        assert_eq!(mcp.operations, ["resources/read"]);
        assert!(model.requests.is_empty());
    }

    #[tokio::test]
    async fn denied_or_mismatched_selected_read_stops_before_model_call() {
        let mut mcp = SelectedResourceMcp {
            resources: vec![selected_form_resource()],
            operations: vec![],
        };
        let mut model = CapturingModel { requests: vec![] };
        let result = run_turn_with_selected_context(
            &mut model,
            &mut mcp,
            &NeverModelInterrupt,
            &ApproveConfirmation,
            SelectedTurnOptions {
                prompt: "Explain this Form",
                capabilities: &[],
                selected_uris: &["ugoite://entry/00000000-0000-0000-0000-0000000000b1".into()],
                context_confirmation: None,
            },
        )
        .await;
        assert!(result.is_err());
        assert_eq!(mcp.operations, ["resources/read"]);
        assert!(model.requests.is_empty());
    }

    #[async_trait]
    impl McpHost for ScriptedMcp {
        async fn call_mcp(&mut self, request: McpRequest, work_id: &str) -> Result<McpResult> {
            self.operations.push(request.operation.clone());
            self.work_ids.push(work_id.to_owned());
            let is_search = request.operation == "ugoite.search";
            let is_save = request.operation == "ugoite.save";
            let success = !(self.fail_save && is_save
                || self.fail_undo && request.operation == "ugoite.undo");
            let error = (!success).then_some(if is_save {
                "save failed"
            } else {
                "undo failed"
            });
            Ok(McpResult {
                request_id: request.request_id,
                operation: request.operation,
                success,
                observation: is_search.then_some(Observation {
                    id: "search-1".into(),
                    kind: ObservationKind::Mcp,
                    summary: "WebAssembly note".into(),
                    facts: Default::default(),
                    resource_references: vec![ResourceReference {
                        uri: "ugoite://entry/1".into(),
                        label: Some("WebAssembly".into()),
                    }],
                }),
                resources: vec![],
                resource_contents: (!is_search)
                    .then_some(ResourceContent {
                        uri: "ugoite://entry/1".into(),
                        content: "WebAssembly memo body".into(),
                    })
                    .into_iter()
                    .collect(),
                error: error.map(Into::into),
            })
        }

        async fn capabilities(&self) -> Vec<Capability> {
            vec![
                Capability {
                    name: "ugoite.search".into(),
                    description: "search entries".into(),
                    input_schema: Some(json!({
                        "type": "object",
                        "properties": {"q": {"type": "string"}},
                        "required": ["q"]
                    })),
                    effect: Some(CapabilityEffect::Read),
                },
                Capability {
                    name: "resources/read".into(),
                    description: "read an entry".into(),
                    input_schema: Some(resources_read_schema()),
                    effect: Some(CapabilityEffect::Read),
                },
                Capability {
                    name: "ugoite.save".into(),
                    description: "save an entry".into(),
                    input_schema: Some(json!({
                        "type": "object",
                        "properties": {"content": {"type": "string"}},
                        "required": ["content"]
                    })),
                    effect: Some(CapabilityEffect::Write),
                },
                Capability {
                    name: "ugoite.undo".into(),
                    description: "undo changes".into(),
                    input_schema: Some(json!({"type": "object"})),
                    effect: Some(CapabilityEffect::Write),
                },
            ]
        }
    }

    #[tokio::test]
    async fn scripted_search_read_then_answer_completes_the_konase_loop() {
        let mut model = ScriptedModel {
            responses: vec![
                Ok(ModelResult {
                    request_id: String::new(),
                    text: None,
                    tool_calls: vec![ugoite_konase::ModelToolCall {
                        id: "call-search".into(),
                        name: "ugoite.search".into(),
                        arguments: json!({"query": "WebAssembly"}),
                    }],
                }),
                Ok(ModelResult {
                    request_id: String::new(),
                    text: Some("1件見つかりました。".into()),
                    tool_calls: vec![ugoite_konase::ModelToolCall {
                        id: "call-read".into(),
                        name: "resources/read".into(),
                        arguments: json!({"uri": "ugoite://entry/1"}),
                    }],
                }),
                Ok(ModelResult {
                    request_id: String::new(),
                    text: Some("WebAssemblyのメモを確認しました。".into()),
                    tool_calls: vec![],
                }),
            ],
        };
        let mut mcp = ScriptedMcp {
            operations: vec![],
            work_ids: vec![],
            fail_save: false,
            fail_undo: false,
        };
        let capabilities = mcp.capabilities().await;
        let TurnResult::Completed(outcome) = run_turn(
            &mut model,
            &mut mcp,
            "WebAssemblyのメモを探して保存して",
            &capabilities,
        )
        .await
        .unwrap() else {
            panic!("expected completed turn");
        };
        assert_eq!(outcome.outcome.summary, "WebAssemblyのメモを確認しました。");
        assert_eq!(outcome.knowledge, KnowledgeOutcome::Unchanged);
        assert!(!outcome.undo_available);
        assert_eq!(mcp.operations, ["ugoite.search", "resources/read"]);
        assert_eq!(mcp.work_ids.len(), 2);
        assert_eq!(mcp.work_ids[0], mcp.work_ids[1]);
    }

    #[tokio::test]
    async fn denied_noninteractive_write_sends_no_mcp_call() {
        struct DenyConfirmation;
        impl ConfirmationSource for DenyConfirmation {
            fn confirm(&self, _request: &ugoite_konase::ConfirmationRequest) -> bool {
                false
            }
        }
        let mut model = ScriptedModel {
            responses: vec![Ok(ModelResult {
                request_id: String::new(),
                text: None,
                tool_calls: vec![ugoite_konase::ModelToolCall {
                    id: "call-save".into(),
                    name: "ugoite.save".into(),
                    arguments: json!({"form":"Entry", "fields":{"title":"Private text"}}),
                }],
            })],
        };
        let mut mcp = ScriptedMcp {
            operations: vec![],
            work_ids: vec![],
            fail_save: false,
            fail_undo: false,
        };
        let capabilities = mcp.capabilities().await;
        let result = run_turn_with_interrupts(
            &mut model,
            &mut mcp,
            "save this",
            &capabilities,
            &NeverModelInterrupt,
            &DenyConfirmation,
        )
        .await
        .unwrap();
        let TurnResult::Failed(failure) = result else {
            panic!("expected denied turn")
        };
        assert_eq!(failure.error.kind, "write_denied");
        assert_eq!(failure.knowledge, KnowledgeOutcome::Unchanged);
        assert!(!failure.undo_available);
        assert!(mcp.operations.is_empty());
    }

    #[tokio::test]
    async fn later_denial_keeps_the_first_confirmed_save_and_undo() {
        struct ApproveThenDeny(std::sync::atomic::AtomicUsize);
        impl ConfirmationSource for ApproveThenDeny {
            fn confirm(&self, _request: &ugoite_konase::ConfirmationRequest) -> bool {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0
            }
        }
        let mut model = ScriptedModel {
            responses: vec![Ok(ModelResult {
                request_id: String::new(),
                text: None,
                tool_calls: vec![
                    ugoite_konase::ModelToolCall {
                        id: "call-save-a".into(),
                        name: "ugoite.save".into(),
                        arguments: json!({"form":"Entry", "fields":{"title":"A"}}),
                    },
                    ugoite_konase::ModelToolCall {
                        id: "call-save-b".into(),
                        name: "ugoite.save".into(),
                        arguments: json!({"form":"Entry", "fields":{"title":"B"}}),
                    },
                ],
            })],
        };
        let mut mcp = ScriptedMcp {
            operations: vec![],
            work_ids: vec![],
            fail_save: false,
            fail_undo: false,
        };
        let capabilities = mcp.capabilities().await;
        let result = run_turn_with_interrupts(
            &mut model,
            &mut mcp,
            "save two entries",
            &capabilities,
            &NeverModelInterrupt,
            &ApproveThenDeny(std::sync::atomic::AtomicUsize::new(0)),
        )
        .await
        .unwrap();
        let TurnResult::Failed(failure) = result else {
            panic!("expected the second confirmation to stop the Job")
        };
        assert_eq!(failure.knowledge, KnowledgeOutcome::Saved);
        assert!(failure.undo_available);
        assert_eq!(mcp.operations, ["ugoite.save"]);
    }

    #[tokio::test]
    async fn signal_coordinator_only_delivers_interrupts_to_a_model_waiter() {
        let coordinator = SignalCoordinator::new();
        let waiter = coordinator.wait_for_model_interrupt();

        assert!(coordinator.interrupt_model_wait());
        waiter.await;
        assert!(!coordinator.interrupt_model_wait());
    }

    #[tokio::test]
    async fn interrupting_model_wait_fails_work_preserves_save_and_allows_next_prompt() {
        let interrupt = TestInterrupt::default();
        let started = Arc::new(Notify::new());
        let model = InterruptibleModel {
            calls: 0,
            started: started.clone(),
            save_before_interrupt: true,
        };
        let mcp = ScriptedMcp {
            operations: vec![],
            work_ids: vec![],
            fail_save: false,
            fail_undo: false,
        };
        let capabilities = mcp.capabilities().await;
        let task_interrupt = interrupt.clone();
        let task_capabilities = capabilities.clone();
        let task = tokio::spawn(async move {
            let mut model = model;
            let mut mcp = mcp;
            let result = run_turn_with_interrupts(
                &mut model,
                &mut mcp,
                "保存して",
                &task_capabilities,
                &task_interrupt,
                &ApproveConfirmation,
            )
            .await;
            (result, model, mcp)
        });

        started.notified().await;
        interrupt.interrupt();
        let (result, mut model, mut mcp) = task.await.unwrap();
        let TurnResult::Failed(failure) = result.unwrap() else {
            panic!("expected interrupted turn to fail");
        };

        assert_eq!(failure.error.kind, MODEL_INTERRUPTED_KIND);
        assert_eq!(failure.error.message, "model request interrupted by Ctrl-C");
        assert!(failure.error.request_id.is_some());
        assert_eq!(failure.knowledge, KnowledgeOutcome::Saved);
        assert!(failure.undo_available);
        assert_eq!(mcp.operations, ["ugoite.save"]);

        let TurnResult::Completed(next) = run_turn_with_interrupts(
            &mut model,
            &mut mcp,
            "次の質問",
            &capabilities,
            &interrupt,
            &ApproveConfirmation,
        )
        .await
        .unwrap() else {
            panic!("expected the next prompt to complete");
        };
        assert_eq!(next.outcome.summary, "完了しました。");
    }

    #[tokio::test]
    async fn failed_undo_keeps_the_same_work_id_for_a_retry() {
        let mut mcp = ScriptedMcp {
            operations: vec![],
            work_ids: vec![],
            fail_save: false,
            fail_undo: true,
        };
        let mut last_work_id = Some("work-1".to_owned());

        let error = undo_last_work(&mut mcp, &mut last_work_id)
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "undo failed");
        assert_eq!(last_work_id.as_deref(), Some("work-1"));

        mcp.fail_undo = false;
        assert!(undo_last_work(&mut mcp, &mut last_work_id).await.unwrap());
        assert!(last_work_id.is_none());
        assert_eq!(mcp.operations, ["ugoite.undo", "ugoite.undo"]);
        assert_eq!(mcp.work_ids, ["work-1", "work-1"]);
    }

    #[tokio::test]
    async fn failed_save_is_reported_as_failed_knowledge_without_undo() {
        let mut model = ScriptedModel {
            responses: vec![
                Ok(ModelResult {
                    request_id: String::new(),
                    text: None,
                    tool_calls: vec![ugoite_konase::ModelToolCall {
                        id: "call-save".into(),
                        name: "ugoite.save".into(),
                        arguments: json!({"form": "Entry", "fields": {"title": "Failed"}}),
                    }],
                }),
                Ok(ModelResult {
                    request_id: String::new(),
                    text: Some("保存を試みました。".into()),
                    tool_calls: vec![],
                }),
            ],
        };
        let mut mcp = ScriptedMcp {
            operations: vec![],
            work_ids: vec![],
            fail_save: true,
            fail_undo: false,
        };
        let capabilities = mcp.capabilities().await;
        let TurnResult::Completed(outcome) =
            run_turn(&mut model, &mut mcp, "保存して", &capabilities)
                .await
                .unwrap()
        else {
            panic!("expected completed turn");
        };

        assert_eq!(outcome.knowledge, KnowledgeOutcome::WriteFailed);
        assert!(!outcome.undo_available);
        assert_eq!(mcp.operations, ["ugoite.save"]);
    }

    #[test]
    fn model_timeout_configuration_requires_positive_seconds() {
        assert_eq!(
            parse_model_timeout(None).unwrap(),
            Duration::from_secs(DEFAULT_MODEL_TIMEOUT_SECS)
        );
        assert_eq!(
            parse_model_timeout(Some(" 7 ")).unwrap(),
            Duration::from_secs(7)
        );
        assert!(parse_model_timeout(Some("0")).is_err());
        assert!(parse_model_timeout(Some("never")).is_err());
    }

    async fn slow_model_provider() -> Response {
        tokio::time::sleep(Duration::from_secs(1)).await;
        (
            StatusCode::OK,
            Json(json!({
                "choices": [{
                    "message": {"content": "late", "tool_calls": []}
                }]
            })),
        )
            .into_response()
    }

    async fn unavailable_model_provider() -> Response {
        (StatusCode::SERVICE_UNAVAILABLE, "provider is busy").into_response()
    }

    async fn empty_model_provider() -> Response {
        Json(json!({
            "choices": [{
                "finish_reason": "length",
                "message": {"content": "   ", "tool_calls": []}
            }]
        }))
        .into_response()
    }

    #[tokio::test]
    async fn slow_model_provider_returns_a_request_scoped_timeout_host_error() {
        let app = Router::new().route("/chat/completions", post(slow_model_provider));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let mut host = OpenAiModelHost {
            client: reqwest::Client::new(),
            base_url: format!("http://{address}"),
            api_key: "test-key".into(),
            model: "test-model".into(),
            timeout: Duration::from_millis(10),
        };

        let error = host
            .call_model(ModelRequest {
                request_id: "request-timeout".into(),
                prompt: "wait".into(),
                history: vec![],
                tools: vec![],
            })
            .await
            .unwrap_err();

        assert_eq!(error.kind, "model_timeout");
        assert_eq!(error.request_id.as_deref(), Some("request-timeout"));
        assert!(error.message.contains("timed out"));
        server.abort();
    }

    #[tokio::test]
    async fn model_provider_failure_returns_a_request_scoped_host_error() {
        let app = Router::new().route("/chat/completions", post(unavailable_model_provider));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let mut host = OpenAiModelHost {
            client: reqwest::Client::new(),
            base_url: format!("http://{address}"),
            api_key: "test-key".into(),
            model: "test-model".into(),
            timeout: Duration::from_secs(1),
        };

        let error = host
            .call_model(ModelRequest {
                request_id: "request-provider-failure".into(),
                prompt: "try".into(),
                history: vec![],
                tools: vec![],
            })
            .await
            .unwrap_err();

        assert_eq!(error.kind, "model_provider");
        assert_eq!(
            error.request_id.as_deref(),
            Some("request-provider-failure")
        );
        assert!(error.message.contains("503 Service Unavailable"));
        assert!(error.message.contains("provider is busy"));
        server.abort();
    }

    #[tokio::test]
    async fn empty_model_completion_returns_a_retryable_response_error() {
        let app = Router::new().route("/chat/completions", post(empty_model_provider));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let mut host = OpenAiModelHost {
            client: reqwest::Client::new(),
            base_url: format!("http://{address}"),
            api_key: "test-key".into(),
            model: "test-model".into(),
            timeout: Duration::from_secs(1),
        };

        let error = host
            .call_model(ModelRequest {
                request_id: "request-empty-completion".into(),
                prompt: "try".into(),
                history: vec![],
                tools: vec![],
            })
            .await
            .unwrap_err();

        assert_eq!(error.kind, "model_response");
        assert!(error.message.contains("output limit"));
        assert!(error.message.contains("retry"));
        server.abort();
    }

    #[tokio::test]
    async fn model_failure_marks_work_failed_without_discarding_saved_knowledge() {
        let mut model = ScriptedModel {
            responses: vec![
                Ok(ModelResult {
                    request_id: String::new(),
                    text: None,
                    tool_calls: vec![ugoite_konase::ModelToolCall {
                        id: "call-save".into(),
                        name: "ugoite.save".into(),
                        arguments: json!({"form": "Entry", "fields": {"title": "Saved"}}),
                    }],
                }),
                Err(HostError {
                    kind: "model_timeout".into(),
                    message: "model request timed out after 10ms".into(),
                    request_id: None,
                }),
            ],
        };
        let mut mcp = ScriptedMcp {
            operations: vec![],
            work_ids: vec![],
            fail_save: false,
            fail_undo: false,
        };
        let capabilities = mcp.capabilities().await;

        let TurnResult::Failed(failure) = run_turn(&mut model, &mut mcp, "保存して", &capabilities)
            .await
            .unwrap()
        else {
            panic!("expected failed turn");
        };

        assert_eq!(failure.error.kind, "model_timeout");
        assert!(failure.error.request_id.is_some());
        assert_eq!(failure.knowledge, KnowledgeOutcome::Saved);
        assert!(failure.undo_available);
        assert_eq!(mcp.operations, ["ugoite.save"]);
    }

    #[derive(Clone)]
    struct FakeMcpState {
        requests: Arc<Mutex<Vec<RecordedMcpRequest>>>,
        fail_undo: bool,
    }

    #[derive(Debug)]
    struct RecordedMcpRequest {
        body: Value,
        headers: HashMap<String, String>,
    }

    async fn fake_mcp(
        State(state): State<FakeMcpState>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Response {
        let method = body
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let name = body
            .pointer("/params/name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        state.requests.lock().await.push(RecordedMcpRequest {
            body: body.clone(),
            headers: headers
                .iter()
                .filter_map(|(name, value)| {
                    Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned()))
                })
                .collect(),
        });

        if state.fail_undo && method == "tools/call" && name == "ugoite.undo" {
            return (
                StatusCode::BAD_GATEWAY,
                "upstream undo failed: storage unavailable",
            )
                .into_response();
        }

        let id = body.get("id").cloned().unwrap_or(Value::Null);
        let result = match (method.as_str(), name) {
            ("server/discover", _) => json!({
                "resultType": "complete",
                "supportedVersions": ["2026-07-28"],
                "capabilities": {
                    "tools": {"listChanged": false},
                    "resources": {"listChanged": false, "subscribe": false}
                },
                "ttlMs": 60000,
                "cacheScope": "private"
            }),
            ("tools/list", _) => json!({
                "resultType": "complete",
                "tools": [
                    {
                        "name": "ugoite.search",
                        "description": "Search entries",
                        "inputSchema": {
                            "type": "object",
                            "properties": {"query": {"type": "string"}},
                            "additionalProperties": false
                        }
                    },
                    {
                        "name": "ugoite.save",
                        "description": "Save an entry",
                        "inputSchema": {
                            "type": "object",
                            "properties": {"content": {"type": "string"}},
                            "additionalProperties": false
                        }
                    },
                    {
                        "name": "ugoite.undo",
                        "description": "Undo a Work",
                        "inputSchema": {
                            "type": "object",
                            "additionalProperties": false
                        }
                    }
                ]
            }),
            ("tools/call", "ugoite.search") => json!({
                "resultType": "complete",
                "content": [
                    {"type": "text", "text": "search result"},
                    {"type": "resource_link", "uri": "ugoite://entry/1", "name": "Entry 1"}
                ]
            }),
            ("tools/call", "ugoite.save") => json!({
                "resultType": "complete",
                "structuredContent": {"id":"entry-1", "uri":"ugoite://entry/entry-1", "status":"updated", "_untrusted_content":true},
                "content": [{"type": "text", "text": "saved"}]
            }),
            ("tools/call", "ugoite.undo") => json!({
                "resultType": "complete",
                "structuredContent": {"run_id":"work-1", "reverted_change_count":1, "_untrusted_content":true},
                "content": [{"type": "text", "text": "undone"}]
            }),
            ("resources/read", _) => json!({
                "resultType": "complete",
                "contents": [{
                    "uri": "ugoite://entry/1",
                    "mimeType": "text/plain",
                    "text": "entry body"
                }]
            }),
            _ => json!({"resultType": "complete"}),
        };
        (
            StatusCode::OK,
            Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result
            })),
        )
            .into_response()
    }

    async fn spawn_fake_mcp_server(
        fail_undo: bool,
    ) -> (String, Arc<Mutex<Vec<RecordedMcpRequest>>>, JoinHandle<()>) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route("/mcp", post(fake_mcp))
            .with_state(FakeMcpState {
                requests: requests.clone(),
                fail_undo,
            });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{address}/mcp"), requests, task)
    }

    async fn connect_test_host(endpoint: &str) -> RmcpMcpHost {
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(endpoint),
        );
        let client = ClientConfig::default()
            .serve_with_lifecycle(
                transport,
                ClientLifecycleMode::Discover {
                    preferred_versions: vec![ProtocolVersion::V_2026_07_28],
                },
            )
            .await
            .unwrap();
        let listed = client.list_tools(None).await.unwrap();
        let capabilities = listed.tools.into_iter().map(capability_from_tool).collect();
        RmcpMcpHost {
            client,
            capabilities,
        }
    }

    #[tokio::test]
    async fn real_rmcp_client_completes_stateless_search_read_save_then_undo() {
        let (endpoint, requests, task) = spawn_fake_mcp_server(false).await;
        let mut host = connect_test_host(&endpoint).await;

        for (operation, arguments) in [
            ("ugoite.search", json!({"query": "entry"})),
            ("resources/read", json!({"uri": "ugoite://entry/1"})),
            ("ugoite.save", json!({"content": "updated"})),
            ("ugoite.undo", json!({})),
        ] {
            let result = host
                .call_mcp(
                    McpRequest {
                        request_id: format!("request-{operation}"),
                        server: "ugoite".into(),
                        operation: operation.into(),
                        arguments: arguments
                            .as_object()
                            .cloned()
                            .unwrap()
                            .into_iter()
                            .collect(),
                        effect: Some(if operation == "ugoite.undo" {
                            CapabilityEffect::Write
                        } else {
                            CapabilityEffect::Read
                        }),
                    },
                    "work-1",
                )
                .await
                .unwrap();
            assert!(result.success, "{operation} failed: {:?}", result.error);
        }

        let requests = requests.lock().await;
        assert_eq!(
            requests
                .iter()
                .map(|request| request.body["method"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "server/discover",
                "tools/list",
                "tools/call",
                "resources/read",
                "tools/call",
                "tools/call"
            ]
        );
        for request in requests.iter().skip(2) {
            assert_eq!(
                request.headers.get("mcp-protocol-version"),
                Some(&"2026-07-28".into())
            );
            assert_eq!(
                request.headers.get("mcp-method"),
                request.body["method"].as_str().map(Into::into).as_ref()
            );
        }
        assert_eq!(
            requests[2].headers.get("mcp-name"),
            Some(&"ugoite.search".into())
        );
        assert_eq!(
            requests[3].headers.get("mcp-name"),
            Some(&"ugoite://entry/1".into())
        );
        assert_eq!(
            requests[4].headers.get("mcp-name"),
            Some(&"ugoite.save".into())
        );
        assert_eq!(
            requests[5].headers.get("mcp-name"),
            Some(&"ugoite.undo".into())
        );
        let save_run_id = requests[4]
            .body
            .pointer("/params/_meta/ugoite~1runId")
            .and_then(Value::as_str);
        let undo_run_id = requests[5]
            .body
            .pointer("/params/_meta/ugoite~1runId")
            .and_then(Value::as_str);
        assert_eq!(save_run_id, Some("work-1"));
        assert_eq!(undo_run_id, save_run_id);

        task.abort();
    }

    #[tokio::test]
    async fn rmcp_transport_error_keeps_status_and_body_in_the_cli_error_chain() {
        let (endpoint, _requests, task) = spawn_fake_mcp_server(true).await;
        let mut host = connect_test_host(&endpoint).await;
        let error = host
            .call_mcp(
                McpRequest {
                    request_id: "undo-request".into(),
                    server: "ugoite".into(),
                    operation: "ugoite.undo".into(),
                    arguments: Default::default(),
                    effect: Some(CapabilityEffect::Write),
                },
                "work-1",
            )
            .await
            .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("call ugoite.undo MCP tool"));
        assert!(message.contains("502"));
        assert!(message.contains("storage unavailable"));
        task.abort();
    }
}
