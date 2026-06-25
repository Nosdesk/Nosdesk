//! Active Directory DirSync control (OID 1.2.840.113556.1.4.841) for
//! incremental sync. AD has no turnkey ldap3 helper, so the control value is
//! hand-BER-encoded here; the request/response shapes are tiny ASN.1 and are
//! byte-level unit-tested against the spec (the encoding is the silent-failure-
//! prone part — a wrong cookie means repeated full syncs or missed changes).
//!
//! Request value:  `SEQUENCE { Flags INTEGER, MaxBytes INTEGER, Cookie OCTET STRING }`
//! Response value: `SEQUENCE { MoreResults INTEGER, unused INTEGER, Cookie OCTET STRING }`
//!
//! The AD round-trip (does the DC accept the control + return deltas) is
//! exercised by the P3 integration harness; the codec correctness is locked here.

use ldap3::controls::RawControl;

pub const DIRSYNC_OID: &str = "1.2.840.113556.1.4.841";

/// Standard DirSync: full attribute values for changed objects, i.e. no
/// LDAP_DIRSYNC_INCREMENTAL_VALUES (which would return only changed values of
/// multi-valued attrs) and no OBJECT_SECURITY. `MAX_BYTES` is the per-reply byte
/// cap (MS-ADTS 3.1.1.3.4.1.3); AD clamps it up to a 0x100000 floor, and
/// batching is handled by the MoreResults + cookie loop regardless, so we send
/// the floor explicitly.
const FLAGS: i32 = 0;
const MAX_BYTES: i32 = 0x100000;

#[derive(Debug, thiserror::Error)]
pub enum DirSyncError {
    #[error("malformed DirSync response control: {0}")]
    Decode(&'static str),
}

/// The parsed DirSync response control.
#[derive(Debug, Clone, PartialEq)]
pub struct DirSyncResponse {
    /// The server has more changes than fit this batch (call again with the
    /// returned cookie before treating the sync as complete).
    pub more_results: bool,
    /// The opaque cookie to persist and send on the next DirSync call.
    pub cookie: Vec<u8>,
}

/// Build the critical DirSync request control carrying `cookie` (empty for the
/// first/full sync).
pub fn request_control(cookie: &[u8]) -> RawControl {
    RawControl {
        ctype: DIRSYNC_OID.to_string(),
        crit: true,
        val: Some(encode_request_value(FLAGS, MAX_BYTES, cookie)),
    }
}

/// Encode the DirSync request control value.
pub fn encode_request_value(flags: i32, max_bytes: i32, cookie: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&tlv(0x02, &encode_int(flags)));
    body.extend_from_slice(&tlv(0x02, &encode_int(max_bytes)));
    body.extend_from_slice(&tlv(0x04, cookie));
    tlv(0x30, &body)
}

/// Decode the DirSync response control value into the more-results flag + the
/// new cookie.
pub fn decode_response_value(val: &[u8]) -> Result<DirSyncResponse, DirSyncError> {
    let mut p = Parser::new(val);
    let seq = p.expect_tag(0x30)?;
    let mut sp = Parser::new(seq);
    let more = sp.expect_tag(0x02)?;
    let _unused = sp.expect_tag(0x02)?;
    let cookie = sp.expect_tag(0x04)?;
    Ok(DirSyncResponse {
        more_results: more.iter().any(|&b| b != 0),
        cookie: cookie.to_vec(),
    })
}

// ---- Minimal DER primitives ------------------------------------------------

/// tag + definite length + content.
fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + content.len());
    out.push(tag);
    out.extend_from_slice(&der_len(content.len()));
    out.extend_from_slice(content);
    out
}

/// Definite-form length. Short form for < 128, long form otherwise.
fn der_len(n: usize) -> Vec<u8> {
    if n < 0x80 {
        return vec![n as u8];
    }
    let bytes = n.to_be_bytes();
    let first = bytes
        .iter()
        .position(|&b| b != 0)
        .unwrap_or(bytes.len() - 1);
    let sig = &bytes[first..];
    let mut out = Vec::with_capacity(1 + sig.len());
    out.push(0x80 | sig.len() as u8);
    out.extend_from_slice(sig);
    out
}

/// Minimal two's-complement big-endian INTEGER content (at least one byte; a
/// leading 0x00 when the high bit would otherwise read as negative).
fn encode_int(v: i32) -> Vec<u8> {
    let be = v.to_be_bytes();
    if v >= 0 {
        // Drop leading 0x00s, but keep one if the next byte's high bit is set
        // (so a positive value isn't misread as negative), and keep at least one.
        let mut i = 0;
        while i < be.len() - 1 && be[i] == 0x00 && be[i + 1] & 0x80 == 0 {
            i += 1;
        }
        be[i..].to_vec()
    } else {
        // Drop leading 0xff while the result stays negative; keep at least one.
        let mut i = 0;
        while i < be.len() - 1 && be[i] == 0xff && be[i + 1] & 0x80 != 0 {
            i += 1;
        }
        be[i..].to_vec()
    }
}

struct Parser<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Read one TLV, require its tag, and return its content slice.
    fn expect_tag(&mut self, tag: u8) -> Result<&'a [u8], DirSyncError> {
        if self.pos >= self.buf.len() {
            return Err(DirSyncError::Decode("truncated: no tag"));
        }
        if self.buf[self.pos] != tag {
            return Err(DirSyncError::Decode("unexpected tag"));
        }
        self.pos += 1;
        let len = self.read_len()?;
        let end = self
            .pos
            .checked_add(len)
            .filter(|&e| e <= self.buf.len())
            .ok_or(DirSyncError::Decode("truncated: content past end"))?;
        let content = &self.buf[self.pos..end];
        self.pos = end;
        Ok(content)
    }

    fn read_len(&mut self) -> Result<usize, DirSyncError> {
        let first = *self
            .buf
            .get(self.pos)
            .ok_or(DirSyncError::Decode("truncated: no length"))?;
        self.pos += 1;
        if first < 0x80 {
            return Ok(first as usize);
        }
        let count = (first & 0x7f) as usize;
        if count == 0 || count > 4 {
            return Err(DirSyncError::Decode("unsupported length form"));
        }
        let mut len = 0usize;
        for _ in 0..count {
            let b = *self
                .buf
                .get(self.pos)
                .ok_or(DirSyncError::Decode("truncated: length bytes"))?;
            self.pos += 1;
            len = (len << 8) | b as usize;
        }
        Ok(len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_int_minimally() {
        assert_eq!(encode_int(0), vec![0x00]);
        assert_eq!(encode_int(1), vec![0x01]);
        assert_eq!(encode_int(127), vec![0x7f]);
        // 128's high bit is set, so a 0x00 pad keeps it positive.
        assert_eq!(encode_int(128), vec![0x00, 0x80]);
        assert_eq!(encode_int(256), vec![0x01, 0x00]);
    }

    #[test]
    fn encodes_the_first_sync_request() {
        // Flags=0, MaxBytes=0, empty cookie:
        // SEQUENCE(len 8) { INTEGER 0, INTEGER 0, OCTET STRING "" }
        assert_eq!(
            encode_request_value(0, 0, &[]),
            vec![0x30, 0x08, 0x02, 0x01, 0x00, 0x02, 0x01, 0x00, 0x04, 0x00]
        );
    }

    #[test]
    fn encodes_a_request_with_a_cookie() {
        let v = encode_request_value(1, 0, &[0xde, 0xad, 0xbe, 0xef]);
        // SEQUENCE(len 12) { INTEGER 1, INTEGER 0, OCTET STRING <4 bytes> }
        assert_eq!(
            v,
            vec![
                0x30, 0x0c, 0x02, 0x01, 0x01, 0x02, 0x01, 0x00, 0x04, 0x04, 0xde, 0xad, 0xbe, 0xef
            ]
        );
    }

    #[test]
    fn decodes_a_response_with_more_results_and_a_cookie() {
        // SEQUENCE(len 10) { INTEGER 1 (more), INTEGER 0, OCTET STRING <cookie> }
        let bytes = vec![
            0x30, 0x0a, 0x02, 0x01, 0x01, 0x02, 0x01, 0x00, 0x04, 0x02, 0xca, 0xfe,
        ];
        let resp = decode_response_value(&bytes).unwrap();
        assert!(resp.more_results);
        assert_eq!(resp.cookie, vec![0xca, 0xfe]);
    }

    #[test]
    fn decodes_a_final_response() {
        // more=0, empty cookie -> sync complete, no cursor advance content.
        let bytes = vec![0x30, 0x08, 0x02, 0x01, 0x00, 0x02, 0x01, 0x00, 0x04, 0x00];
        let resp = decode_response_value(&bytes).unwrap();
        assert!(!resp.more_results);
        assert!(resp.cookie.is_empty());
    }

    #[test]
    fn round_trips_a_long_cookie() {
        // A >127-byte cookie exercises the long-form length on both sides.
        let cookie: Vec<u8> = (0..200u32).map(|i| (i % 256) as u8).collect();
        let req = encode_request_value(0, 0, &cookie);
        // Re-shape as a response (more=0) to round-trip the cookie through decode.
        let mut body = Vec::new();
        body.extend_from_slice(&tlv(0x02, &encode_int(0)));
        body.extend_from_slice(&tlv(0x02, &encode_int(0)));
        body.extend_from_slice(&tlv(0x04, &cookie));
        let resp = decode_response_value(&tlv(0x30, &body)).unwrap();
        assert_eq!(resp.cookie, cookie);
        // The request encoded the same cookie octet-string too.
        assert!(req.windows(cookie.len()).any(|w| w == cookie.as_slice()));
    }

    #[test]
    fn rejects_malformed_response() {
        assert!(decode_response_value(&[0x30, 0x02, 0x02]).is_err());
        assert!(decode_response_value(&[]).is_err());
    }
}
