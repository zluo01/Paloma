use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub title: String,
    pub icon: Option<IconRef>,
    pub actions: Vec<Action>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Action {
    pub label: String,
    pub params: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActionOutcome {
    Hide,
    Stay,
    Replace { input: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum IconRef {
    Name(String),
    Path(String),
    Embedded { format: ImageFormat, data: Vec<u8> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageFormat {
    Png,
    Jpeg,
    Svg,
    Webp,
    Gif,
}

pub trait Capability: Send + Sync + 'static {
    fn id(&self) -> &'static str;
    fn metadata(&self) -> CapabilityMeta;
}

pub trait QueryHandler: Capability {
    fn query(&self, input: &str) -> Vec<Item>;

    fn run(&self, action: Action) -> ActionOutcome;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityMeta {
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<IconRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

pub trait Tool: Capability {
    type Args: serde::de::DeserializeOwned + schemars::JsonSchema + Send;

    const NAME: &'static str;
    const DESCRIPTION: &'static str;

    fn invoke(&self, args: Self::Args) -> impl Future<Output = Result<ToolResult, String>> + Send;

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: Self::NAME.into(),
            description: Self::DESCRIPTION.into(),
            input_schema: serde_json::to_value(schemars::schema_for!(Self::Args))
                .expect("JsonSchema output is always serializable"),
        }
    }
}

pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

pub struct ToolResult {
    pub content: String,
}

pub trait DynTool: Send + Sync {
    fn schema(&self) -> ToolSchema;
    fn invoke<'a>(
        &'a self,
        args: serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult, String>> + Send + 'a>>;
}

impl<T> DynTool for T
where
    T: Tool + Send + Sync,
{
    fn schema(&self) -> ToolSchema {
        Tool::schema(self)
    }

    fn invoke<'a>(
        &'a self,
        args: serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult, String>> + Send + 'a>> {
        Box::pin(async move {
            let parsed: T::Args = serde_json::from_value(args).map_err(|e| e.to_string())?;
            Tool::invoke(self, parsed).await
        })
    }
}
