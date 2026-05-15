<script setup lang="ts">
import { ref, watch, nextTick, computed } from 'vue';
import { useFluent } from 'fluent-vue';
import { useGlobalSearch } from '@/composables/useGlobalSearch';
import SearchResultGroup from './SearchResultGroup.vue';
import { ENTITY_DISPLAY_ORDER, ENTITY_TYPE_CONFIG } from '@/types/search';
import Icon from '@/components/common/Icon.vue';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const {
  isOpen,
  query,
  groupedResults,
  flatResults,
  searchState,
  error,
  selectedIndex,
  searchTookMs,
  totalResults,
  activeTypes,
  closeSearch,
  clearTypes,
  navigateToResult,
} = useGlobalSearch();

const filterLabels = computed<Record<string, string>>(() => ({
  documentation: t('search-global-filter-documentation'),
  ticket: t('search-global-filter-tickets'),
  device: t('search-global-filter-devices'),
  user: t('search-global-filter-users'),
}));

const placeholder = computed(() => {
  if (activeTypes.value) {
    const label = filterLabels.value[activeTypes.value] || activeTypes.value;
    return t('search-global-placeholder-filtered', { filter: label.toLowerCase() });
  }
  return t('search-global-placeholder');
});

const inputRef = ref<HTMLInputElement | null>(null);
const resultsRef = ref<HTMLDivElement | null>(null);

watch(isOpen, async (open) => {
  if (open) {
    await nextTick();
    inputRef.value?.focus();
  }
});

watch(selectedIndex, () => {
  if (selectedIndex.value >= 0 && resultsRef.value) {
    const selectedElement = resultsRef.value.querySelector('[data-selected="true"]');
    selectedElement?.scrollIntoView({ block: 'nearest' });
  }
});

const selectedId = computed(() => {
  if (selectedIndex.value >= 0 && selectedIndex.value < flatResults.value.length) {
    return flatResults.value[selectedIndex.value].id;
  }
  return null;
});

const resultGroups = ENTITY_DISPLAY_ORDER.map(type => ({
  type,
  key: ENTITY_TYPE_CONFIG[type].key,
}));
</script>

<template>
  <Teleport to="body">
    <Transition name="search-modal" appear>
      <div
        v-if="isOpen"
        class="fixed inset-0 z-overlay flex items-start justify-center px-4 pt-[12vh] sm:pt-[15vh]"
      >
        <!-- Backdrop. Subtle blur, click to dismiss. -->
        <div
          class="absolute inset-0 bg-black/40 dark:bg-black/60 backdrop-blur-sm"
          @click="closeSearch"
        />

        <!-- Modal card. min-h gives a stable lower bound so the
             frame doesn't shrink when state swaps (which the eye
             otherwise reads as a flash); max-h is viewport-
             relative so the modal grows with the user's screen
             instead of clamping at a hard pixel cap. The body's
             flex-1 absorbs any difference. -->
        <div
          class="relative w-full max-w-[640px] min-h-[420px] max-h-[80vh] bg-surface rounded-2xl shadow-2xl shadow-black/20 dark:shadow-black/40 overflow-hidden flex flex-col ring-1 ring-default"
          role="dialog"
          aria-modal="true"
          :aria-label="t('search-global-aria-label')"
        >
          <!-- Search header. Single row, no border on the input. -->
          <div class="flex items-center gap-2.5 px-4 h-12 border-b border-default">
            <Icon name="search" size="md" class="flex-shrink-0 text-tertiary" />

            <button
              v-if="activeTypes"
              @click="clearTypes"
              class="inline-flex items-center gap-1 px-2 h-6 text-[11px] font-medium rounded-md bg-accent/10 text-accent border border-accent/20 hover:bg-accent/20 transition-colors flex-shrink-0"
            >
              {{ filterLabels[activeTypes] || activeTypes }}
              <Icon name="close" size="xs" />
            </button>

            <input
              ref="inputRef"
              v-model="query"
              type="text"
              :placeholder="placeholder"
              class="flex-1 bg-transparent text-primary placeholder-tertiary/60 outline-none text-sm font-medium"
              autocomplete="off"
              spellcheck="false"
            />
          </div>

          <!-- Results region. Holds all body states; `min-h-0`
               + flex-1 lets the inner scroll container size to
               the modal's max height without overflowing it.
               State swaps are instant — no fade transition. With
               the debounced query, only one state change happens
               per search cycle, and it lands fast enough that
               cross-fading just adds visible "in-between" latency. -->
          <div
            ref="resultsRef"
            class="flex-1 overflow-y-auto min-h-0 overscroll-contain"
          >
            <div
              v-if="searchState === 'error'"
              class="px-4 py-6 text-center text-sm text-status-error"
            >
              {{ error }}
            </div>

            <div
              v-else-if="searchState === 'prompt'"
              class="px-4 py-12 text-center"
            >
              <p class="text-sm text-secondary font-medium">{{ t('search-global-prompt-title') }}</p>
              <p class="text-xs text-tertiary mt-1">
                {{ t('search-global-prompt-subtitle') }}
              </p>
            </div>

            <div v-else-if="searchState === 'results'">
              <SearchResultGroup
                v-for="group in resultGroups"
                :key="group.type"
                :type="group.type"
                :results="groupedResults[group.key]"
                :selected-id="selectedId"
                @select="navigateToResult"
              />
            </div>

            <!-- `searching`: the input has changed but no fresh
                 results have landed yet (and there are no stale
                 ones to keep on screen). Body stays visually
                 empty so the surface doesn't flash "no results"
                 mid-type. -->
            <div
              v-else-if="searchState === 'searching'"
              aria-hidden="true"
              class="flex-1"
            />

            <div
              v-else
              class="px-4 py-12 text-center"
            >
              <p class="text-sm text-secondary font-medium">
                {{ t('search-global-empty-prefix') }}"<span class="text-primary">{{ query }}</span>"
              </p>
              <p class="text-xs text-tertiary mt-1">
                {{ t('search-global-empty-hint') }}
              </p>
            </div>
          </div>

          <!-- Persistent footer. Keyboard hints on the left,
               result stats on the right. Always rendered so the
               modal's bottom edge doesn't jump as states swap. -->
          <div
            class="flex items-center justify-between gap-3 px-3 h-9 border-t border-default bg-surface-alt/50 text-[11px] text-tertiary"
          >
            <div class="flex items-center gap-3">
              <span class="inline-flex items-center gap-1">
                <kbd class="inline-flex items-center justify-center w-4 h-4 rounded bg-surface border border-default text-[9px] font-medium text-secondary">↑</kbd>
                <kbd class="inline-flex items-center justify-center w-4 h-4 rounded bg-surface border border-default text-[9px] font-medium text-secondary">↓</kbd>
                <span>{{ t('search-global-hint-navigate') }}</span>
              </span>
              <span v-if="searchState === 'results'" class="inline-flex items-center gap-1">
                <kbd class="inline-flex items-center justify-center min-w-[1rem] h-4 px-1 rounded bg-surface border border-default text-[9px] font-medium text-secondary">↵</kbd>
                <span>{{ t('search-global-hint-open') }}</span>
              </span>
              <span class="inline-flex items-center gap-1">
                <kbd class="inline-flex items-center justify-center min-w-[1.5rem] h-4 px-1 rounded bg-surface border border-default text-[9px] font-medium text-secondary">esc</kbd>
                <span>{{ t('search-global-hint-close') }}</span>
              </span>
            </div>
            <div v-if="searchState === 'results'" class="tabular-nums">
              {{ t('search-global-results-count', { count: totalResults }) }}
              <span class="text-tertiary/60">·</span>
              {{ t('search-global-results-took', { ms: searchTookMs }) }}
            </div>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.search-modal-enter-active,
.search-modal-leave-active {
  transition: opacity 0.15s ease;
}

.search-modal-enter-active > div:last-child,
.search-modal-leave-active > div:last-child {
  transition: transform 0.18s cubic-bezier(0.16, 1, 0.3, 1), opacity 0.15s ease;
}

.search-modal-enter-from,
.search-modal-leave-to {
  opacity: 0;
}

.search-modal-enter-from > div:last-child {
  opacity: 0;
  transform: scale(0.97) translateY(-6px);
}

.search-modal-leave-to > div:last-child {
  opacity: 0;
  transform: scale(0.98);
}


/* Subtle scrollbar on the results area. */
.overflow-y-auto {
  scrollbar-width: thin;
  scrollbar-color: var(--color-default) transparent;
}

.overflow-y-auto::-webkit-scrollbar {
  width: 6px;
}

.overflow-y-auto::-webkit-scrollbar-track {
  background: transparent;
}

.overflow-y-auto::-webkit-scrollbar-thumb {
  background-color: var(--color-default);
  border-radius: 3px;
}

.overflow-y-auto::-webkit-scrollbar-thumb:hover {
  background-color: var(--color-strong);
}
</style>
