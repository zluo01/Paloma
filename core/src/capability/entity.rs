use scry_provider_protocol::v1::ToolDefinition;
use serde_json::Value;
use uuid::Uuid;

use crate::entity::{HealthStatus, Icon};

pub trait Capability: Send + Sync + 'static {
    /// handler unique name
    fn id(&self) -> &'static str;
    /// handler descriptions for UI display
    fn metadata(&self) -> CapabilityMeta;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityMeta {
    pub name: String,
    pub version: String,
    pub description: String,
    pub icon: Option<Icon>,
    pub homepage: Option<String>,
    pub author: Option<String>,
}

#[async_trait::async_trait]
pub trait Tool: Capability {
    type Args: serde::de::DeserializeOwned + schemars::JsonSchema + Send;

    const NAME: &'static str;
    const DESCRIPTION: &'static str;

    async fn invoke(
        &self,
        session_id: Uuid,
        call_id: String,
        args: Self::Args,
    ) -> Result<ToolResult, String>;

    async fn cancel_session(&self, session_id: Uuid);

    fn specs(&self) -> Vec<ToolSpec> {
        vec![ToolSpec {
            name: Self::NAME.into(),
            tool: None,
            schema: ToolSchema {
                name: Self::NAME.into(),
                description: Self::DESCRIPTION.into(),
                parameters: serde_json::to_value(schemars::schema_for!(Self::Args))
                    .expect("JsonSchema output is always serializable"),
            },
        }]
    }
}

#[derive(Clone, Debug)]
pub struct ToolSpec {
    pub name: String,
    pub tool: Option<String>,
    pub schema: ToolSchema,
}

#[derive(Clone, Debug)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

impl ToolSchema {
    pub(crate) fn to_definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name.clone(),
            description: self.description.clone(),
            parameters: self.parameters.to_string(),
        }
    }
}

#[derive(Debug)]
pub enum ToolResult {
    Text(String),
    #[allow(dead_code)]
    Binary {
        mime_type: String,
        data: Vec<u8>,
    },
}

#[async_trait::async_trait]
pub trait DynTool: Send + Sync {
    async fn specs(&self) -> Result<Vec<ToolSpec>, String>;

    fn health_status(&self) -> HealthStatus;

    fn description(&self) -> &str {
        ""
    }

    fn error(&self) -> Option<&str> {
        None
    }

    async fn invoke(
        &self,
        name: Option<String>,
        session_id: Uuid,
        call_id: String,
        args: Value,
    ) -> Result<ToolResult, String>;

    async fn cancel_session(&self, session_id: Uuid);
}

#[async_trait::async_trait]
impl<T> DynTool for T
where
    T: Tool + Send + Sync,
{
    async fn specs(&self) -> Result<Vec<ToolSpec>, String> {
        Ok(Tool::specs(self))
    }

    /// for all local tool, default is running
    /// remote or extension should override it
    fn health_status(&self) -> HealthStatus {
        HealthStatus::Running
    }

    async fn invoke(
        &self,
        _name: Option<String>,
        session_id: Uuid,
        call_id: String,
        args: Value,
    ) -> Result<ToolResult, String> {
        let parsed: T::Args = serde_json::from_value(args).map_err(|e| e.to_string())?;
        Tool::invoke(self, session_id, call_id, parsed).await
    }

    async fn cancel_session(&self, session_id: Uuid) {
        Tool::cancel_session(self, session_id).await
    }
}
