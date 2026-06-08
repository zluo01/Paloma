use std::{
    collections::{BTreeMap, HashSet},
    sync::{Arc, RwLock},
};

use dashmap::DashMap;
use log::error;
use scry_capability::{
    DynTool, HealthStatus, McpTool, ProcessManagerClient, Shell, Tool, ToolResult, ToolSchema,
    ToolSpec,
};
use scry_storage::Storage;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::remote::{
    permission_workflow_manager::PermissionState, PermissionWorkflowManagerClient,
};

pub struct ToolController {
    handlers: DashMap<String, Arc<dyn DynTool>>,
    tool_specs: RwLock<Arc<BTreeMap<String, ToolSpec>>>,
    storage: Storage,
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
    pub async fn new(
        storage: Storage,
        process_manager_client: ProcessManagerClient,
        permission_workflow_client: PermissionWorkflowManagerClient,
    ) -> Self {
        let handlers: DashMap<String, Arc<dyn DynTool>> = DashMap::new();
        let mut tool_specs: BTreeMap<String, ToolSpec> = BTreeMap::new();

        let shell: Arc<dyn DynTool> = Arc::new(Shell::new(process_manager_client));
        for spec in shell.specs().await.unwrap() {
            tool_specs.insert(spec.schema.name.clone(), spec);
        }
        handlers.insert(Shell::NAME.to_string(), shell);

        // Register configured MCP servers. `McpTool::new` never hard-fails — a
        // server that can't connect registers itself as `Unhealthy` (filtered
        // out at schema time) — so a bad server can't block startup.
        let plugins = storage.all_mcp_plugins().await.unwrap_or_else(|e| {
            error!("failed to load mcp plugins: {e}");
            Vec::new()
        });
        for plugin in plugins {
            let name = plugin.name.clone();
            let (tool, specs) = McpTool::new(&name, plugin).await;
            for spec in specs {
                tool_specs.insert(spec.schema.name.clone(), spec);
            }
            handlers.insert(name, Arc::new(tool));
        }

        Self {
            handlers,
            tool_specs: RwLock::new(Arc::new(tool_specs)),
            storage,
            permission_workflow_client,
        }
    }

    pub async fn tool_schemas(&self) -> Vec<ToolSchema> {
        let disabled = self.storage.disabled_plugins().await.unwrap_or_else(|e| {
            error!("fail to get disabled plugins. {}", e);
            HashSet::new()
        });

        let running: HashSet<String> = self
            .handlers
            .iter()
            .filter(|e| e.health_statue() == HealthStatus::Running)
            .map(|e| e.key().clone())
            .collect();

        let tools = self.tool_specs.read().unwrap().clone();
        tools
            .values()
            .filter(|spec| !disabled.contains(&spec.name))
            .filter(|spec| running.contains(&spec.name))
            .map(|spec| spec.schema.clone())
            .collect()
    }

    pub fn retrieve_toolspec(&self, function_call_name: &str) -> Option<ToolSpec> {
        let tools = self.tool_specs.read().unwrap().clone();
        tools.get(function_call_name).cloned()
    }

    /// We should populate all errors back to the model such that model has context on what happens and what to do next
    /// Also log error so can debug internally
    pub async fn exec(&self, session_id: Uuid, call: &ToolCallPayload) -> String {
        let spec = match self.retrieve_toolspec(&call.name) {
            Some(spec) => spec,
            None => {
                let msg = format!(
                    "fail to find tool spec from function call name {}",
                    &call.name
                );
                error!("{}", msg);
                return msg;
            },
        };

        let Some(tool) = self.handlers.get(&spec.name).map(|t| t.value().clone()) else {
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

        match tool
            .invoke(spec.tool, session_id, call_id.clone(), args)
            .await
        {
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
