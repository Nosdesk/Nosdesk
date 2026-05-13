//! RFC 3676 Format=flowed plaintext unfolding.
//!
//! Mail clients that emit `text/plain; format=flowed` (Apple Mail
//! by default, Thunderbird, mutt, several mailing-list robots)
//! soft-wrap their bodies at 78-80 columns by appending a space
//! to every line that should logically continue onto the next.
//! A naive renderer that treats every newline as a hard break
//! produces awkwardly short lines with no real structure; worse,
//! our quote splitter's "On ... wrote:" header detection matches
//! the wrapped header inconsistently.
//!
//! The fix is the unfolder published in RFC 3676 §4.5:
//!
//!   1. **Quote depth.** Count leading `>` characters per line.
//!      Two lines may only join when their quote depths match.
//!   2. **Space stuffing.** A leading space after the quote
//!      markers is escape padding and gets stripped. A line that
//!      legitimately starts with `From ` would be space-stuffed to
//!      ` From ` on the wire so mbox parsers don't treat it as a
//!      message separator.
//!   3. **Soft break.** A line whose stuffed content ends in a
//!      space is a soft break: join it to the next line if the
//!      depths match. With `DelSp=yes` parameter the trailing
//!      space is removed when joining; default (`DelSp=no`) keeps
//!      it.
//!   4. **Signature delimiter.** The line `-- ` (two dashes plus
//!      a space) is a signature separator per RFC 3676 §4.3 and
//!      is *never* a soft break — that trailing space is part of
//!      the protocol delimiter, not a continuation marker.
//!
//! The unfolder runs at MIME parse time so the splitter and
//! sanitiser downstream see a normalised plain-text body. Inputs
//! that aren't flowed (no Content-Type `format=flowed` parameter)
//! pass through untouched.

/// Unfold a plain-text body per RFC 3676. `delsp` is the parameter
/// value from the message's Content-Type header — `true` means
/// strip the trailing space when joining soft-wrapped lines.
pub fn unfold(body: &str, delsp: bool) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut pending: Option<Pending> = None;

    for raw_line in body.split('\n') {
        // Tolerate CRLF without an explicit CRLF split pass.
        let line = raw_line.trim_end_matches('\r');

        let (depth, content) = strip_quote_prefix(line);
        let content = strip_space_stuffing(content);
        let is_sig = content == "-- ";
        let is_soft = content.ends_with(' ') && !is_sig;

        // If we're mid-join, try to extend the pending buffer.
        // The depth gate is the load-bearing safety check: a line
        // with different quote depth is structurally a different
        // paragraph, so we flush the pending buffer first and
        // reprocess the current line as a fresh start.
        if let Some(p) = pending.as_mut() {
            if p.depth == depth {
                if delsp && p.content.ends_with(' ') {
                    p.content.pop();
                }
                p.content.push_str(content);
                if !is_soft {
                    out.push(format_line(p.depth, &p.content));
                    pending = None;
                }
                continue;
            }
            // Depth mismatch: flush pending, fall through.
            out.push(format_line(p.depth, &p.content));
            pending = None;
        }

        if is_soft {
            pending = Some(Pending {
                depth,
                content: content.to_string(),
            });
        } else {
            out.push(format_line(depth, content));
        }
    }

    // A trailing soft-wrap that never found a partner still has
    // to be emitted — drop it on the floor and the user loses
    // their last word.
    if let Some(p) = pending {
        out.push(format_line(p.depth, &p.content));
    }

    out.join("\n")
}

/// Accumulator for an in-progress soft-wrap join. Holds the
/// stuffed content (no leading `>` markers, no escape padding)
/// and the quote depth the next line must match to extend.
struct Pending {
    depth: usize,
    content: String,
}

fn strip_quote_prefix(line: &str) -> (usize, &str) {
    let depth = line.bytes().take_while(|&b| b == b'>').count();
    (depth, &line[depth..])
}

/// Strip ONE leading space (space-stuffing per §4.4). More than
/// one is meaningful indentation that came from the sender and
/// stays.
fn strip_space_stuffing(s: &str) -> &str {
    s.strip_prefix(' ').unwrap_or(s)
}

/// Re-emit a line with its canonical quote prefix. `> ` between
/// the markers and content matches what Gmail, Apple Mail, and
/// mutt all emit and what the splitter's `>` run-detector
/// expects.
fn format_line(depth: usize, content: &str) -> String {
    let mut out = String::with_capacity(depth * 2 + 1 + content.len());
    for _ in 0..depth {
        out.push('>');
    }
    if depth > 0 && !content.is_empty() {
        out.push(' ');
    }
    out.push_str(content);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_soft_wrapped_lines() {
        // Apple Mail emits a 78-column soft-wrap with a trailing
        // space on the continued line. The unfolder should join
        // these into one logical paragraph.
        let input = "Hi there, I wanted to ask whether the maintenance \nwindow is still planned for Friday night.";
        let out = unfold(input, false);
        assert_eq!(
            out,
            "Hi there, I wanted to ask whether the maintenance window is still planned for Friday night."
        );
    }

    #[test]
    fn preserves_hard_breaks() {
        // Lines without trailing space are hard breaks and stay
        // separated. This is what an interactive sender (typing
        // a list) emits.
        let input = "Line one.\nLine two.\nLine three.";
        let out = unfold(input, false);
        assert_eq!(out, "Line one.\nLine two.\nLine three.");
    }

    #[test]
    fn delsp_strips_trailing_space() {
        // With DelSp=yes the wrapping space is part of the
        // soft-break marker and gets removed at join time. Most
        // commonly seen on CJK content where word boundaries
        // aren't space-delimited.
        let input = "first \nsecond";
        assert_eq!(unfold(input, true), "firstsecond");
        assert_eq!(unfold(input, false), "first second");
    }

    #[test]
    fn signature_delimiter_is_a_hard_break() {
        // `-- ` (with trailing space) is the RFC 3676 signature
        // delimiter; it must NOT join with the next line.
        let input = "Body text.\n-- \nSignature here.";
        let out = unfold(input, false);
        assert_eq!(out, "Body text.\n-- \nSignature here.");
    }

    #[test]
    fn space_stuffing_stripped() {
        // A line that legitimately starts with `From ` would be
        // wire-encoded as ` From ` (space-stuffed) so mbox-style
        // parsers don't mis-split on it. The unfolder strips one
        // leading space.
        let input = " From me to you";
        let out = unfold(input, false);
        assert_eq!(out, "From me to you");
    }

    #[test]
    fn quoted_lines_join_when_depths_match() {
        // Quote depth must match for a soft-wrap join. Two
        // depth-1 lines join; a depth-1 followed by depth-2
        // would flush the first as-is.
        let input = "> The first part of the quoted message \n> and its continuation.";
        let out = unfold(input, false);
        assert_eq!(
            out,
            "> The first part of the quoted message and its continuation."
        );
    }

    #[test]
    fn depth_mismatch_breaks_soft_wrap() {
        // Soft-wrap on a depth-0 line whose continuation has
        // depth 1 — the depths don't match so the original line
        // emits as-is (with its trailing soft-break space
        // preserved) and the depth-1 line stands alone.
        let input = "Soft wrap here \n> and a quoted reply.";
        let out = unfold(input, false);
        assert_eq!(out, "Soft wrap here \n> and a quoted reply.");
    }

    #[test]
    fn empty_lines_preserved() {
        // Blank paragraph separators must survive unfolding so
        // the downstream splitter and renderer see the same
        // paragraph structure the sender intended.
        let input = "Paragraph one. \nstill one.\n\nParagraph two.";
        let out = unfold(input, false);
        assert_eq!(out, "Paragraph one. still one.\n\nParagraph two.");
    }

    #[test]
    fn crlf_tolerated() {
        // Mail clients emit CRLF on the wire. The unfolder must
        // strip the CR before evaluating soft-break trailing
        // spaces.
        let input = "soft \r\nbreak";
        let out = unfold(input, false);
        assert_eq!(out, "soft break");
    }

    #[test]
    fn three_line_chain_joins_in_one_paragraph() {
        // Long wrapped paragraphs span more than two lines. Each
        // soft break chains into the next until a hard break
        // terminates the paragraph.
        let input = "one \ntwo \nthree \nfour.";
        let out = unfold(input, false);
        assert_eq!(out, "one two three four.");
    }

    #[test]
    fn trailing_soft_wrap_with_no_partner_still_emits() {
        // If the last line is itself soft-wrapped but there's
        // nothing after it, we still emit the buffered content
        // rather than silently dropping the sender's final word.
        let input = "lonely soft wrap ";
        let out = unfold(input, false);
        assert_eq!(out, "lonely soft wrap ");
    }
}
