//! Plugin Services
//!
//! Services for plugin functionality including external request proxying
//! and provisioning.

pub mod install;
pub mod lifecycle;
pub mod local_key;
pub mod manifest_validate;
pub mod provisioning;
pub mod proxy;
pub mod registry;
pub mod signing;
pub mod svg_validate;
pub mod trust;
pub mod types;
pub mod validation;

pub use provisioning::provision_plugins;
pub use proxy::{PluginProxyError, PluginProxyService};
