//! Learn the External Driver API one capability at a time.
//!
//! Start with [`registry`] for a control's definition, then [`application`] for
//! its read/write request and [`domain::Device`] for the vendor operation to supply.
//! [`demo::DemoDevice`] supplies a small in-memory teaching implementation. Replace
//! its methods with vendor operations; no second service or protocol is required.
//! [`protocol`] and [`session`] demonstrate wire responses and delivery rules.
//! The module layout is an example design, not a requirement of SPEC-23.
pub mod application;
pub mod config;
pub mod demo;
pub mod diagnostics;
pub mod domain;
pub mod identity;
pub mod protocol;
pub mod registry;
mod sdp;
pub mod session;
pub mod tls;

// Transport/fault-injection equipment is optional, not part of the driver lesson.
#[cfg(feature = "device-lab")]
#[path = "../tools/device_lab/mod.rs"]
pub mod lab;
