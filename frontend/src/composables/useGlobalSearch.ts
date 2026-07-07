import { ref, computed, watch, onMounted, onUnmounted } from 'vue';
import { useRouter } from 'vue-router';
import { useDebouncedRef } from '@/composables/useDebouncedRef';
import { searchService } from '@nosdesk/core/services/searchService';
import type { SearchResult, SearchResponse, GroupedSearchResults, SearchEntityType, SearchSortOrder } from '@nosdesk/core/types/search';
import { groupResultsByType, emptyGroupedResults, ENTITY_DISPLAY_ORDER, ENTITY_TYPE_CONFIG } from '@nosdesk/core/types/search';
import { translate } from '@/i18n';

/** Mutually exclusive states for the search surface. */
export type SearchState = 'prompt' | 'searching' | 'results' | 'empty' | 'error';

/**
 * The kinds offered as scope rows in the prompt state, in display
 * order. Comments and attachments are reachable as scopes too (group
 * headers, `in:` operator) but don't earn a prompt row — they're
 * rarely a *starting* intent.
 */
export const SCOPE_OPTIONS: SearchEntityType[] = [
  'ticket',
  'documentation',
  'device',
  'user',
  'project',
];

/**
 * Typed-operator vocabulary: `in:<alias>` scopes the search. Aliases
 * are deliberately generous (docs, assets, people…) — the operator is
 * a power-user shortcut, so guessing wrong should still land.
 */
const SCOPE_ALIASES: Record<string, SearchEntityType> = {
  ticket: 'ticket',
  tickets: 'ticket',
  doc: 'documentation',
  docs: 'documentation',
  documentation: 'documentation',
  device: 'device',
  devices: 'device',
  asset: 'device',
  assets: 'device',
  user: 'user',
  users: 'user',
  people: 'user',
  person: 'user',
  project: 'project',
  projects: 'project',
  comment: 'comment',
  comments: 'comment',
  attachment: 'attachment',
  attachments: 'attachment',
};

/**
 * Resolve a completed `in:` token to a scope, or null. Exact aliases
 * win; otherwise a prefix resolves when EVERY alias it prefixes
 * agrees on the same kind (`doc ` → doc/docs/documentation all mean
 * documentation), so abbreviations land without a lookup table of
 * their own. Tokens that prefix nothing stay plain query text.
 */
function resolveScopeToken(token: string): SearchEntityType | null {
  const lower = token.toLowerCase();
  if (SCOPE_ALIASES[lower]) return SCOPE_ALIASES[lower];
  const candidates = new Set(
    Object.keys(SCOPE_ALIASES)
      .filter((a) => a.startsWith(lower))
      .map((a) => SCOPE_ALIASES[a]),
  );
  return candidates.size === 1 ? [...candidates][0] : null;
}

// ============================================
// SHARED STATE (singleton pattern)
// ============================================
const isOpen = ref(false);
const query = ref('');
// `query` updates synchronously so the input feels snappy as the
// user types; `debouncedQuery` is what actually fires searches.
//
// 150ms sits inside Algolia's "preferred" 200ms-or-faster zone
// (above 300ms degrades the typed-as-you-search feel), tight
// enough that progressive narrowing is visible during natural
// pauses while still cutting roughly 80% of API calls vs firing
// per keystroke.
//
// The `leading` predicate skips the delay at either edge of the
// empty/non-empty boundary: typing the first character from a
// fresh modal fires the search instantly (no first-keystroke
// dead zone) and clearing the input flushes immediately back to
// the prompt state. Mid-query edits — the bulk of typing — still
// debounce normally so we don't thrash the index on every char.
const debouncedQuery = useDebouncedRef(query, 150, {
  leading: (prev, next) => !prev.trim() || !next.trim(),
});
const results = ref<SearchResult[]>([]);
const groupedResults = ref<GroupedSearchResults>(emptyGroupedResults());
const isLoading = ref(false);
const error = ref<string | null>(null);
const selectedIndex = ref(-1);
const searchTookMs = ref(0);
const totalResults = ref(0);
const activeTypes = ref<string | undefined>(undefined);
// Result ordering. 'relevance' (BM25) is the default; 'updated' asks the
// backend for newest-first. In 'updated' mode the surface also drops the
// per-type grouping in favour of one flat chronological list (see
// flatResults / the modal) — grouping by kind would otherwise override
// the recency order the user just asked for.
const sortOrder = ref<SearchSortOrder>('relevance');
// Highlighted row among the prompt-state scope rows (unscoped, empty
// query). Kept separate from `selectedIndex` so entering/leaving the
// prompt never clobbers result selection logic.
const selectedScopeIndex = ref(0);

// `from:` person filter. When set, searches carry `author=<uuid>` and a
// person chip shows in the header alongside the scope chip. Resolved
// through an inline autocomplete (authorCandidates) that reuses the user
// search — no dedicated endpoint. One author at a time, by design.
const authorFilter = ref<{ uuid: string; name: string } | null>(null);
const authorCandidates = ref<SearchResult[]>([]);
const selectedAuthorIndex = ref(0);

/**
 * Matches an in-progress `from:<partial>` token at the very end of the
 * query — the token the user is actively typing. While this matches (and
 * no author is set yet) the palette is in author-picker mode: the main
 * search is suppressed and the candidate list drives the surface.
 */
const FROM_TOKEN_RE = /(^|\s)from:(\S*)$/i;

let keyboardListenerRegistered = false;

/** Step an index by `delta` within `[0, len)`, wrapping at both ends. */
function wrapIndex(index: number, len: number, delta: number): number {
  if (len === 0) return 0;
  return (index + delta + len) % len;
}

/** Reset search state to empty */
function resetResults() {
  results.value = [];
  groupedResults.value = emptyGroupedResults();
  totalResults.value = 0;
  selectedIndex.value = -1;
  error.value = null;
}

/**
 * Composable for global search functionality.
 * Uses shared state so all components see the same search state.
 */
export function useGlobalSearch() {
  const router = useRouter();

  // The flat list backing keyboard navigation and selection. It must
  // match what's on screen top-to-bottom, which differs by sort:
  //  - relevance: the grouped view, so flatten groups in display order.
  //  - updated: one flat chronological list, so use the backend order
  //    (already newest-first) verbatim.
  const flatResults = computed(() => {
    if (sortOrder.value === 'updated') {
      return results.value;
    }
    const flat: SearchResult[] = [];
    for (const type of ENTITY_DISPLAY_ORDER) {
      const key = ENTITY_TYPE_CONFIG[type].key;
      flat.push(...groupedResults.value[key]);
    }
    return flat;
  });

  const performSearch = async (searchQuery: string) => {
    if (!searchQuery.trim()) {
      resetResults();
      return;
    }

    isLoading.value = true;
    error.value = null;

    try {
      const response: SearchResponse = await searchService.search({
        q: searchQuery,
        limit: 50,
        types: activeTypes.value,
        sort: sortOrder.value,
        author: authorFilter.value?.uuid,
      });

      results.value = response.results;
      groupedResults.value = groupResultsByType(response.results);
      totalResults.value = response.total;
      searchTookMs.value = response.took_ms;
      selectedIndex.value = response.results.length > 0 ? 0 : -1;
    } catch (err) {
      console.error('Search error:', err);
      error.value = translate('search-failed', undefined, 'Search failed. Please try again.');
      results.value = [];
      groupedResults.value = emptyGroupedResults();
    } finally {
      isLoading.value = false;
    }
  };

  // Whether the palette is mid `from:` token (author-picker mode). While
  // active, the candidate list owns the surface and the main search is
  // suppressed so the raw "from:sar" text never hits the index.
  const fromPromptActive = computed(
    () => isOpen.value && !authorFilter.value && FROM_TOKEN_RE.test(query.value),
  );

  watch(debouncedQuery, (newQuery) => {
    // The author picker owns the surface; don't search the raw token text.
    if (fromPromptActive.value) return;
    if (newQuery.trim()) {
      performSearch(newQuery);
    } else {
      resetResults();
    }
  });

  // Author autocomplete: while typing `from:<partial>`, look up matching
  // people (reusing the user search) and offer them as candidates. Guarded
  // against stale responses — a slower fetch for an earlier partial must
  // not overwrite a newer one.
  watch(query, async (raw) => {
    const match = raw.match(FROM_TOKEN_RE);
    if (!match || authorFilter.value) {
      authorCandidates.value = [];
      return;
    }
    const partial = match[2].trim();
    if (!partial) {
      // `from:` with nothing typed yet — wait for a character rather than
      // dumping an arbitrary user list.
      authorCandidates.value = [];
      selectedAuthorIndex.value = 0;
      return;
    }
    try {
      const resp = await searchService.search({ q: partial, types: 'user', limit: 6 });
      if (query.value !== raw) return; // a newer keystroke superseded this
      authorCandidates.value = resp.results;
      selectedAuthorIndex.value = 0;
    } catch {
      authorCandidates.value = [];
    }
  });

  // Typed-operator tokenisation: `in:docs printer` → scope chip
  // "Documentation" + query "printer". Runs on the raw (undebounced)
  // query so the chip appears the moment the token completes; the
  // stripped query then flows through the normal debounce. A token
  // only converts once it's *finished* — followed by a space — never
  // mid-word: converting eagerly at the first unambiguous prefix
  // strands the user's remaining keystrokes in the query ("in:t" →
  // chip, then "ickets" as search text).
  watch(query, (raw) => {
    const match = raw.match(/(^|\s)in:([a-zA-Z]+)\s/);
    if (!match) return;
    const scope = resolveScopeToken(match[2]);
    if (!scope) return;
    activeTypes.value = scope;
    query.value = raw.replace(match[0], match[1]).replace(/\s{2,}/g, ' ').trimStart();
  });

  // Re-run the search whenever a filter or the sort axis changes — scope
  // chip, sort toggle, or person chip. Each narrows/re-orders an existing
  // query, so we go back to the index rather than re-sorting the current
  // (truncated) page. With no query text there's nothing to run, so we fall
  // back to the prompt; any chips stay visible, ready for a query.
  watch([activeTypes, sortOrder, authorFilter], () => {
    if (query.value.trim()) {
      performSearch(query.value);
    } else {
      resetResults();
    }
  });

  const resetAuthor = () => {
    authorFilter.value = null;
    authorCandidates.value = [];
    selectedAuthorIndex.value = 0;
  };

  const openSearch = (types?: string) => {
    isOpen.value = true;
    query.value = '';
    activeTypes.value = types;
    sortOrder.value = 'relevance';
    selectedScopeIndex.value = 0;
    resetAuthor();
    resetResults();
  };

  const closeSearch = () => {
    isOpen.value = false;
    query.value = '';
    activeTypes.value = undefined;
    sortOrder.value = 'relevance';
    selectedScopeIndex.value = 0;
    resetAuthor();
    resetResults();
  };

  const clearTypes = () => {
    activeTypes.value = undefined;
    selectedScopeIndex.value = 0;
  };

  /** Scope the palette to one kind (prompt rows, group headers, `in:`). */
  const applyScope = (type: SearchEntityType) => {
    activeTypes.value = type;
  };

  /** Mouse-hover parity with arrow keys on the prompt scope rows. */
  const setScopeIndex = (index: number) => {
    selectedScopeIndex.value = index;
  };

  /** Switch the result ordering (footer / mobile toolbar toggle). */
  const setSort = (order: SearchSortOrder) => {
    sortOrder.value = order;
  };

  /**
   * Resolve a `from:` candidate to the person chip: set the filter, drop
   * the candidate list, and strip the half-typed `from:<partial>` token
   * from the query (leaving any real search text behind it intact).
   */
  const applyAuthor = (user: SearchResult) => {
    const uuid = user.url.replace('/users/', '');
    if (!uuid) return;
    authorFilter.value = { uuid, name: user.title };
    authorCandidates.value = [];
    // The `from:` token is always the trailing token, so trimming both ends
    // leaves just the real query text (empty if `from:` was all there was).
    query.value = query.value
      .replace(FROM_TOKEN_RE, '$1')
      .replace(/\s{2,}/g, ' ')
      .trim();
  };

  /** Drop the person filter (chip X, or Backspace on an empty query). */
  const clearAuthor = () => {
    resetAuthor();
  };

  /** Mouse-hover parity with arrow keys on the author candidates. */
  const setAuthorIndex = (index: number) => {
    selectedAuthorIndex.value = index;
  };

  // The prompt-state scope rows own the keyboard while the palette is
  // unscoped with an empty query — the same arrows/Enter vocabulary as
  // results, so the hand never changes shape.
  const scopePromptActive = computed(
    () => isOpen.value && !query.value.trim() && !activeTypes.value,
  );

  const navigateToResult = (result: SearchResult) => {
    closeSearch();
    router.push(result.url);
  };

  // Keyboard navigation
  const selectNext = () => {
    if (flatResults.value.length === 0) return;
    selectedIndex.value = wrapIndex(selectedIndex.value, flatResults.value.length, 1);
  };

  const selectPrevious = () => {
    if (flatResults.value.length === 0) return;
    selectedIndex.value = wrapIndex(selectedIndex.value, flatResults.value.length, -1);
  };

  const selectResult = () => {
    if (selectedIndex.value >= 0 && selectedIndex.value < flatResults.value.length) {
      navigateToResult(flatResults.value[selectedIndex.value]);
    }
  };

  const handleKeyDown = (event: KeyboardEvent) => {
    if ((event.metaKey || event.ctrlKey) && event.key === 'k') {
      event.preventDefault();
      isOpen.value ? closeSearch() : openSearch();
      return;
    }

    if (!isOpen.value) return;

    // Author picker (mid `from:` token) owns the keyboard while it has
    // candidates — same arrows/Enter/Tab vocabulary as the scope rows.
    if (fromPromptActive.value && authorCandidates.value.length > 0) {
      switch (event.key) {
        case 'ArrowDown':
          event.preventDefault();
          selectedAuthorIndex.value = wrapIndex(selectedAuthorIndex.value, authorCandidates.value.length, 1);
          return;
        case 'ArrowUp':
          event.preventDefault();
          selectedAuthorIndex.value = wrapIndex(selectedAuthorIndex.value, authorCandidates.value.length, -1);
          return;
        case 'Tab':
        case 'Enter':
          event.preventDefault();
          applyAuthor(authorCandidates.value[selectedAuthorIndex.value]);
          return;
      }
    }

    // Prompt state, unscoped: arrows/Enter/Tab drive the scope rows.
    if (scopePromptActive.value) {
      switch (event.key) {
        case 'ArrowDown':
          event.preventDefault();
          selectedScopeIndex.value = wrapIndex(selectedScopeIndex.value, SCOPE_OPTIONS.length, 1);
          return;
        case 'ArrowUp':
          event.preventDefault();
          selectedScopeIndex.value = wrapIndex(selectedScopeIndex.value, SCOPE_OPTIONS.length, -1);
          return;
        case 'Tab':
        case 'Enter':
          event.preventDefault();
          applyScope(SCOPE_OPTIONS[selectedScopeIndex.value]);
          return;
      }
    }

    switch (event.key) {
      case 'Escape':
        event.preventDefault();
        closeSearch();
        break;
      case 'ArrowDown':
        event.preventDefault();
        selectNext();
        break;
      case 'ArrowUp':
        event.preventDefault();
        selectPrevious();
        break;
      case 'Enter':
        event.preventDefault();
        selectResult();
        break;
      case 'Backspace':
        // Token semantics: backspacing past the start of the query pops a
        // chip, like deleting a token in any tag input. Pop the person chip
        // first (it's the more recently added, rightmost token), then scope.
        if (!query.value && authorFilter.value) {
          event.preventDefault();
          clearAuthor();
        } else if (!query.value && activeTypes.value) {
          event.preventDefault();
          clearTypes();
        }
        break;
    }
  };

  onMounted(() => {
    if (!keyboardListenerRegistered) {
      window.addEventListener('keydown', handleKeyDown);
      keyboardListenerRegistered = true;
    }
  });

  onUnmounted(() => {
    // Listener persists for app lifetime (shared state)
  });

  // Single derived state for the search surface. Five mutually
  // exclusive values; consumers branch on the name instead of
  // juggling several flags. Order matters: `results` wins over
  // `searching` so a fresh search refresh keeps the previous
  // hits on screen (stale-while-revalidate), only blanking the
  // body when there's nothing to show. That's how Raycast feels
  // snappy without using transitions — the surface never goes
  // empty when it doesn't have to.
  const searchState = computed<SearchState>(() => {
    if (error.value) return 'error';
    if (!query.value.trim()) return 'prompt';
    if (flatResults.value.length > 0) return 'results';
    if (isLoading.value || query.value.trim() !== debouncedQuery.value.trim()) {
      return 'searching';
    }
    return 'empty';
  });

  return {
    isOpen,
    query,
    groupedResults,
    flatResults,
    searchState,
    error,
    selectedIndex,
    selectedScopeIndex: computed(() => selectedScopeIndex.value),
    scopePromptActive,
    searchTookMs,
    totalResults,
    activeTypes: computed(() => activeTypes.value),
    sortOrder: computed(() => sortOrder.value),
    authorFilter: computed(() => authorFilter.value),
    authorCandidates: computed(() => authorCandidates.value),
    selectedAuthorIndex: computed(() => selectedAuthorIndex.value),
    fromPromptActive,
    openSearch,
    closeSearch,
    clearTypes,
    applyScope,
    setScopeIndex,
    setSort,
    applyAuthor,
    clearAuthor,
    setAuthorIndex,
    navigateToResult,
    selectNext,
    selectPrevious,
    selectResult,
  };
}

export default useGlobalSearch;
