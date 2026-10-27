//! What moves between workspaces: the table and column policy ([`policy`]),
//! the row shaping that applies it ([`rows`]), the import plan ([`plan`])
//! that decides every row and person before a load, the references no
//! foreign key carries ([`refs`]), its application to an existing
//! workspace ([`apply`]), and where the archive's files land ([`files`]).
//! Instance backup and restore, the workspace export and the workspace
//! import all read it.

pub mod apply;
pub mod files;
pub mod plan;
pub mod policy;
pub mod refs;
pub mod rows;
