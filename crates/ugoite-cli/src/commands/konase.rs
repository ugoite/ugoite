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
    async fn call_mcp(&mut self, request: McpRequest) -> Result<McpResult>;
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

impl RmcpMcpHost {
    async fn connect(base_url: &str) -> Result<Self> {
        let session = crate::commands::auth::active_session(base_url)
            .await?
            .ok_or_else(|| anyhow!("`ugoite konase` requires `ugoite auth login`"))?;
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(format!(
                "{}/mcp",
                base_url.trim_end_matches('/')
            ))
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
            .map(|tool| Capability {
                name: tool.name.into_owned(),
                description: tool
                    .description
                    .map_or_else(String::new, |value| value.into_owned()),
            })
            .filter(|capability| capability.name == "ugoite.search")
            .collect::<Vec<_>>();
        capabilities.push(Capability {
            name: "resources/read".into(),
            description: "Read the full content of an opaque Ugoite resource URI".into(),
        });
        Ok(Self {
            client,
            capabilities,
        })
    }
}

#[async_trait]
impl McpHost for RmcpMcpHost {
    async fn call_mcp(&mut self, request: McpRequest) -> Result<McpResult> {
        if request.operation == "resources/read" {
            let uri = request
                .arguments
                .get("uri")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("resources/read requires a string uri"))?;
            let result = self
                .client
                .read_resource(ReadResourceRequestParams::new(uri))
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
        if request.operation != "ugoite.search" {
            bail!("unsupported read-only MCP operation {}", request.operation);
        }
        let result = self
            .client
            .call_tool(
                CallToolRequestParams::new("ugoite.search")
                    .with_arguments(request.arguments.into_iter().collect()),
            )
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
            let outcome = run_turn(&mut model, &mut mcp, &prompt, &capabilities).await?;
            println!("{}", outcome.summary);
        }
        None => {
            let interactive = io::stdin().is_terminal();
            if interactive {
                println!("Konase");
            }
            let mut input = String::new();
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
                let outcome = run_turn(&mut model, &mut mcp, prompt, &capabilities).await?;
                println!("{}", outcome.summary);
            }
        }
    }
    Ok(())
}

async fn run_turn<M: ModelHost, C: McpHost>(
    model: &mut M,
    mcp: &mut C,
    prompt: &str,
    capabilities: &[Capability],
) -> Result<JobOutcome> {
    let work_id = format!("work-{}", Uuid::now_v7());
    let job_id = format!("job-{}", Uuid::now_v7());
    let mut state = step(
        Default::default(),
        ugoite_konase::KonaseEvent::UserSubmitted(UserRequest {
            work_id: work_id.clone(),
            job_id: job_id.clone(),
            goal: prompt.into(),
            available_capabilities: capabilities.to_vec(),
            safety_hints: vec!["This read-only MVP must not save or delete entries".into()],
            expected_response_schema: None,
        }),
    )
    .state;
    let start = state
        .pending_effect
        .as_ref()
        .ok_or_else(|| anyhow!("Konase did not start a Job"))?;
    if !matches!(start, ugoite_konase::PendingEffect::StartJob) {
        bail!("Konase returned an unexpected initial effect");
    }
    let mut runtime = RigAgentRuntime::default();
    let first_action = runtime.start(
        ugoite_konase::JobSpec {
            id: job_id.clone(),
            work_id: work_id.clone(),
            goal: prompt.into(),
            expected_response_schema: None,
        },
        ugoite_konase::ContextCapsule {
            work_goal: prompt.into(),
            job_goal: prompt.into(),
            current_strategy_summary: None,
            relevant_observations: vec![],
            available_capabilities: capabilities.to_vec(),
            selected_resource_contents: vec![],
            safety_hints: vec![],
            expected_response_schema: None,
        },
    )?;
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
    loop {
        action = match action {
            AgentAction::CallModel(request) => {
                let result = model.call_model(request).await?;
                runtime.resume(AgentRuntimeInput::ModelCompleted(result))?
            }
            AgentAction::CallMcp(request) => {
                println!("{}", request.operation);
                let result = mcp.call_mcp(request).await?;
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
                return Ok(outcome);
            }
            AgentAction::AskConfirmation(_) => {
                bail!("confirmation is outside the read-only CLI MVP")
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
            return Ok(outcome);
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
    }

    #[async_trait]
    impl McpHost for ScriptedMcp {
        async fn call_mcp(&mut self, request: McpRequest) -> Result<McpResult> {
            self.operations.push(request.operation.clone());
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
                },
                Capability {
                    name: "resources/read".into(),
                    description: "read an entry".into(),
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
        let mut mcp = ScriptedMcp { operations: vec![] };
        let capabilities = mcp.capabilities().await;
        let outcome = run_turn(
            &mut model,
            &mut mcp,
            "WebAssemblyのメモを探して",
            &capabilities,
        )
        .await
        .unwrap();
        assert_eq!(outcome.summary, "WebAssemblyのメモを確認しました。");
        assert_eq!(mcp.operations, ["ugoite.search", "resources/read"]);
    }
}
