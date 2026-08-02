pub use tonic;

pub mod v1 {
    include!(concat!(env!("OUT_DIR"), "/paloma.binding.v1.rs"));
}
