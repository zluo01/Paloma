use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use log::error;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    capability::{DynTool, Shell, Tool, ToolResult, ToolSchema, ToolSpec},
    controller::remote::{McpController, PermissionWorkflowManagerClient},
    db::Storage,
    permission::PermissionState,
};

pub struct ToolController {
    shell: Arc<dyn DynTool>,
    specs: HashMap<String, ToolSpec>,
    storage: Storage,
    mcp_controller: Arc<McpController>,
    permission_workflow_client: PermissionWorkflowManagerClient,
}

#[derive(Debug)]
pub struct ToolCallPayload {
    pub call_id: String,
    pub name: String,
    pub arguments: String,
}

impl ToolController {
    pub async fn new(
        storage: Storage,
        mcp_controller: Arc<McpController>,
        permission_workflow_client: PermissionWorkflowManagerClient,
    ) -> Arc<Self> {
        let shell: Arc<dyn DynTool> = Arc::new(Shell::new());
        let specs = shell
            .specs()
            .await
            .unwrap()
            .into_iter()
            .map(|spec| (spec.schema.name.clone(), spec))
            .collect();

        Arc::new(Self {
            shell,
            specs,
            storage,
            mcp_controller,
            permission_workflow_client,
        })
    }

    pub async fn tool_schemas(&self) -> Vec<ToolSchema> {
        let disabled = self.storage.disabled_plugins().await.unwrap_or_else(|e| {
            error!("fail to get disabled plugins. {}", e);
            HashSet::new()
        });

        let mut schemas: Vec<ToolSchema> = self
            .specs
            .values()
            .map(|spec| spec.schema.clone())
            .collect();
        schemas.extend(self.mcp_controller.schemas(&disabled).await);
        schemas.sort_by(|a, b| a.name.cmp(&b.name));
        schemas
    }

    pub async fn retrieve_toolspec(&self, function_call_name: &str) -> Option<ToolSpec> {
        match self.specs.get(function_call_name) {
            Some(spec) => Some(spec.clone()),
            None => self.mcp_controller.spec(function_call_name).await,
        }
    }

    /// We should populate all errors back to the model such that model has context on what happens and what to do next
    /// Also log error so can debug internally
    pub async fn exec(&self, session_id: Uuid, call: &ToolCallPayload) -> String {
        let args: Value = match serde_json::from_str(&call.arguments) {
            Ok(args) => args,
            Err(err) => return format!("invalid arguments for {}: {err}", call.name),
        };

        let call_id = &call.call_id;

        // Shell and MCP commands must clear the permission workflow before they run.
        if let Err(msg) = self.authorize(call_id.clone()).await {
            error!("error happens when waiting permission for {call_id}: {msg}");
            return msg;
        }

        let outcome = if call.name == Shell::NAME {
            self.shell
                .invoke(None, session_id, call_id.clone(), args)
                .await
        } else {
            self.mcp_controller
                .call(call.name.clone(), session_id, call_id.clone(), args)
                .await
                .map_err(|err| err.to_string())
        };

        match outcome {
            Ok(ToolResult::Text(text)) => text,
            Ok(ToolResult::Binary { mime_type, .. }) => format!("<binary output: {mime_type}>"),
            Err(message) => {
                error!("tool {} failed: {message}", call.name);
                message
            },
        }
    }

    /// Wait and Get the user decision on permission
    async fn authorize(&self, call_id: String) -> Result<(), String> {
        let decision = self
            .permission_workflow_client
            .wait_decision(call_id)
            .await
            .map_err(|err| format!("permission workflow unavailable: {err}"))?;

        match decision.await {
            Some(PermissionState::Allow) => Ok(()),
            Some(PermissionState::Deny) => Err("command was denied by the user".into()),
            Some(PermissionState::Error) => {
                Err("the command could not be validated for permission (it may be empty or malformed); it was not executed".into())
            },
            None => Err("permission request was cancelled; command was not executed".into()),
        }
    }

    pub async fn cancel_session(&self, session_id: Uuid) {
        self.mcp_controller.cancel_session(session_id);
        self.shell.cancel_session(session_id).await;
    }
}
