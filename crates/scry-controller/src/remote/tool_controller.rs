use std::sync::Arc;

use dashmap::DashMap;
use log::error;
use scry_capability::{
    tools::shell::{process_manager::ProcessManagerClient, Shell},
    DynTool, Tool, ToolResult,
};
use scry_provider::ToolSchema as ProviderToolSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::remote::{
    permission_workflow_manager::PermissionState, PermissionWorkflowManagerClient,
};

pub struct ToolController {
    handlers: DashMap<&'static str, Arc<dyn DynTool>>,
    permission_workflow_client: PermissionWorkflowManagerClient,
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
    pub fn new(
        process_manager_client: ProcessManagerClient,
        permission_workflow_client: PermissionWorkflowManagerClient,
    ) -> Self {
        let handlers: DashMap<&'static str, Arc<dyn DynTool>> = DashMap::new();

        let shell: Arc<dyn DynTool> = Arc::new(Shell::new(process_manager_client));
        handlers.insert(Shell::NAME, shell);

        Self {
            handlers,
            permission_workflow_client,
        }
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

        let call_id = &call.call_id;

        // Shell commands must clear the permission workflow before they run.
        if call.name == Shell::NAME {
            if let Err(msg) = self.authorize_shell(call_id.clone()).await {
                error!("shell permission gate for {call_id}: {msg}");
                return msg;
            }
        }

        match tool.invoke(session_id, call_id.clone(), args).await {
            Ok(ToolResult::Text(text)) => text,
            Ok(ToolResult::Binary { mime_type, .. }) => format!("<binary output: {mime_type}>"),
            Err(message) => {
                error!("tool {} failed: {message}", call.name);
                message
            },
        }
    }

    /// Wait and Get the user decision on permission
    async fn authorize_shell(&self, call_id: String) -> Result<(), String> {
        let decision = self
            .permission_workflow_client
            .wait_decision(call_id)
            .await
            .map_err(|err| format!("permission workflow unavailable: {err}"))?;

        match decision.await {
            Some(PermissionState::Allow) => Ok(()),
            Some(PermissionState::Deny) => Err("command was denied by the user".into()),
            Some(PermissionState::Timeout) => {
                Err("permission request timed out; command was not executed".into())
            },
            None => Err("permission request was cancelled; command was not executed".into()),
        }
    }
}
