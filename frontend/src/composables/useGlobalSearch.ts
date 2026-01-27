import { ref, computed, watch, onMounted, onUnmounted } from 'vue';
import { useRouter } from 'vue-router';
import { searchService } from '@/services/searchService';
import type { SearchResult, SearchResponse, GroupedSearchResults } from '@/types/search';
import { groupResultsByType, emptyGroupedResults, ENTITY_DISPLAY_ORDER, ENTITY_TYPE_CONFIG } from '@/types/search';

// Debounce helper
function debounce<T extends (...args: unknown[]) => unknown>(
  fn: T,
  delay: number
): (...args: Parameters<T>) => void {
  let timeoutId: ReturnType<typeof setTimeout> | null = null;
  return (...args: Parameters<T>) => {
    if (timeoutId) clearTimeout(timeoutId);
    timeoutId = setTimeout(() => fn(...args), delay);
  };
}

// ============================================
// SHARED STATE (singleton pattern)
// ============================================
const isOpen = ref(false);
const query = ref('');
const results = ref<SearchResult[]>([]);
const groupedResults = ref<GroupedSearchResults>(emptyGroupedResults());
const isLoading = ref(false);
const error = ref<string | null>(null);
const selectedIndex = ref(-1);
const searchTookMs = ref(0);
const totalResults = ref(0);

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
      });

      results.value = response.results;
      groupedResults.value = groupResultsByType(response.results);
      totalResults.value = response.total;
      searchTookMs.value = response.took_ms;
      selectedIndex.value = response.results.length > 0 ? 0 : -1;
    } catch (err) {
      console.error('Search error:', err);
      error.value = 'Search failed. Please try again.';
      results.value = [];
      groupedResults.value = emptyGroupedResults();
    } finally {
      isLoading.value = false;
    }
  };

  const debouncedSearch = debounce(performSearch, 200);

  watch(query, (newQuery) => {
    if (newQuery.trim()) {
      debouncedSearch(newQuery);
    } else {
      resetResults();
    }
  });

  const openSearch = () => {
    isOpen.value = true;
    query.value = '';
    resetResults();
  };

  const closeSearch = () => {
    isOpen.value = false;
    query.value = '';
    resetResults();
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

  return {
    isOpen,
    query,
    results,
    groupedResults,
    flatResults,
    isLoading,
    error,
    selectedIndex,
    searchTookMs,
    totalResults,
    openSearch,
    closeSearch,
    navigateToResult,
    selectNext,
    selectPrevious,
    selectResult,
  };
}

export default useGlobalSearch;
