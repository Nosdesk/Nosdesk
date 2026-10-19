//! What moves between workspaces: the table and column policy ([`policy`])
//! and the row shaping that applies it ([`rows`]). Instance backup and
//! restore, the workspace export and the workspace import all read it.

pub mod policy;
pub mod rows;
