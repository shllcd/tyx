pub mod bit;
pub mod capn;
pub mod data;
pub mod flat;
pub mod formats;
pub mod meter;
pub mod model;
pub mod proto;
pub mod report;

#[allow(clippy::all, mismatched_lifetime_syntaxes, reason = "capnpc output")]
pub mod rows_capnp {
    include!(concat!(env!("OUT_DIR"), "/rows_capnp.rs"));
}
