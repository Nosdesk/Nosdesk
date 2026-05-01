/**
 * useTableOfContents — derive a live table of contents from a
 * caller-supplied DOM root.
 *
 * The composable encapsulates the two side-effects this kind of
 * outline view needs:
 *   1. A `MutationObserver` on the root, so the heading list stays
 *      current as the document mutates (collaborative edits, paste,
 *      undo). Vue has no higher-level abstraction for this and the
 *      browser primitive is the documented idiom — see
 *      https://vuejs.org/guide/essentials/lifecycle.html for
 *      lifecycle pairing.
 *   2. An `IntersectionObserver` for active-heading tracking. We
 *      avoid scroll listeners because they fire continuously and
 *      cost layout work each frame; the IntersectionObserver wakes
 *      only when a heading crosses the activation rectangle.
 *
 * The `source` argument is a getter — `() => element.value` style —
 * so callers can pass a `ref<HTMLElement | null>`, a prop that
 * tracks the DOM (`() => props.element`), or any other reactive
 * expression. We re-attach observers whenever the source resolves
 * to a different element, and cleanly disconnect on unmount.
 *
 * Heading levels h1-h4 are walked. Anything deeper is rare in a doc
 * outline and clutters the rail.
 */

import { onScopeDispose, ref, watch, type Ref } from 'vue';

export interface TocEntry {
  /** Stable slug used as anchor + key. */
  id: string;
  /** Header text content as plain text. */
  text: string;
  /** Heading level 1-6, used to indent nested entries. */
  level: number;
  /** Element reference so callers can scroll to it without a fresh
   *  DOM query at click time. */
  el: HTMLElement;
}

export interface UseTableOfContentsResult {
  /** Heading list, kept current as the source DOM mutates. */
  headings: Ref<TocEntry[]>;
  /** Anchor id of the heading currently in the activation zone, or
   *  `null` when nothing matches yet. Drives the highlight in the
   *  rail. */
  activeId: Ref<string | null>;
}

/**
 * @param source Getter that returns the DOM element to observe, or
 *               `null` when no element is available yet. The
 *               composable watches it and re-attaches observers
 *               on every truthy change.
 */
export function useTableOfContents(
  source: () => HTMLElement | null,
): UseTableOfContentsResult {
  const headings = ref<TocEntry[]>([]);
  const activeId = ref<string | null>(null);

  let mutationObserver: MutationObserver | null = null;
  let intersectionObserver: IntersectionObserver | null = null;

  /** Slugify a heading's text into a stable anchor id. Two headings
   *  with identical text get a numeric suffix so each entry is a
   *  unique scroll target. */
  function slugify(text: string, used: Set<string>): string {
    const base =
      text
        .toLowerCase()
        .trim()
        .replace(/[^\w\s-]/g, '')
        .replace(/\s+/g, '-')
        .slice(0, 80) || 'heading';
    let candidate = base;
    let n = 1;
    while (used.has(candidate)) {
      candidate = `${base}-${++n}`;
    }
    used.add(candidate);
    return candidate;
  }

  function disconnectObservers() {
    mutationObserver?.disconnect();
    intersectionObserver?.disconnect();
    mutationObserver = null;
    intersectionObserver = null;
  }

  /** Walk the root, collect h1-h4, stamp slug ids, rebuild the
   *  entries list, and re-attach the intersection observer to the
   *  new set. Wrapped in try/catch so a malformed doc never bubbles
   *  past the composable boundary. */
  function rebuild(root: HTMLElement) {
    try {
      const used = new Set<string>();
      const out: TocEntry[] = [];
      const nodes = root.querySelectorAll('h1, h2, h3, h4');
      nodes.forEach((node) => {
        const el = node as HTMLElement;
        const text = (el.textContent ?? '').trim();
        if (!text) return; // skip empty heading placeholders
        const level = Number(el.tagName.slice(1));
        if (!el.id) {
          el.id = slugify(text, used);
        } else {
          used.add(el.id);
        }
        out.push({ id: el.id, text, level, el });
      });
      headings.value = out;
      attachIntersection(out);
    } catch (err) {
      // Defensive: never let an outline bug crash the page.
      // eslint-disable-next-line no-console
      console.warn('[useTableOfContents] failed to walk headings', err);
      headings.value = [];
    }
  }

  function attachIntersection(entries: TocEntry[]) {
    intersectionObserver?.disconnect();
    intersectionObserver = null;
    if (entries.length === 0 || typeof IntersectionObserver === 'undefined') {
      activeId.value = null;
      return;
    }
    // `rootMargin` biases the activation zone to the top quarter of
    // the viewport so a heading reads as "active" once it scrolls
    // into the upper reading band, rather than when it's centred.
    intersectionObserver = new IntersectionObserver(
      (records) => {
        const visible = records
          .filter((r) => r.isIntersecting)
          .sort(
            (a, b) =>
              a.boundingClientRect.top - b.boundingClientRect.top,
          );
        if (visible.length > 0) {
          activeId.value = (visible[0].target as HTMLElement).id;
        }
      },
      { rootMargin: '0px 0px -75% 0px', threshold: 0 },
    );
    for (const entry of entries) intersectionObserver.observe(entry.el);
  }

  function attachMutation(root: HTMLElement) {
    mutationObserver?.disconnect();
    if (typeof MutationObserver === 'undefined') return;
    // Coalesce mutation bursts via microtask — the editor often
    // emits dozens of mutations per Yjs sync; one rebuild at the
    // end is enough.
    let scheduled = false;
    mutationObserver = new MutationObserver(() => {
      if (scheduled) return;
      scheduled = true;
      queueMicrotask(() => {
        scheduled = false;
        rebuild(root);
      });
    });
    mutationObserver.observe(root, {
      childList: true,
      subtree: true,
      characterData: true,
    });
  }

  watch(
    source,
    (root) => {
      disconnectObservers();
      if (!root) {
        headings.value = [];
        activeId.value = null;
        return;
      }
      rebuild(root);
      attachMutation(root);
    },
    { immediate: true },
  );

  // `onScopeDispose` runs when the owning effect-scope ends — for a
  // component that's unmount, for a manually-managed scope it's
  // when its `stop()` is called. Pairs the cleanup with the same
  // lifetime as the effects it owns, no `onUnmounted` required.
  onScopeDispose(disconnectObservers);

  return { headings, activeId };
}
