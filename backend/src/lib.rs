// Crate-wide clippy allows for stylistic lints we accept across the
// codebase. These are not bug-class lints, they're shape/size concerns
// that don't justify a refactor:
//
// - too_many_arguments: handlers and helpers thread DB pool, actor,
//   request context, and route params; >7 args is normal.
// - large_enum_variant: HttpResponse-producing match arms naturally
//   build differently-sized Responder values; boxing every variant
//   would bloat callers for no runtime benefit.
// - type_complexity: Result<Option<Vec<(T, U)>>, E> shapes show up
//   in repository signatures; aliases would obscure the layout.
// - should_implement_trait: a handful of types expose inherent
//   `from_str` methods that we don't want to lock into the
//   FromStr trait's `Err = ParseIntError`-style ergonomics.
// - doc_lazy_continuation / doc_overindented_list_items:
//   stylistic rustdoc formatting checks; many existing `//!`
//   blocks use prose layouts that don't fit the lint's expected
//   list indentation. Not worth churning every doc block for.
// - field_reassign_with_default: `let mut x = T::default(); x.f = ...`
//   is more readable than the struct-update form when the field
//   set is small and the type's defaults are uninteresting.
// - manual_strip: a few hot paths do `&s[2..]` after a starts_with
//   check; rewriting as strip_prefix gains nothing and makes the
//   downstream `.find()` chain harder to read.
#![allow(
    clippy::too_many_arguments,
    clippy::large_enum_variant,
    clippy::type_complexity,
    clippy::should_implement_trait,
    clippy::doc_lazy_continuation,
    clippy::doc_overindented_list_items,
    clippy::field_reassign_with_default,
    clippy::manual_strip
)]

pub mod config;
pub mod config_utils;
pub mod db;
pub mod extractors;
pub mod handlers;
pub mod license;
pub mod middleware;
pub mod models;
pub mod oidc;
pub mod repository;
pub mod schema;
pub mod services;
pub mod sync;
pub mod telemetry;
pub mod utils;
pub mod workers;

#[cfg(test)]
pub mod test_helpers;

#[cfg(test)]
mod route_registration_tests;
