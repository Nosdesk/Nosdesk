# Sync model registry

JSON manifests describing each aggregate that ships through the sync
engine (`sync_actions` table). Single source of truth for:

- Aggregate name (matches the `sync_aggregate` Postgres ENUM and the
  Rust `SyncAggregate` enum).
- Current payload `schema_version` (stamped on every emitted row).
- The set of `event_type` strings the aggregate emits, with their op
  (I / U / D / A).
- Field shapes — for a future codegen pass that emits Rust types and
  TypeScript types from these manifests.

Today the manifests are validated by `tests/sync_model_registry.rs`,
which asserts:

1. Every `SyncAggregate` enum variant has a matching `.json` manifest.
2. The `name` field in each manifest matches the enum's `as_str()`.
3. The `schema_version` matches `sync::registry::schema_version_for`.
4. Every `event_type` referenced by `repository::*` exists in some
   manifest.

A future commit will add a `build.rs` that generates Rust struct
definitions from these manifests (replacing the hand-written
`models::SyncAggregate` etc.) and a Vite plugin that emits matching
TypeScript types into `frontend/src/sync/generated/`. The pre-codegen
shape lives here so consumers are never out-of-step with what the
substrate emits.

## File layout

One file per aggregate, named `<aggregate>.json`. Aggregate names use
underscores (`workflow_state.json`), not camelCase or kebab-case.
