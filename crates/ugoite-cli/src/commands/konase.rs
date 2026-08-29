use crate::config::{load_config, non_empty_env_value, validated_base_url, EndpointMode};
use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use clap::Args;
use rmcp::{
    model::{ClientInfo, *},
    transport::{streamable_http_client::StreamableHttpClientTransportConfig, *},
    ClientLifecycleMode, ClientServiceExt, RoleClient,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, IsTerminal, Write};
use ugoite_konase::{
    step, AgentAction, AgentProgress, AgentRuntime, AgentRuntimeInput, Capability, JobOutcome,
    McpRequest, McpResult, ModelMessage, ModelRequest, ModelResult, Observation, ObservationKind,
    ResourceContent, ResourceReference, UserRequest,
};
use ugoite_konase_rig::RigAgentRuntime;
use uuid::Uuid;

#[derive(Args)]
pub struct KonaseCmd {
    /// Run one request and exit instead of reading an interactive stdin loop.
    #[arg(long)]
    pub prompt: Option<String>,
}

#[async_trait]
trait ModelHost {
    async fn call_model(&mut self, request: ModelRequest) -> Result<ModelResult>;
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
        })
    }
}

#[async_trait]
impl ModelHost for OpenAiModelHost {
    async fn call_model(&mut self, request: ModelRequest) -> Result<ModelResult> {
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
            .context("call model provider")?;
        let status = response.status();
        let body: ChatResponse = response.json().await.context("decode model response")?;
        if !status.is_success() {
            bail!("model provider returned {status}");
        }
        let message = body
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("model provider returned no choices"))?
            .message;
        let mut tool_calls = Vec::new();
        for call in message.tool_calls {
            let arguments = serde_json::from_str(&call.function.arguments).with_context(|| {
                format!("decode model tool arguments for {}", call.function.name)
            })?;
            tool_calls.push(ugoite_konase::ModelToolCall {
                id: call.id,
                name: call.function.name,
                arguments,
            });
        }
        Ok(ModelResult {
            request_id: request.request_id,
            text: message.content,
            tool_calls,
        })
    }
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

type McpClient = rmcp::service::RunningService<RoleClient, ClientInfo>;

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
    Capability {
        name: tool.name.into_owned(),
        description: tool
            .description
            .map_or_else(String::new, |value| value.into_owned()),
        input_schema: Some(input_schema),
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
    async fn connect(base_url: &str) -> Result<Self> {
        let target = crate::commands::auth::mcp_target(base_url).await?;
        let session = crate::commands::auth::active_session_for(base_url, Some(&target.resource))
            .await?
            .ok_or_else(|| {
                anyhow!(
                    "`ugoite konase` requires an MCP credential; run `ugoite auth login --for mcp`"
                )
            })?;
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(target.endpoint)
                .auth_header(session.access_token),
        );
        let client = ClientInfo::default()
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
                .context("undo Ugoite Work")?;
            let text = result
                .content
                .iter()
                .filter_map(|content| match content {
                    ContentBlock::Text(content) => Some(content.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            let success = result.is_error != Some(true);
            return Ok(McpResult {
                request_id,
                operation,
                success,
                observation: None,
                resources: vec![],
                resource_contents: vec![],
                error: (!success).then_some(text),
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

pub async fn run(cmd: KonaseCmd) -> Result<()> {
    let config = load_config();
    if config.mode == EndpointMode::Core {
        bail!("`ugoite konase` currently requires backend or api mode with an MCP credential");
    }
    let base_url =
        validated_base_url(&config)?.ok_or_else(|| anyhow!("remote endpoint is missing"))?;
    let mut mcp = RmcpMcpHost::connect(&base_url).await?;
    let mut model = OpenAiModelHost::from_env()?;
    let capabilities = mcp.capabilities().await;
    match cmd.prompt {
        Some(prompt) => {
            if prompt.trim().is_empty() {
                bail!("Konase prompt must not be empty");
            }
            let turn = run_turn(&mut model, &mut mcp, &prompt, &capabilities).await?;
            println!("{}", turn.outcome.summary);
        }
        None => {
            let interactive = io::stdin().is_terminal();
            if interactive {
                println!("Konase");
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
                    let Some(work_id) = last_work_id.take() else {
                        println!("取り消せる Work はありません。");
                        continue;
                    };
                    let result = mcp
                        .call_mcp(
                            McpRequest {
                                request_id: format!("undo-{}", Uuid::now_v7()),
                                server: "ugoite".into(),
                                operation: "ugoite.undo".into(),
                                arguments: Default::default(),
                            },
                            &work_id,
                        )
                        .await?;
                    if !result.success {
                        bail!(result.error.unwrap_or_else(|| "Undo failed".into()));
                    }
                    println!("✓ 取り消しました");
                    continue;
                }
                let turn = run_turn(&mut model, &mut mcp, prompt, &capabilities).await?;
                println!("{}", turn.outcome.summary);
                last_work_id = turn.undo_available.then_some(turn.work_id);
                if last_work_id.is_some() {
                    println!("[u] 取り消す");
                }
            }
        }
    }
    Ok(())
}

struct TurnOutcome {
    outcome: JobOutcome,
    work_id: String,
    undo_available: bool,
}

async fn run_turn<M: ModelHost, C: McpHost>(
    model: &mut M,
    mcp: &mut C,
    prompt: &str,
    capabilities: &[Capability],
) -> Result<TurnOutcome> {
    let work_id = format!("work-{}", Uuid::now_v7());
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
    loop {
        action = match action {
            AgentAction::CallModel(request) => {
                let result = model.call_model(request).await?;
                runtime.resume(AgentRuntimeInput::ModelCompleted(result))?
            }
            AgentAction::CallMcp(request) => {
                println!("{}", request.operation);
                undo_available |= request.operation == "ugoite.save";
                let result = mcp.call_mcp(request, &work_id).await?;
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
                return Ok(TurnOutcome {
                    outcome,
                    work_id,
                    undo_available,
                });
            }
            AgentAction::AskConfirmation(_) => {
                bail!("confirmation is outside the CLI MVP")
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
            return Ok(TurnOutcome {
                outcome,
                work_id,
                undo_available,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct ScriptedModel {
        responses: Vec<ModelResult>,
    }

    #[async_trait]
    impl ModelHost for ScriptedModel {
        async fn call_model(&mut self, request: ModelRequest) -> Result<ModelResult> {
            let mut response = self.responses.remove(0);
            response.request_id = request.request_id;
            Ok(response)
        }
    }

    struct ScriptedMcp {
        operations: Vec<String>,
        work_ids: Vec<String>,
    }

    #[test]
    fn write_tool_requests_keep_model_arguments_and_bind_the_work_id() {
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
            };
            let (name, params) = write_tool_request(request, "work-1").unwrap();
            assert_eq!(name, operation);
            assert_eq!(params.name, operation);
            assert_eq!(
                params.arguments.unwrap(),
                arguments.as_object().cloned().unwrap()
            );
            assert_eq!(
                params.meta.unwrap().0 .0.get("ugoite/runId"),
                Some(&Value::String("work-1".into()))
            );
        }
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
        let capability = capability_from_tool(Tool::new("ugoite.search", "Search entries", schema));

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
    }

    #[async_trait]
    impl McpHost for ScriptedMcp {
        async fn call_mcp(&mut self, request: McpRequest, work_id: &str) -> Result<McpResult> {
            self.operations.push(request.operation.clone());
            self.work_ids.push(work_id.to_owned());
            let is_search = request.operation == "ugoite.search";
            Ok(McpResult {
                request_id: request.request_id,
                operation: request.operation,
                success: true,
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
                error: None,
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
                },
                Capability {
                    name: "resources/read".into(),
                    description: "read an entry".into(),
                    input_schema: Some(resources_read_schema()),
                },
            ]
        }
    }

    #[tokio::test]
    async fn scripted_search_read_then_answer_completes_the_konase_loop() {
        let mut model = ScriptedModel {
            responses: vec![
                ModelResult {
                    request_id: String::new(),
                    text: None,
                    tool_calls: vec![ugoite_konase::ModelToolCall {
                        id: "call-search".into(),
                        name: "ugoite.search".into(),
                        arguments: json!({"query": "WebAssembly"}),
                    }],
                },
                ModelResult {
                    request_id: String::new(),
                    text: Some("1件見つかりました。".into()),
                    tool_calls: vec![ugoite_konase::ModelToolCall {
                        id: "call-read".into(),
                        name: "resources/read".into(),
                        arguments: json!({"uri": "ugoite://entry/1"}),
                    }],
                },
                ModelResult {
                    request_id: String::new(),
                    text: Some("WebAssemblyのメモを確認しました。".into()),
                    tool_calls: vec![],
                },
            ],
        };
        let mut mcp = ScriptedMcp {
            operations: vec![],
            work_ids: vec![],
        };
        let capabilities = mcp.capabilities().await;
        let outcome = run_turn(
            &mut model,
            &mut mcp,
            "WebAssemblyのメモを探して",
            &capabilities,
        )
        .await
        .unwrap();
        assert_eq!(outcome.outcome.summary, "WebAssemblyのメモを確認しました。");
        assert_eq!(mcp.operations, ["ugoite.search", "resources/read"]);
        assert_eq!(mcp.work_ids.len(), 2);
        assert_eq!(mcp.work_ids[0], mcp.work_ids[1]);
    }
}
