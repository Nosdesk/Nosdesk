//! Windows SID helpers for AD primaryGroupID resolution.
//!
//! A user's PRIMARY group (Domain Users by default, sometimes a department) is
//! NOT listed in the group's `member` attribute. Instead the user object carries
//! `primaryGroupID` = the primary group's RID (relative identifier). The group's
//! SID is the user's domain SID with the trailing RID swapped for the
//! primaryGroupID, so we can reconstruct it and match it against the group's own
//! `objectSid`.
//!
//! SID binary layout (MS-DTYP 2.4.2.2): revision(1) + sub-authority count N(1) +
//! identifier authority(6) + N sub-authorities(4 each, little-endian). The last
//! sub-authority is the RID.

/// Lowercase hex (no separators) — the stable key both sides match on.
pub fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Build the primary group's SID from a user's `objectSid` + `primaryGroupID`,
/// by replacing the trailing user RID with the group RID. Returns `None` if the
/// SID is malformed (so a bad blob is skipped, never panics).
pub fn primary_group_sid(user_sid: &[u8], primary_group_rid: u32) -> Option<Vec<u8>> {
    let sub_count = *user_sid.get(1)? as usize;
    // Header is 8 bytes; each sub-authority is 4. Need at least one (the RID).
    if sub_count == 0 || user_sid.len() != 8 + 4 * sub_count {
        return None;
    }
    let mut sid = user_sid[..user_sid.len() - 4].to_vec();
    sid.extend_from_slice(&primary_group_rid.to_le_bytes());
    Some(sid)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// S-1-5-21-A-B-C-1105 (a domain user, RID 1105).
    fn sample_user_sid() -> Vec<u8> {
        let mut sid = vec![0x01, 0x05]; // revision 1, 5 sub-authorities
        sid.extend_from_slice(&[0, 0, 0, 0, 0, 5]); // NT authority (5)
        sid.extend_from_slice(&21u32.to_le_bytes()); // sub-auth 1: domain id part
        sid.extend_from_slice(&0xAAAA_AAAAu32.to_le_bytes());
        sid.extend_from_slice(&0xBBBB_BBBBu32.to_le_bytes());
        sid.extend_from_slice(&0xCCCC_CCCCu32.to_le_bytes());
        sid.extend_from_slice(&1105u32.to_le_bytes()); // user RID
        sid
    }

    #[test]
    fn swaps_the_rid_keeping_the_domain() {
        let user = sample_user_sid();
        // Primary group = Domain Users (RID 513).
        let pg = primary_group_sid(&user, 513).unwrap();
        // Same length + identical domain prefix; only the trailing RID differs.
        assert_eq!(pg.len(), user.len());
        assert_eq!(pg[..user.len() - 4], user[..user.len() - 4]);
        assert_eq!(pg[user.len() - 4..], 513u32.to_le_bytes());
        // Idempotent + matchable via hex.
        assert_eq!(to_hex(&pg), to_hex(&primary_group_sid(&user, 513).unwrap()));
        assert_ne!(to_hex(&pg), to_hex(&user));
    }

    #[test]
    fn rejects_malformed() {
        assert!(primary_group_sid(&[], 513).is_none());
        assert!(primary_group_sid(&[0x01, 0x00], 513).is_none()); // zero sub-auths
        assert!(primary_group_sid(&[0x01, 0x05, 0, 0, 0, 0, 0, 5], 513).is_none());
        // truncated
    }
}
