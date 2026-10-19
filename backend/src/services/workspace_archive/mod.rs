//! What moves between workspaces: the table and column policy ([`policy`]),
//! the row shaping that applies it ([`rows`]), the import plan ([`plan`])
//! that decides every row and person before a load, the references no
//! foreign key carries ([`refs`]), and its application to an existing
//! workspace ([`apply`]). Instance backup and restore, the workspace export
//! and the workspace import all read it.

pub mod apply;
pub mod plan;
pub mod policy;
pub mod refs;
pub mod rows;
