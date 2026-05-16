import { ref, computed, watch, onMounted, onUnmounted } from 'vue';
import { useRouter } from 'vue-router';
import { useDebouncedRef } from '@/composables/useDebouncedRef';
import { searchService } from '@/services/searchService';
import type { SearchResult, SearchResponse, GroupedSearchResults } from '@/types/search';
import { groupResultsByType, emptyGroupedResults, ENTITY_DISPLAY_ORDER, ENTITY_TYPE_CONFIG } from '@/types/search';
import { translate } from '@/i18n';

/** Mutually exclusive states for the search surface. */
export type SearchState = 'prompt' | 'searching' | 'results' | 'empty' | 'error';

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

  // Flatten grouped results for keyboard navigation (in display order)
  const flatResults = computed(() => {
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

  // Re-search when activeTypes changes (e.g., clearing the filter badge)
  watch(activeTypes, () => {
    if (query.value.trim()) {
      performSearch(query.value);
    }
  });

  const openSearch = (types?: string) => {
    isOpen.value = true;
    query.value = '';
    activeTypes.value = types;
    resetResults();
  };

  const closeSearch = () => {
    isOpen.value = false;
    query.value = '';
    activeTypes.value = undefined;
    resetResults();
  };

  const clearTypes = () => {
    activeTypes.value = undefined;
  };

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
    searchTookMs,
    totalResults,
    activeTypes: computed(() => activeTypes.value),
    openSearch,
    closeSearch,
    clearTypes,
    navigateToResult,
    selectNext,
    selectPrevious,
    selectResult,
  };
}

export default useGlobalSearch;
