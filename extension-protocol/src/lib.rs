pub use prost::{DecodeError, Message, bytes::Bytes};

pub const PROTOCOL_VERSION: u64 = 1;

pub mod v1 {
    include!(concat!(env!("OUT_DIR"), "/scry.extension.v1.rs"));

    impl CapabilityIcon {
        pub fn name(name: impl Into<String>) -> Self {
            Self {
                icon: Some(capability_icon::Icon::Name(name.into())),
            }
        }

        pub fn path(path: impl Into<String>) -> Self {
            Self {
                icon: Some(capability_icon::Icon::Path(path.into())),
            }
        }

        pub fn embedded(data: Vec<u8>) -> Self {
            Self {
                icon: Some(capability_icon::Icon::Embedded(data)),
            }
        }
    }

    impl ToolContent {
        pub fn new(tag: impl Into<String>) -> Self {
            Self {
                tag: tag.into(),
                ..Default::default()
            }
        }

        pub fn text(text: impl Into<String>) -> Self {
            Self {
                body: Some(tool_content::Body::Text(text.into())),
                ..Default::default()
            }
        }

        pub fn binary(mime_type: impl Into<String>, data: Vec<u8>) -> Self {
            Self {
                body: Some(tool_content::Body::Binary(Binary {
                    mime_type: mime_type.into(),
                    data,
                })),
                ..Default::default()
            }
        }

        pub fn attr(mut self, key: impl Into<String>, value: impl ToString) -> Self {
            self.attributes.push(Attribute {
                key: key.into(),
                value: value.to_string(),
            });
            self
        }

        pub fn attr_if(
            self,
            condition: bool,
            key: impl Into<String>,
            value: impl ToString,
        ) -> Self {
            if condition {
                self.attr(key, value)
            } else {
                self
            }
        }

        pub fn child(mut self, child: ToolContent) -> Self {
            match &mut self.body {
                Some(tool_content::Body::Children(children)) => children.nodes.push(child),
                _ => {
                    self.body = Some(tool_content::Body::Children(Children {
                        nodes: vec![child],
                    }));
                },
            }
            self
        }

        pub fn cdata(mut self, text: impl Into<String>) -> Self {
            self.body = Some(tool_content::Body::Text(text.into()));
            self
        }

        pub fn children(&self) -> &[ToolContent] {
            match &self.body {
                Some(tool_content::Body::Children(children)) => &children.nodes,
                _ => &[],
            }
        }
    }
}
