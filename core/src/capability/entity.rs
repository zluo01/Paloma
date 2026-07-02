use serde_json::Value;
use uuid::Uuid;

use crate::entity::HealthStatus;

#[derive(Clone, Debug)]
pub struct Item {
    pub title: String,
    pub subtitle: Option<String>,
    pub icon: Option<IconRef>,
    pub actions: Vec<Action>,
}

#[derive(Clone, Debug)]
pub struct Action {
    /// action name for both UI display and as action enum
    pub label: String,
    /// action input params
    pub params: Vec<String>,
    /// whether the action is the default action.
    pub primary: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionOutcome {
    Hide,
    Stay,
    Replace { input: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IconRef {
    Name(String),
    Path(String),
    Embedded { format: ImageFormat, data: Vec<u8> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Svg,
    Webp,
    Gif,
}

pub trait Capability: Send + Sync + 'static {
    /// handler unique name
    fn id(&self) -> &'static str;
    /// handler descriptions for UI display
    fn metadata(&self) -> CapabilityMeta;
}

pub trait QueryHandler: Capability {
    fn query(&self, input: &str) -> Vec<Item>;

    fn run(&self, action: Action) -> ActionOutcome;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityMeta {
    pub name: String,
    pub version: String,
    pub description: String,
    pub icon: Option<IconRef>,
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

    fn health_statue(&self) -> HealthStatus;

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
    fn health_statue(&self) -> HealthStatus {
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
}

pub struct Placeholder;

#[async_trait::async_trait]
impl DynTool for Placeholder {
    async fn specs(&self) -> Result<Vec<ToolSpec>, String> {
        Ok(Vec::new())
    }

    fn health_statue(&self) -> HealthStatus {
        HealthStatus::Starting
    }

    async fn invoke(
        &self,
        _name: Option<String>,
        _session_id: Uuid,
        _call_id: String,
        _args: Value,
    ) -> Result<ToolResult, String> {
        Err("tool is still starting".to_string())
    }
}
