<script setup lang="ts">
import { ref, watch, nextTick, computed } from 'vue';
import { useFluent } from 'fluent-vue';
import { useGlobalSearch, SCOPE_OPTIONS } from '@/composables/useGlobalSearch';
import { useVisualViewport } from '@/composables/useVisualViewport';
import SearchResultGroup from './SearchResultGroup.vue';
import SearchResultItem from './SearchResultItem.vue';
import SearchSortToggle from './SearchSortToggle.vue';
import {
  ENTITY_DISPLAY_ORDER,
  ENTITY_TYPE_CONFIG,
  getEntityTypeLabel,
  type SearchEntityType,
} from '@nosdesk/core/types/search';
import Icon from '@/components/common/Icon.vue';
import type { IconName } from '@/components/common/icons';

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
  selectedScopeIndex,
  scopePromptActive,
  searchTookMs,
  totalResults,
  activeTypes,
  sortOrder,
  authorFilter,
  authorCandidates,
  selectedAuthorIndex,
  fromPromptActive,
  closeSearch,
  clearTypes,
  applyScope,
  setScopeIndex,
  setSort,
  applyAuthor,
  clearAuthor,
  setAuthorIndex,
  navigateToResult,
} = useGlobalSearch();

// While the palette is open, mirror the visual viewport into CSS vars
// so the mobile sheet can size itself to the visible area — this is
// what keeps the footer and results above the on-screen keyboard on
// iOS/WKWebView, where the layout viewport never resizes.
useVisualViewport(isOpen);

const filterLabels = computed<Record<string, string>>(() => ({
  documentation: t('search-global-filter-documentation'),
  ticket: t('search-global-filter-tickets'),
  device: t('search-global-filter-devices'),
  user: t('search-global-filter-users'),
  project: t('search-global-filter-projects'),
}));

// Chip / placeholder label for the active scope. Kinds without a
// dedicated filter key (comment, attachment — reachable via group
// headers and `in:`) fall back to their entity-type label.
const scopeLabel = (type: string) =>
  filterLabels.value[type] ?? getEntityTypeLabel(type as SearchEntityType);

const placeholder = computed(() => {
  if (activeTypes.value) {
    return t('search-global-placeholder-filtered', {
      filter: scopeLabel(activeTypes.value).toLowerCase(),
    });
  }
  return t('search-global-placeholder');
});

// Prompt-state scope rows: neutral icons (these are actions, not
// results — the per-type colour stays reserved for real hits).
const scopeRows = computed(() =>
  SCOPE_OPTIONS.map((type, index) => ({
    type,
    index,
    icon: (ENTITY_TYPE_CONFIG[type]?.icon ?? 'search') as IconName,
    label: t('search-global-scope-row', { type: scopeLabel(type) }),
  })),
);

const inputRef = ref<HTMLInputElement | null>(null);
const resultsRef = ref<HTMLDivElement | null>(null);

watch(isOpen, async (open) => {
  if (open) {
    await nextTick();
    inputRef.value?.focus();
  }
});

// Scoping from a group header or scope row must not drop focus from
// the input — the whole point is to keep typing.
const scopeAndRefocus = (type: SearchEntityType) => {
  applyScope(type);
  inputRef.value?.focus();
};

// Picking a person from the autocomplete keeps the input focused too, so
// the user can carry straight on typing the query the filter narrows.
const authorAndRefocus = (user: (typeof authorCandidates.value)[number]) => {
  applyAuthor(user);
  inputRef.value?.focus();
};

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
        class="fixed inset-0 z-overlay flex items-start justify-center sm:px-4 sm:pt-[15dvh]"
      >
        <!-- Backdrop. Subtle blur, click to dismiss. Fully covered by
             the sheet below `sm`, where the header close button takes
             over dismissal. -->
        <div
          class="absolute inset-0 bg-black/40 dark:bg-black/60 backdrop-blur-sm"
          @click="closeSearch"
        />

        <!-- Palette surface. Desktop (sm+): the floating Raycast card
             — min-h gives a stable lower bound so the frame doesn't
             shrink when state swaps, max-h is dvh-relative so it
             grows with the screen. Mobile (<sm): a full-height, top-
             anchored sheet (see scoped .search-card) whose height
             tracks the *visual* viewport, so the keyboard shrinks the
             sheet instead of covering it; the input pinned at the top
             also avoids WKWebView's scroll-to-reveal jump. -->
        <div
          class="search-card relative w-full sm:max-w-[640px] sm:min-h-[420px] sm:max-h-[80dvh] bg-surface sm:rounded-2xl shadow-2xl shadow-black/20 dark:shadow-black/40 overflow-hidden flex flex-col ring-1 ring-default"
          role="dialog"
          aria-modal="true"
          :aria-label="t('search-global-aria-label')"
        >
          <!-- Search header. Single row, no border on the input. -->
          <div class="flex items-center gap-2.5 px-4 h-12 border-b border-default flex-shrink-0">
            <Icon name="search" size="md" class="flex-shrink-0 text-tertiary" />

            <button
              v-if="activeTypes"
              @click="clearTypes"
              class="inline-flex items-center gap-1 px-2 h-6 text-[11px] font-medium rounded-md bg-accent/10 text-accent border border-accent/20 hover:bg-accent/20 transition-colors flex-shrink-0"
            >
              {{ scopeLabel(activeTypes) }}
              <Icon name="close" size="xs" />
            </button>

            <!-- Person filter chip. Composes with the scope chip; the
                 leading "from" prefix reads as the operator that set it. -->
            <button
              v-if="authorFilter"
              @click="clearAuthor"
              class="inline-flex items-center gap-1 px-2 h-6 text-[11px] font-medium rounded-md bg-brand-pink/10 text-brand-pink border border-brand-pink/20 hover:bg-brand-pink/20 transition-colors flex-shrink-0 max-w-[10rem]"
              :title="t('search-global-from-chip', { name: authorFilter.name })"
            >
              <Icon name="user" size="xs" class="flex-shrink-0" />
              <span class="truncate">{{ authorFilter.name }}</span>
              <Icon name="close" size="xs" class="flex-shrink-0" />
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

            <!-- Mobile-only close. The sheet covers the backdrop and
                 touch keyboards have no Esc, so the exit affordance
                 must live in the chrome. -->
            <button
              type="button"
              class="sm:hidden flex-shrink-0 -mr-1 p-1.5 rounded-md text-tertiary hover:text-secondary hover:bg-surface-hover/60 transition-colors"
              :aria-label="t('search-global-hint-close')"
              @click="closeSearch"
            >
              <Icon name="close" size="sm" />
            </button>
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
            class="search-results flex-1 overflow-y-auto min-h-0 overscroll-contain"
          >
            <div
              v-if="searchState === 'error'"
              class="px-4 py-6 text-center text-sm text-status-error"
            >
              {{ error }}
            </div>

            <!-- Author picker (mid `from:` token). The candidate list of
                 people replaces the results while active; picking one sets
                 the person chip and drops the token. -->
            <div v-else-if="fromPromptActive" class="py-1 px-1">
              <div class="px-2 pt-2 pb-1">
                <span class="text-[10px] font-semibold uppercase tracking-wider text-tertiary">
                  {{ t('search-global-from-heading') }}
                </span>
              </div>
              <button
                v-for="(user, index) in authorCandidates"
                :key="user.id"
                type="button"
                tabindex="-1"
                :data-author-selected="index === selectedAuthorIndex"
                :class="[
                  'w-full px-2 py-1.5 flex items-center gap-2.5 text-left rounded-md transition-colors focus:outline-none',
                  index === selectedAuthorIndex ? 'bg-accent/10' : 'hover:bg-surface-hover/60',
                ]"
                @mouseenter="setAuthorIndex(index)"
                @click="authorAndRefocus(user)"
              >
                <span class="flex-shrink-0 inline-flex w-7 h-7 rounded-md items-center justify-center bg-[rgba(255,102,179,0.15)] text-brand-pink">
                  <Icon name="user" size="xs" />
                </span>
                <span class="flex-1 min-w-0">
                  <span class="block text-sm text-primary font-medium truncate">{{ user.title }}</span>
                  <span v-if="user.preview" class="block text-[11px] text-tertiary truncate">{{ user.preview }}</span>
                </span>
                <kbd
                  v-if="index === selectedAuthorIndex"
                  class="hidden sm:inline-flex items-center justify-center min-w-[1.25rem] h-4 px-1 rounded bg-surface border border-default text-[9px] font-medium text-secondary"
                >⏎</kbd>
              </button>
              <!-- Nothing typed yet, or no matches. -->
              <div
                v-if="authorCandidates.length === 0"
                class="px-3 py-8 text-center text-xs text-tertiary"
              >
                {{ t('search-global-from-hint') }}
              </div>
            </div>

            <!-- Prompt, unscoped: the scope rows. Tab/Enter (or tap)
                 narrows the search before typing — the palette's one
                 filtering affordance, presented where a filter
                 decision is actually made: before the query. -->
            <div v-else-if="scopePromptActive" class="py-1 px-1">
              <div class="px-2 pt-2 pb-1">
                <span class="text-[10px] font-semibold uppercase tracking-wider text-tertiary">
                  {{ t('search-global-scope-heading') }}
                </span>
              </div>
              <button
                v-for="row in scopeRows"
                :key="row.type"
                type="button"
                tabindex="-1"
                :data-scope-selected="row.index === selectedScopeIndex"
                :class="[
                  'w-full px-2 py-1.5 flex items-center gap-2.5 text-left rounded-md transition-colors focus:outline-none',
                  row.index === selectedScopeIndex ? 'bg-accent/10' : 'hover:bg-surface-hover/60',
                ]"
                @mouseenter="setScopeIndex(row.index)"
                @click="scopeAndRefocus(row.type)"
              >
                <span class="flex-shrink-0 inline-flex w-7 h-7 rounded-md items-center justify-center bg-surface-alt text-tertiary">
                  <Icon :name="row.icon" size="xs" />
                </span>
                <span class="flex-1 text-sm text-primary font-medium truncate">
                  {{ row.label }}
                </span>
                <kbd
                  v-if="row.index === selectedScopeIndex"
                  class="hidden sm:inline-flex items-center justify-center min-w-[1.25rem] h-4 px-1 rounded bg-surface border border-default text-[9px] font-medium text-secondary"
                >⇥</kbd>
              </button>
            </div>

            <!-- Prompt, scoped: the chip already narrates the scope;
                 plain copy invites the query. -->
            <div
              v-else-if="searchState === 'prompt'"
              class="px-4 py-12 text-center"
            >
              <p class="text-sm text-secondary font-medium">{{ t('search-global-prompt-title') }}</p>
              <p class="text-xs text-tertiary mt-1">
                {{ t('search-global-prompt-subtitle') }}
              </p>
            </div>

            <!-- Results. Best-match keeps the per-type grouped view (with
                 scope-able headers); Newest collapses to one flat
                 chronological list, since grouping by kind would fight the
                 recency order. A slim sort toolbar rides above the list on
                 mobile (the desktop footer carries the same toggle). -->
            <div v-else-if="searchState === 'results'">
              <div
                class="sm:hidden flex items-center justify-end px-3 h-9 border-b border-default"
              >
                <SearchSortToggle
                  :model-value="sortOrder"
                  @update:model-value="setSort"
                />
              </div>

              <div v-if="sortOrder === 'updated'" class="py-1 px-1">
                <SearchResultItem
                  v-for="result in flatResults"
                  :key="result.id"
                  :result="result"
                  :is-selected="result.id === selectedId"
                  @select="navigateToResult"
                />
              </div>

              <template v-else>
                <SearchResultGroup
                  v-for="group in resultGroups"
                  :key="group.type"
                  :type="group.type"
                  :results="groupedResults[group.key]"
                  :selected-id="selectedId"
                  @select="navigateToResult"
                  @scope="scopeAndRefocus"
                />
              </template>
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

          <!-- Persistent footer, desktop only. Keyboard hints on the
               left, result stats on the right; always rendered there
               so the bottom edge doesn't jump as states swap. On a
               phone every one of those is dead weight — no keys to
               hint, stats aren't worth a bar — so the results list
               takes the height instead. -->
          <div
            class="hidden sm:flex items-center justify-between gap-3 px-3 h-9 border-t border-default bg-surface-alt/50 text-[11px] text-tertiary flex-shrink-0"
          >
            <div class="hidden sm:flex items-center gap-3">
              <span class="inline-flex items-center gap-1">
                <kbd class="inline-flex items-center justify-center w-4 h-4 rounded bg-surface border border-default text-[9px] font-medium text-secondary">↑</kbd>
                <kbd class="inline-flex items-center justify-center w-4 h-4 rounded bg-surface border border-default text-[9px] font-medium text-secondary">↓</kbd>
                <span>{{ t('search-global-hint-navigate') }}</span>
              </span>
              <span v-if="scopePromptActive" class="inline-flex items-center gap-1">
                <kbd class="inline-flex items-center justify-center min-w-[1rem] h-4 px-1 rounded bg-surface border border-default text-[9px] font-medium text-secondary">⇥</kbd>
                <span>{{ t('search-global-hint-scope') }}</span>
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
            <div v-if="searchState === 'results'" class="flex items-center gap-3 ml-auto">
              <SearchSortToggle
                :model-value="sortOrder"
                @update:model-value="setSort"
              />
              <span class="tabular-nums">
                {{ t('search-global-results-count', { count: totalResults }) }}
                <span class="text-tertiary/60">·</span>
                {{ t('search-global-results-took', { ms: searchTookMs }) }}
              </span>
            </div>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
/* Mobile: full-height, top-anchored sheet. Height tracks the visual
   viewport (set by useVisualViewport while open) so the on-screen
   keyboard shrinks the sheet instead of covering its lower half; the
   dvh fallback covers browsers without the API and the moments before
   the first viewport event lands. Safe-area padding keeps the input
   row out of the status bar / notch (viewport-fit=cover). */
@media (max-width: 639.98px) {
  .search-card {
    height: var(--visual-viewport-height, 100dvh);
    max-height: none;
    border-radius: 0;
    padding-top: env(safe-area-inset-top);
  }

  /* No footer on mobile — the results list runs to the sheet's
     bottom edge, so it carries the home-indicator clearance itself
     (when the keyboard is up the sheet already ends above it). */
  .search-results {
    padding-bottom: env(safe-area-inset-bottom);
  }
}

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
