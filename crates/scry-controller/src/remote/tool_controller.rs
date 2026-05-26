use dashmap::DashMap;
use log::error;
use scry_capability::tools::shell::process_manager::ProcessManagerClient;
use scry_capability::tools::shell::Shell;
use scry_capability::{DynTool, Tool, ToolResult};
use scry_provider::entity::ToolSchema as ProviderToolSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::sync::Arc;
use uuid::Uuid;

pub struct ToolController {
    handlers: DashMap<&'static str, Arc<dyn DynTool>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ToolCallPayload {
    pub call_id: String,
    pub name: String,
    pub arguments: String,
    #[serde(flatten)]
    pub rest: Map<String, Value>, // everything else: id, type, status, …
}

impl ToolController {
    pub fn new(process_manager_client: ProcessManagerClient) -> Self {
        let handlers: DashMap<&'static str, Arc<dyn DynTool>> = DashMap::new();

        let shell: Arc<dyn DynTool> = Arc::new(Shell::new(process_manager_client));
        handlers.insert(Shell::NAME, shell);

        Self { handlers }
    }

    pub fn tool_schemas(&self) -> Vec<ProviderToolSchema> {
        self.handlers
            .iter()
            .map(|entry| {
                let schema = entry.value().schema();
                ProviderToolSchema {
                    name: schema.name,
                    description: schema.description,
                    parameters: schema.input_schema,
                }
            })
            .collect()
    }

    /// We should populate all errors back to the model such that model has context on what happens and what to do next
    /// Also log error so can debug internally
    pub async fn exec(&self, session_id: Uuid, call: &ToolCallPayload) -> String {
        let Some(tool) = self
            .handlers
            .get(call.name.as_str())
            .map(|t| t.value().clone())
        else {
            let msg = format!("unknown tool: {}", call.name);
            error!("{msg}");
            return msg;
        };

        let args: Value = match serde_json::from_str(&call.arguments) {
            Ok(args) => args,
            Err(err) => return format!("invalid arguments for {}: {err}", call.name),
        };

        let caller_id = &call.call_id;
        match tool.invoke(session_id, caller_id.clone(), args).await {
            Ok(ToolResult::Text(text)) => text,
            Ok(ToolResult::Binary { mime_type, .. }) => format!("<binary output: {mime_type}>"),
            Err(message) => {
                error!("tool {} failed: {message}", call.name);
                message
            }
        }
    }
}
