//! What moves between workspaces: the table and column policy ([`policy`]),
//! the row shaping that applies it ([`rows`]), and the import plan ([`plan`])
//! that decides every row and person before a load. Instance backup and
//! restore, the workspace export and the workspace import all read it.

pub mod plan;
pub mod policy;
pub mod rows;
