//! Identity for this backend process.
//!
//! Distinct from `system_meta.instance_id`, which identifies the *installation*
//! and is shared by every process serving it. This one changes on every restart
//! and differs between replicas, which is exactly what makes it useful for
//! telling two machines apart in an operator surface.

use std::sync::OnceLock;
use uuid::Uuid;

/// Short, stable-for-this-process id.
///
/// Minted on first use rather than at startup so nothing has to remember to
/// initialise it, and truncated to 8 characters because its only job is to let
/// a human notice that two responses came from different processes. It is not a
/// secret, not a correlation id for tracing, and never leaves an operator
/// surface or a log line.
pub fn process_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| Uuid::now_v7().simple().to_string()[..8].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stable within a process: an operator comparing two responses must be
    /// able to conclude "same machine" from a match.
    #[test]
    fn process_id_is_stable_and_short() {
        assert_eq!(process_id(), process_id());
        assert_eq!(process_id().len(), 8);
        assert!(process_id().chars().all(|c| c.is_ascii_hexdigit()));
    }
}
