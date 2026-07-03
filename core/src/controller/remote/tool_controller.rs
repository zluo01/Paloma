use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{Arc, RwLock},
};

use dashmap::DashMap;
use log::error;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    capability::{
        DynTool, McpTool, Placeholder, ProcessManagerClient, ProcessManagerError, Shell, Tool,
        ToolResult, ToolSchema, ToolSpec,
    },
    controller::remote::PermissionWorkflowManagerClient,
    db::{Storage, StorageError},
    entity::{HealthStatus, Plugin},
    permission::PermissionState,
};

pub struct ToolStatus {
    pub description: String,
    pub status: HealthStatus,
    pub error: Option<String>,
}

pub struct ToolController {
    handlers: DashMap<String, Arc<dyn DynTool>>,
    tool_specs: RwLock<Arc<BTreeMap<String, ToolSpec>>>,
    storage: Storage,
    request_client: reqwest::Client,
    process_manager_client: ProcessManagerClient,
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
        request_client: reqwest::Client,
        process_manager_client: ProcessManagerClient,
        permission_workflow_client: PermissionWorkflowManagerClient,
    ) -> Arc<Self> {
        let handlers: DashMap<String, Arc<dyn DynTool>> = DashMap::new();
        let mut tool_specs: BTreeMap<String, ToolSpec> = BTreeMap::new();

        let shell: Arc<dyn DynTool> = Arc::new(Shell::new(process_manager_client.clone()));
        for spec in shell.specs().await.unwrap() {
            tool_specs.insert(spec.schema.name.clone(), spec);
        }
        handlers.insert(Shell::NAME.to_string(), shell);

        let plugins = storage.all_mcp_plugins().await.unwrap_or_else(|e| {
            error!("failed to load mcp plugins: {e}");
            Vec::new()
        });

        let controller = Arc::new(Self {
            handlers,
            tool_specs: RwLock::new(Arc::new(tool_specs)),
            storage: storage.clone(),
            request_client: request_client.clone(),
            process_manager_client,
            permission_workflow_client,
        });

        // Connect configured MCP servers in the background so a slow or broken
        // server can neither fail nor delay startup.
        for plugin in plugins {
            let placeholder: Arc<dyn DynTool> = Arc::new(Placeholder);
            controller
                .handlers
                .insert(plugin.name.clone(), Arc::clone(&placeholder));

            let controller = Arc::clone(&controller);
            let client = request_client.clone();
            let storage = storage.clone();
            tokio::spawn(async move {
                let (tool, specs) = McpTool::new(&plugin, client.clone(), storage.clone()).await;
                let mut swapped = false;
                controller.handlers.alter(&plugin.name, |_, current| {
                    if Arc::ptr_eq(&current, &placeholder) {
                        swapped = true;
                        Arc::new(tool)
                    } else {
                        current
                    }
                });
                if swapped {
                    controller.register_specs(specs);
                }
            });
        }

        controller
    }

    /// insert a tool's specs and handler
    fn register(&self, name: String, tool: McpTool, specs: Vec<ToolSpec>) {
        self.register_specs(specs);
        self.handlers.insert(name, Arc::new(tool));
    }

    fn register_specs(&self, specs: Vec<ToolSpec>) {
        let mut current = self.tool_specs.write().unwrap();
        let mut tools = (**current).clone();
        for spec in specs {
            tools.insert(spec.schema.name.clone(), spec);
        }
        *current = Arc::new(tools);
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

    /// get all non-built-in tools status, keyed by tool name
    pub async fn get_tools_status(&self) -> HashMap<String, ToolStatus> {
        self.handlers
            .iter()
            .filter(|entry| entry.key() != Shell::NAME)
            .map(|entry| {
                (
                    entry.key().clone(),
                    ToolStatus {
                        description: entry.value().description().to_string(),
                        status: entry.value().health_statue(),
                        error: entry.value().error().map(str::to_string),
                    },
                )
            })
            .collect()
    }

    /// add new tool to the controller in runtime
    pub async fn register_tool(&self, config: &Plugin) -> Result<()> {
        let name = config.name.clone();
        let (tool, specs) =
            McpTool::new(config, self.request_client.clone(), self.storage.clone()).await;
        // fail to init
        if tool.health_statue() != HealthStatus::Running {
            return Err(ToolControllerError::FailToInitialize {
                reason: tool.error().map(str::to_string),
            });
        }

        self.register(name, tool, specs);
        Ok(())
    }

    /// remove the tool from the controller in runtime
    pub async fn deregister_tool(&self, name: &str) -> Result<()> {
        self.handlers.remove(name);

        {
            let mut specs = self.tool_specs.write().unwrap();
            let mut tools = (**specs).clone();
            tools.retain(|_, spec| spec.name != name);
            *specs = Arc::new(tools);
        }

        self.storage.delete_plugin(name).await?;
        Ok(())
    }

    /// update tool with new setting or simply reinit the tool
    pub async fn update_tool(&self, config: &Plugin) -> Result<()> {
        let name = config.name.clone();
        let (tool, specs) =
            McpTool::new(config, self.request_client.clone(), self.storage.clone()).await;
        // fail to init
        if tool.health_statue() != HealthStatus::Running {
            return Err(ToolControllerError::FailToInitialize {
                reason: tool.error().map(str::to_string),
            });
        }

        self.handlers.remove(&name);

        {
            let mut current = self.tool_specs.write().unwrap();
            let mut tools = (**current).clone();
            // remove the old specs
            tools.retain(|_, spec| spec.name != name);
            // add the new specs
            for spec in specs {
                tools.insert(spec.schema.name.clone(), spec);
            }
            *current = Arc::new(tools);
        }

        self.handlers.insert(name, Arc::new(tool));
        self.storage
            .update_plugin(
                &config.name,
                config.transport,
                config.timeout,
                &config.env,
                &config.args,
            )
            .await?;
        Ok(())
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

        // Shell or MCP commands must clear the permission workflow before they run.
        if (call.name == Shell::NAME || spec.tool.is_some())
            && let Err(msg) = self.authorize(call_id.clone()).await
        {
            error!("error happens when waiting permission for {call_id}: {msg}");
            return msg;
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
    async fn authorize(&self, call_id: String) -> std::result::Result<(), String> {
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

    pub async fn cancel_session(&self, session_id: Uuid) -> Result<()> {
        self.process_manager_client
            .cancel_session(session_id)
            .await?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ToolControllerError {
    #[error(transparent)]
    Storage(#[from] StorageError),

    #[error(transparent)]
    ProcessManager(#[from] ProcessManagerError),

    #[error("fail to initialize mcp plugin: {}", reason.as_deref().unwrap_or("unknown error"))]
    FailToInitialize { reason: Option<String> },
}

type Result<T> = std::result::Result<T, ToolControllerError>;
