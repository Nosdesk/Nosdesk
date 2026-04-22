/**
 * Split an email-style message body into the "new content" the reader
 * cares about and the "trimmed content" (quoted history, forwarded
 * blocks) that should start collapsed.
 *
 * When a customer replies to a ticket, their mail client typically
 * prepends their new text to the entire previous thread:
 *
 *   Thanks, will do!
 *
 *   On Tue, Jan 2, 2024 at 10:15 AM Tech <tech@yourco.com> wrote:
 *   > Hi, we'll look into this...
 *   > ...
 *
 * Rendering the whole thing in the timeline buries what matters.
 * Zendesk, Freshdesk, Help Scout all collapse the quoted portion by
 * default with a disclosure affordance.
 *
 * Detection is intentionally conservative: we split at the **first**
 * unambiguous marker and only keep the tail as "trimmed". Ambiguous
 * or borderline bodies are returned as `{ visible: entire, trimmed:
 * "" }` — rendering the full body is always the safe default.
 *
 * Patterns recognised:
 *
 *   - Gmail-style intro: `On <date>, <name> wrote:` (followed by
 *     `>`-prefixed lines)
 *   - Plain `>`-prefixed quote block starting on its own line
 *   - Outlook rule + `From:` header block
 *     (`________________________________` + `From:`)
 *   - `-----Original Message-----` delimiter
 *   - `Begin forwarded message:` (Apple Mail)
 *   - `---------- Forwarded message ---------` (Gmail)
 *   - `-------- Forwarded Message --------` (Thunderbird)
 */
export interface QuotedSplit {
  /** Content that should be shown by default in the comment body. */
  visible: string;
  /** Quoted history to put behind a disclosure; empty string if none. */
  trimmed: string;
}

/**
 * Split `body` into visible + trimmed parts. Always returns both
 * fields; when no quoted section is detected, `trimmed` is empty.
 */
export function splitQuotedReply(body: string): QuotedSplit {
  if (!body) return { visible: '', trimmed: '' };

  const idx = findQuoteStart(body);
  if (idx === null) return { visible: body, trimmed: '' };

  // Trim trailing whitespace off the visible portion so the disclosure
  // sits tightly below the new content rather than after 3 blank lines.
  const visible = body.slice(0, idx).replace(/\s+$/, '');
  const trimmed = body.slice(idx).replace(/^\s+/, '');
  return { visible, trimmed };
}

/**
 * Scan for the first marker that unambiguously begins a quoted /
 * forwarded section. Returns the starting index or `null`.
 *
 * Each pattern is evaluated independently and we take the earliest
 * match. Gmail's "On ... wrote:" only counts when followed by a
 * `>`-prefixed line within a few lines, because that phrase can
 * legitimately appear in prose.
 */
function findQuoteStart(body: string): number | null {
  const candidates: number[] = [];

  // Explicit delimiters — these are unambiguous, match on their own
  // line, and can be taken at face value.
  const explicitPatterns: RegExp[] = [
    /^\s*-----\s*Original Message\s*-----\s*$/m,
    /^\s*-{3,}\s*Forwarded message\s*-{3,}\s*$/mi,
    /^\s*-{3,}\s*Forwarded Message\s*-{3,}\s*$/m,
    /^\s*Begin forwarded message:\s*$/m,
    /^\s*_{10,}\s*$/m,
  ];
  for (const re of explicitPatterns) {
    const m = re.exec(body);
    if (m && m.index !== undefined) candidates.push(m.index);
  }

  // Gmail/Outlook attribution intro. English ships as "On … wrote:",
  // but Gmail/Outlook localise the verb ("schrieb", "a écrit",
  // "escribió", "schreef", "scrisse", "napisał(a)", "napsal(a)",
  // "skrev", "kirjoitti", "yazdı", "написал(а)"). We match a handful
  // of common ones — better to over-collapse a little than to leave
  // quoted history uncollapsed for every non-English customer.
  const introVerbs =
    '(?:wrote|schrieb|a\\s+écrit|escribió|escreveu|ha\\s+scritto|scrisse|schreef|napisał(?:a)?|napsal(?:a)?|skrev|kirjoitti|yazdı|написал(?:а)?)';
  const gmailIntro = new RegExp(`^\\s*(?:On|Am|Le|El|Il|A|W\\s+dniu|Dne|Den|Le|På)\\s+.+?\\s+${introVerbs}:\\s*$`, 'mi');
  const gmailMatch = gmailIntro.exec(body);
  if (gmailMatch && gmailMatch.index !== undefined) {
    const after = body.slice(gmailMatch.index + gmailMatch[0].length, gmailMatch.index + gmailMatch[0].length + 500);
    if (/\n\s*>/.test(after) || /\n>/.test(after)) {
      candidates.push(gmailMatch.index);
    }
  }

  // Plain `> ` quote block: the first line at column 0 starting with
  // `>` (with or without space), PROVIDED an earlier non-quoted line
  // has real content. Avoids false-positive on an entire body that
  // happens to start with `>` (someone legitimately pasting a quote).
  const quoteLine = /^>/m;
  const qMatch = quoteLine.exec(body);
  if (qMatch && qMatch.index !== undefined && qMatch.index > 0) {
    const before = body.slice(0, qMatch.index);
    if (/\S/.test(before)) candidates.push(qMatch.index);
  }

  // Outlook-style `From: / Sent: / To:` block header without the
  // explicit horizontal rule. Requires three consecutive header
  // lines to avoid matching a casual "From: John" sentence. Matches
  // the common localised labels Outlook ships — en / de / fr / es /
  // it / nl / pt.
  const fromLabel = '(?:From|Von|De|Da|Van)';
  const sentLabel = '(?:Sent|Gesendet|Envoyé|Enviado|Inviato|Verzonden)';
  const toLabel = '(?:To|An|À|A|Para|Aan)';
  const outlookHeader = new RegExp(
    `^\\s*${fromLabel}:\\s.+\\n\\s*${sentLabel}:\\s.+\\n\\s*${toLabel}:\\s.+$`,
    'mi',
  );
  const outMatch = outlookHeader.exec(body);
  if (outMatch && outMatch.index !== undefined) candidates.push(outMatch.index);

  if (candidates.length === 0) return null;
  return Math.min(...candidates);
}
