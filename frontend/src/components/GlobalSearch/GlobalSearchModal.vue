<script setup lang="ts">
import { ref, watch, nextTick, computed } from 'vue';
import { useGlobalSearch } from '@/composables/useGlobalSearch';
import SearchResultGroup from './SearchResultGroup.vue';
import { ENTITY_DISPLAY_ORDER, ENTITY_TYPE_CONFIG } from '@/types/search';

const {
  isOpen,
  query,
  groupedResults,
  flatResults,
  isLoading,
  error,
  selectedIndex,
  searchTookMs,
  totalResults,
  closeSearch,
  navigateToResult,
} = useGlobalSearch();

const inputRef = ref<HTMLInputElement | null>(null);
const resultsRef = ref<HTMLDivElement | null>(null);

// Focus input when modal opens
watch(isOpen, async (open) => {
  if (open) {
    await nextTick();
    inputRef.value?.focus();
  }
});

// Scroll selected item into view
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

const hasResults = computed(() => flatResults.value.length > 0);

// Result groups derived from central config
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
        class="fixed inset-0 z-[9999] flex items-start justify-center px-4 pt-[12vh] sm:pt-[15vh]"
      >
        <!-- Backdrop with blur -->
        <div
          class="absolute inset-0 bg-black/40 dark:bg-black/60 backdrop-blur-sm"
          @click="closeSearch"
        />

        <!-- Modal container -->
        <div
          class="relative w-full max-w-[600px] bg-surface rounded-2xl shadow-2xl shadow-black/20 dark:shadow-black/40 overflow-hidden flex flex-col max-h-[min(70vh,600px)] ring-1 ring-default"
          role="dialog"
          aria-modal="true"
          aria-label="Search"
        >
          <!-- Search header -->
          <div class="flex items-center gap-3 px-4 py-3.5 border-b border-default bg-surface">
            <!-- Search icon with subtle animation -->
            <div class="relative flex-shrink-0">
              <svg
                :class="[
                  'w-5 h-5 transition-colors duration-200',
                  isLoading ? 'text-accent' : 'text-tertiary'
                ]"
                fill="none"
                viewBox="0 0 24 24"
                stroke="currentColor"
                stroke-width="2"
              >
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"
                />
              </svg>
              <!-- Loading ring -->
              <svg
                v-if="isLoading"
                class="absolute inset-0 w-5 h-5 animate-spin text-accent"
                fill="none"
                viewBox="0 0 24 24"
              >
                <circle
                  class="opacity-20"
                  cx="12"
                  cy="12"
                  r="10"
                  stroke="currentColor"
                  stroke-width="3"
                />
                <path
                  class="opacity-80"
                  fill="currentColor"
                  d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
                />
              </svg>
            </div>

            <!-- Input field -->
            <input
              ref="inputRef"
              v-model="query"
              type="text"
              placeholder="Search tickets, docs, devices, users..."
              class="flex-1 bg-transparent text-primary placeholder-tertiary/70 outline-none text-[15px] font-medium"
              autocomplete="off"
              spellcheck="false"
            />

            <!-- Keyboard hints -->
            <div class="hidden sm:flex items-center gap-1.5 flex-shrink-0">
              <kbd
                class="inline-flex items-center justify-center min-w-[24px] h-6 px-1.5 text-[11px] font-medium text-secondary bg-surface-alt rounded-md border border-default shadow-sm"
              >
                esc
              </kbd>
            </div>
          </div>

          <!-- Results area -->
          <div
            ref="resultsRef"
            class="flex-1 overflow-y-auto min-h-0 overscroll-contain"
          >
            <!-- Error state -->
            <div v-if="error" class="p-6 text-center">
              <div class="inline-flex items-center justify-center w-12 h-12 rounded-full bg-status-error-muted mb-3">
                <svg
                  class="w-6 h-6 text-status-error"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                  stroke-width="1.5"
                >
                  <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    d="M12 9v3.75m9-.75a9 9 0 11-18 0 9 9 0 0118 0zm-9 3.75h.008v.008H12v-.008z"
                  />
                </svg>
              </div>
              <p class="text-sm text-status-error font-medium">{{ error }}</p>
            </div>

            <!-- Empty query state -->
            <div
              v-else-if="!query.trim()"
              class="p-8 text-center"
            >
              <div class="inline-flex items-center justify-center w-14 h-14 rounded-2xl bg-surface-alt mb-4">
                <svg
                  class="w-7 h-7 text-tertiary"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                  stroke-width="1.5"
                >
                  <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    d="M21 21l-5.197-5.197m0 0A7.5 7.5 0 105.196 5.196a7.5 7.5 0 0010.607 10.607z"
                  />
                </svg>
              </div>
              <p class="text-sm text-secondary font-medium mb-2">Search your helpdesk</p>
              <p class="text-xs text-tertiary mb-4">
                Find tickets, documentation, devices, and more
              </p>
              <div class="flex items-center justify-center gap-3 text-[11px] text-tertiary">
                <span class="inline-flex items-center gap-1.5">
                  <kbd class="inline-flex items-center justify-center w-5 h-5 bg-surface-alt rounded border border-default text-[10px]">↑</kbd>
                  <kbd class="inline-flex items-center justify-center w-5 h-5 bg-surface-alt rounded border border-default text-[10px]">↓</kbd>
                  <span>Navigate</span>
                </span>
                <span class="inline-flex items-center gap-1.5">
                  <kbd class="inline-flex items-center justify-center min-w-[28px] h-5 px-1 bg-surface-alt rounded border border-default text-[10px]">↵</kbd>
                  <span>Select</span>
                </span>
                <span class="inline-flex items-center gap-1.5">
                  <kbd class="inline-flex items-center justify-center min-w-[24px] h-5 px-1 bg-surface-alt rounded border border-default text-[10px]">esc</kbd>
                  <span>Close</span>
                </span>
              </div>
            </div>

            <!-- No results state -->
            <div
              v-else-if="!isLoading && !hasResults"
              class="p-8 text-center"
            >
              <div class="inline-flex items-center justify-center w-14 h-14 rounded-2xl bg-surface-alt mb-4">
                <svg
                  class="w-7 h-7 text-tertiary"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                  stroke-width="1.5"
                >
                  <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    d="M15.182 16.318A4.486 4.486 0 0012.016 15a4.486 4.486 0 00-3.198 1.318M21 12a9 9 0 11-18 0 9 9 0 0118 0zM9.75 9.75c0 .414-.168.75-.375.75S9 10.164 9 9.75 9.168 9 9.375 9s.375.336.375.75zm-.375 0h.008v.015h-.008V9.75zm5.625 0c0 .414-.168.75-.375.75s-.375-.336-.375-.75.168-.75.375-.75.375.336.375.75zm-.375 0h.008v.015h-.008V9.75z"
                  />
                </svg>
              </div>
              <p class="text-sm text-secondary font-medium mb-1">No results found</p>
              <p class="text-xs text-tertiary">
                No matches for "<span class="text-primary font-medium">{{ query }}</span>"
              </p>
              <p class="text-xs text-tertiary mt-2">
                Try different keywords or check your spelling
              </p>
            </div>

            <!-- Results grouped by type -->
            <div v-else-if="hasResults" class="py-1">
              <SearchResultGroup
                v-for="group in resultGroups"
                :key="group.type"
                :type="group.type"
                :results="groupedResults[group.key]"
                :selected-id="selectedId"
                @select="navigateToResult"
              />
            </div>

            <!-- Loading skeleton (only show when query exists and still loading) -->
            <div v-else-if="isLoading && query.trim()" class="p-4 space-y-3">
              <div v-for="i in 4" :key="i" class="flex items-center gap-3 px-3 py-2.5">
                <div class="w-9 h-9 rounded-lg bg-surface-alt animate-pulse" />
                <div class="flex-1 space-y-2">
                  <div class="h-3.5 bg-surface-alt rounded animate-pulse w-3/4" />
                  <div class="h-2.5 bg-surface-alt rounded animate-pulse w-1/2" />
                </div>
              </div>
            </div>
          </div>

          <!-- Footer -->
          <div
            v-if="hasResults && !isLoading"
            class="flex items-center justify-between px-4 py-2 text-[11px] text-tertiary border-t border-default bg-surface-alt/50"
          >
            <span class="font-medium">
              {{ totalResults }} result{{ totalResults === 1 ? '' : 's' }}
            </span>
            <span class="tabular-nums">
              {{ searchTookMs }}ms
            </span>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
/* Modal entrance/exit animations */
.search-modal-enter-active,
.search-modal-leave-active {
  transition: opacity 0.2s ease;
}

.search-modal-enter-active > div:last-child,
.search-modal-leave-active > div:last-child {
  transition: transform 0.2s cubic-bezier(0.16, 1, 0.3, 1), opacity 0.2s ease;
}

.search-modal-enter-from,
.search-modal-leave-to {
  opacity: 0;
}

.search-modal-enter-from > div:last-child {
  opacity: 0;
  transform: scale(0.96) translateY(-8px);
}

.search-modal-leave-to > div:last-child {
  opacity: 0;
  transform: scale(0.98);
}

/* Custom scrollbar for results area */
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
