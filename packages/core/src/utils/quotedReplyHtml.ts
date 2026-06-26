/**
 * Split an inbound HTML email body into the visible new content and
 * the quoted history that mail clients normally collapse on reply.
 *
 * The plaintext sibling (`splitQuotedReply` in `./quotedReply.ts`)
 * relies on conventions like `> ` line prefixes and `On X wrote:`
 * intros. HTML emails carry the same intent through markup instead:
 *
 *   - Gmail wraps the prior thread in `<div class="gmail_quote">`.
 *   - Outlook desktop / Outlook on the web emit
 *     `<div id="divRplyFwdMsg">` or `<div id="appendonsend">` and a
 *     thin `<hr>` immediately above the prior thread.
 *   - Apple Mail uses `<blockquote type="cite">`.
 *
 * Detection cascades through the most-specific markers first; bare
 * `<blockquote>` is the last resort so a customer who legitimately
 * pastes a single quoted line into their reply doesn't get the rest
 * of their message hidden.
 *
 * The function operates on top-level body children only, mirroring
 * the way mail clients structure replies. Quoted blocks deeper in
 * the tree (e.g. an inline pull quote) are left where they are.
 */

/** First-pass selectors evaluated in priority order. */
const QUOTE_SELECTORS = [
    'div.gmail_quote',
    'div.gmail_extra > div.gmail_quote',
    'div#divRplyFwdMsg',
    'div#appendonsend',
    'div.OutlookMessageHeader',
    'blockquote[type="cite"]',
    'blockquote.gmail_quote',
    'blockquote',
] as const;

export interface QuotedHtmlSplit {
    /** HTML the reader sees by default. */
    visibleHtml: string;
    /** Quoted history HTML to put behind a disclosure; empty when none. */
    trimmedHtml: string;
}

export function splitQuotedHtml(html: string): QuotedHtmlSplit {
    if (!html) return { visibleHtml: '', trimmedHtml: '' };

    let doc: Document;
    try {
        doc = new DOMParser().parseFromString(html, 'text/html');
    } catch {
        // Defensive — DOMParser doesn't typically throw, but if it ever
        // does (malformed XHTML, edge browser quirks) we'd rather
        // render the whole body than lose it.
        return { visibleHtml: html, trimmedHtml: '' };
    }

    const body = doc.body;
    if (!body) return { visibleHtml: html, trimmedHtml: '' };

    const quote = findTopLevelQuote(body);
    if (!quote) return { visibleHtml: html, trimmedHtml: '' };

    // Walk body's direct children. Anything before the quote (and any
    // sibling we already classified as visible) goes into the visible
    // bucket; the quote and everything after goes into trimmed.
    const visibleNodes: Node[] = [];
    const trimmedNodes: Node[] = [];
    let crossed = false;
    for (const child of Array.from(body.childNodes)) {
        if (!crossed && (child === quote || nodeContains(child, quote))) {
            crossed = true;
        }
        (crossed ? trimmedNodes : visibleNodes).push(child);
    }

    const visibleHtml = stringifyNodes(visibleNodes).trim();
    const trimmedHtml = stringifyNodes(trimmedNodes).trim();

    // If the entire body turned out to be the quote itself, treat it
    // as no quote — splitting would leave the visible side empty,
    // which is worse than just showing the original body.
    if (!visibleHtml) return { visibleHtml: html, trimmedHtml: '' };

    return { visibleHtml, trimmedHtml };
}

/**
 * Find the first quote container that's either a direct child of body
 * or wraps something above body level (e.g. nested inside a wrapper
 * `<div>` Gmail sometimes adds). We only collapse top-level quotes
 * because deeper quotes are usually intentional inline citations.
 */
function findTopLevelQuote(body: HTMLElement): Element | null {
    for (const sel of QUOTE_SELECTORS) {
        const el = body.querySelector(sel);
        if (el && isTopLevel(el, body)) return el;
    }
    return null;
}

/**
 * The element is "top-level" for our purposes when its chain of
 * parents up to body contains nothing but block-level wrappers (div,
 * body) — i.e. it isn't sitting inside a paragraph or list item where
 * collapsing would chop the visible body apart.
 */
function isTopLevel(el: Element, body: HTMLElement): boolean {
    let cur: Element | null = el.parentElement;
    while (cur && cur !== body) {
        const tag = cur.tagName.toLowerCase();
        if (tag !== 'div' && tag !== 'span') return false;
        cur = cur.parentElement;
    }
    return cur === body;
}

function nodeContains(maybeAncestor: Node, target: Element): boolean {
    return (
        maybeAncestor.nodeType === Node.ELEMENT_NODE &&
        (maybeAncestor as Element).contains(target)
    );
}

function stringifyNodes(nodes: Node[]): string {
    let out = '';
    for (const n of nodes) {
        if (n.nodeType === Node.ELEMENT_NODE) {
            out += (n as Element).outerHTML;
        } else if (n.nodeType === Node.TEXT_NODE) {
            // Preserve text-node whitespace verbatim — it's significant
            // for inline layout (e.g. trailing space before a link).
            out += n.textContent ?? '';
        }
        // Comment / doctype nodes are skipped — they have no display
        // value and cluttering the output with them serves no one.
    }
    return out;
}
