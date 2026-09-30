//! Learn the External Driver API one capability at a time.
//!
//! Start with [`registry`] for a control's definition, then [`application`] for
//! its read/write request and [`domain::Device`] for the vendor operation to supply.
//! [`device`] implements that seam for a separate simulator: replace it for your
//! device, rather than copying the simulator's private protocol into a real driver.
//! [`protocol`] and [`session`] demonstrate wire responses and delivery rules.
//! The module layout is an example design, not a requirement of SPEC-23.
pub mod application;
pub mod config;
pub mod device;
pub mod diagnostics;
pub mod domain;
pub mod identity;
pub mod protocol;
pub mod registry;
pub mod session;
pub mod simulator;
pub mod tls;
