//! # aria-core
//!
//! Foundational types for the HiveClaw architecture.
//! This crate is `#![no_std]` compatible with the `alloc` crate.
//!
//! ## Types
//! - [`AgentRequest`] — Inbound user request normalized across all channels
//! - [`AgentResponse`] — Outbound agent response with skill trace
//! - [`ToolDefinition`] — Tool metadata including JSON schema and embedding vector
//! - [`HardwareIntent`] — Low-level motor/actuator command for HAL layer

#![no_std]

extern crate alloc;

use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use serde::{Deserialize, Serialize};

mod agent;
mod app;
mod browser;
mod computer;
mod execution_backend;
mod errors;
mod legacy;
mod model;
mod robotics;
mod runtime;
#[cfg(test)]
mod tests;

pub use agent::*;
pub use app::*;
pub use browser::*;
pub use computer::*;
pub use execution_backend::*;
pub use errors::*;
pub use legacy::*;
pub use model::*;
pub use robotics::*;
pub use runtime::*;
