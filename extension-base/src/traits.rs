use scry_extension_protocol::v1::{
    Action, Capability as CapabilityMeta, Item, run_action_response::Behavior,
};

pub trait Capability: Send + Sync {
    fn metadata(&self) -> CapabilityMeta;
}

pub trait QueryHandler: Capability {
    fn search(&self, input: &str) -> Vec<Item>;

    fn run_search_action(&self, action: Action) -> Behavior;
}
