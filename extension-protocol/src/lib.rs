pub use prost::{DecodeError, Message, bytes::Bytes};

pub const PROTOCOL_VERSION: u64 = 1;

pub mod v1 {
    include!(concat!(env!("OUT_DIR"), "/scry.extension.v1.rs"));
}
