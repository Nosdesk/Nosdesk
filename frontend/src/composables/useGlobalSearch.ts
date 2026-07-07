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

let keyboardListenerRegistered = false;

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

  watch(debouncedQuery, (newQuery) => {
    if (newQuery.trim()) {
      performSearch(newQuery);
    } else {
      resetResults();
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

  // Re-search when activeTypes changes (e.g., clearing the filter badge)
  watch(activeTypes, () => {
    if (query.value.trim()) {
      performSearch(query.value);
    }
  });

  // Re-search when the sort axis changes so the new ordering comes from
  // the index rather than a client-side re-sort of the current page (a
  // 50-result page isn't the whole match set, so re-sorting locally would
  // reorder a truncated slice and mislead).
  watch(sortOrder, () => {
    if (query.value.trim()) {
      performSearch(query.value);
    }
  });

  const openSearch = (types?: string) => {
    isOpen.value = true;
    query.value = '';
    activeTypes.value = types;
    sortOrder.value = 'relevance';
    selectedScopeIndex.value = 0;
    resetResults();
  };

  const closeSearch = () => {
    isOpen.value = false;
    query.value = '';
    activeTypes.value = undefined;
    sortOrder.value = 'relevance';
    selectedScopeIndex.value = 0;
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
    selectedIndex.value = (selectedIndex.value + 1) % flatResults.value.length;
  };

  const selectPrevious = () => {
    if (flatResults.value.length === 0) return;
    selectedIndex.value =
      selectedIndex.value <= 0
        ? flatResults.value.length - 1
        : selectedIndex.value - 1;
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

    // Prompt state, unscoped: arrows/Enter/Tab drive the scope rows.
    if (scopePromptActive.value) {
      switch (event.key) {
        case 'ArrowDown':
          event.preventDefault();
          selectedScopeIndex.value = (selectedScopeIndex.value + 1) % SCOPE_OPTIONS.length;
          return;
        case 'ArrowUp':
          event.preventDefault();
          selectedScopeIndex.value =
            selectedScopeIndex.value <= 0
              ? SCOPE_OPTIONS.length - 1
              : selectedScopeIndex.value - 1;
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
        // Token semantics: backspacing past the start of the query
        // pops the scope chip, like deleting a token in any tag input.
        if (!query.value && activeTypes.value) {
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
    openSearch,
    closeSearch,
    clearTypes,
    applyScope,
    setScopeIndex,
    setSort,
    navigateToResult,
    selectNext,
    selectPrevious,
    selectResult,
  };
}

export default useGlobalSearch;
