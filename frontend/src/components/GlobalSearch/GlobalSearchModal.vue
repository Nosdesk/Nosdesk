<script setup lang="ts">
import { ref, watch, computed, onScopeDispose } from 'vue';
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

// Publish `--keyboard-height` while open, so the mobile input bar can dock just
// above the keyboard (the only element that moves).
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

// Take the caret once the sheet is entering. On mobile the keyboard is already
// up (raised by the primer inside the opening tap), so this only moves focus
// into the real input, it never has to raise the keyboard itself. `preventScroll`
// guards WKWebView's scroll-to-input jump (a no-op under the body scroll-lock,
// but free).
const focusInput = () => inputRef.value?.focus({ preventScroll: true });

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

// ---------------------------------------------------------------
// Swipe-down-to-dismiss + open/close motion (mobile sheet).
//
// A full-screen sheet slides on the Y axis (Apple HIG / Material / vaul),
// not scale-fade like the desktop palette. A downward flick OR a drag past
// ~20% dismisses, and the close continues from the finger's release point
// straight off-screen (velocity-aware), never jumping back to origin.
// Curves: vaul/iOS open `cubic-bezier(.32,.72,0,1)`; an emphasized-accelerate
// close so the fling momentum carries.
// ---------------------------------------------------------------
const OPEN_EASE = 'cubic-bezier(0.32, 0.72, 0, 1)';
const CLOSE_EASE = 'cubic-bezier(0.3, 0, 0.8, 0.15)';
const OPEN_MS = 440;
const VELOCITY_DISMISS = 0.4; // px/ms downward flick
const DISTANCE_FRACTION = 0.2; // or dragged past 20% of the sheet height

const isMobile = () => window.innerWidth < 640;
const reduceMotion = () => window.matchMedia('(prefers-reduced-motion: reduce)').matches;

const dragY = ref(0);
const dragging = ref(false);
const snapping = ref(false);
let startY = 0;
let armed = false;

// Downward velocity: EMA of per-frame px/ms, so a late flick still counts.
let vel = 0;
let lastDy = 0;
let lastT = 0;
let flingVel = 0; // handed to the leave hook on dismiss

const dragStyle = computed(() => {
  if (!dragging.value && !snapping.value && dragY.value === 0) return {};
  return {
    transform: `translateY(${dragY.value}px)`,
    transition: dragging.value ? 'none' : 'transform 0.22s cubic-bezier(0.16,1,0.3,1)',
    willChange: 'transform',
  };
});

const onTouchStart = (e: TouchEvent) => {
  if (e.touches.length !== 1 || !isMobile()) return;
  startY = e.touches[0].clientY;
  // Arm only from the top of the results, so scrolling down never dismisses.
  armed = (resultsRef.value?.scrollTop ?? 0) <= 0;
  dragging.value = false;
  vel = 0;
  lastDy = 0;
  lastT = performance.now();
};

const onTouchMove = (e: TouchEvent) => {
  if (!armed) return;
  const dy = e.touches[0].clientY - startY;
  if (dy <= 0) {
    if (!dragging.value) armed = false; // upward: hand back to scrolling
    return;
  }
  const now = performance.now();
  const dt = now - lastT;
  if (dt > 0) vel = vel * 0.7 + ((dy - lastDy) / dt) * 0.3;
  lastDy = dy;
  lastT = now;
  dragging.value = true;
  dragY.value = dy;
  e.preventDefault();
};

const onTouchEnd = () => {
  armed = false;
  if (!dragging.value) return;
  dragging.value = false;
  const height =
    (document.querySelector('.search-card') as HTMLElement | null)?.getBoundingClientRect()
      .height ?? window.innerHeight;
  if (vel > VELOCITY_DISMISS || dragY.value > height * DISTANCE_FRACTION) {
    flingVel = Math.max(vel, 0);
    closeSearch(); // leave hook slides from dragY off-screen; do NOT reset dragY
    return;
  }
  // Snap back to rest.
  snapping.value = true;
  requestAnimationFrame(() => (dragY.value = 0));
  window.setTimeout(() => (snapping.value = false), 240);
};

// ---------------------------------------------------------------
// Enter/leave motion via WAAPI in JS hooks (the Transition uses
// `:css=false`). Mobile slides on Y; desktop keeps the scale-pop. The
// mobile leave starts from the live drag offset so a swipe flows straight
// into the close, and its duration derives from the fling velocity.
// ---------------------------------------------------------------
const cardOf = (el: Element) => el.querySelector('.search-card') as HTMLElement;
const backdropOf = (el: Element) => el.querySelector('.search-backdrop') as HTMLElement | null;

const onBeforeEnter = (el: Element) => {
  if (isMobile() && !reduceMotion()) cardOf(el).style.transform = 'translateY(100%)';
};

const onEnter = (el: Element, done: () => void) => {
  const card = cardOf(el);
  const clear = () => {
    card.style.transform = '';
    done();
  };
  focusInput(); // keyboard is already up on mobile (primer); just take the caret
  if (reduceMotion()) {
    clear();
    return;
  }
  const mobile = isMobile();
  backdropOf(el)?.animate([{ opacity: 0 }, { opacity: 1 }], {
    duration: mobile ? 280 : 150,
    easing: 'ease-out',
    fill: 'backwards',
  });
  const anim = mobile
    ? card.animate([{ transform: 'translateY(100%)' }, { transform: 'translateY(0)' }], {
        duration: OPEN_MS,
        easing: OPEN_EASE,
        fill: 'backwards',
      })
    : card.animate(
        [
          { opacity: 0, transform: 'scale(0.97) translateY(-6px)' },
          { opacity: 1, transform: 'none' },
        ],
        { duration: 180, easing: 'cubic-bezier(0.16,1,0.3,1)', fill: 'backwards' },
      );
  anim.onfinish = clear;
  anim.oncancel = clear;
};

const onLeave = (el: Element, done: () => void) => {
  const card = cardOf(el);
  const backdrop = backdropOf(el);
  const finish = () => {
    dragY.value = 0;
    flingVel = 0;
    done();
  };
  if (reduceMotion()) {
    finish();
    return;
  }
  if (isMobile()) {
    const from = dragY.value;
    const height = card.getBoundingClientRect().height || window.innerHeight;
    const remaining = Math.max(height - from, 1);
    const duration = flingVel > 0.1 ? Math.min(360, Math.max(160, remaining / flingVel)) : 260;
    backdrop?.animate([{ opacity: 1 }, { opacity: 0 }], {
      duration: Math.min(duration, 200),
      easing: 'ease-out',
      fill: 'forwards',
    });
    const anim = card.animate(
      [{ transform: `translateY(${from}px)` }, { transform: 'translateY(100%)' }],
      { duration, easing: CLOSE_EASE, fill: 'forwards' },
    );
    anim.onfinish = finish;
    anim.oncancel = finish;
  } else {
    backdrop?.animate([{ opacity: 1 }, { opacity: 0 }], {
      duration: 150,
      easing: 'ease',
      fill: 'forwards',
    });
    const anim = card.animate(
      [
        { opacity: 1, transform: 'none' },
        { opacity: 0, transform: 'scale(0.98)' },
      ],
      { duration: 150, easing: 'ease', fill: 'forwards' },
    );
    anim.onfinish = finish;
    anim.oncancel = finish;
  }
};

// ---------------------------------------------------------------
// Background scroll-lock (mobile). A position:fixed body (not
// overflow:hidden, which iOS ignores for rubber-band / keyboard
// pan) stops the layout viewport from panning under the keyboard,
// which is what otherwise slides the fixed sheet out of the visible
// band. The overlay is itself position:fixed (Teleported to body),
// so it escapes the lock and its results list keeps scrolling.
// ---------------------------------------------------------------
let restoreScroll: (() => void) | null = null;

const lockBody = () => {
  if (restoreScroll || window.innerWidth >= 640) return;
  const y = window.scrollY;
  const s = document.body.style;
  const prev = { position: s.position, top: s.top, left: s.left, right: s.right, width: s.width };
  s.position = 'fixed';
  s.top = `-${y}px`;
  s.left = '0';
  s.right = '0';
  s.width = '100%';
  restoreScroll = () => {
    Object.assign(document.body.style, prev);
    window.scrollTo(0, y);
    restoreScroll = null;
  };
};

watch(isOpen, (open) => (open ? lockBody() : restoreScroll?.()), { immediate: true });
onScopeDispose(() => restoreScroll?.());
</script>

<template>
  <Teleport to="body">
    <Transition
      :css="false"
      appear
      @before-enter="onBeforeEnter"
      @enter="onEnter"
      @appear="onEnter"
      @leave="onLeave"
    >
      <div
        v-if="isOpen"
        class="search-overlay fixed inset-0 z-overlay flex items-start justify-center sm:px-4 sm:pt-[15dvh]"
      >
        <!-- Backdrop. Subtle blur, click to dismiss. Fully covered by
             the sheet below `sm`, where the header close button takes
             over dismissal. Opacity is animated by the enter/leave hooks
             (search-backdrop). -->
        <div
          class="search-backdrop absolute inset-0 bg-black/40 dark:bg-black/60 backdrop-blur-sm"
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
          :style="dragStyle"
          @touchstart.passive="onTouchStart"
          @touchmove="onTouchMove"
          @touchend.passive="onTouchEnd"
          @touchcancel.passive="onTouchEnd"
        >
          <!-- Search input bar. Desktop: top header. Mobile: docks to the
               bottom, above the keyboard (see .search-inputbar), Firefox/Brave
               style, so the results fill the space above it. The wrapper owns
               the docking/border/background (and, on mobile, extends its
               background into the home-indicator safe area); the inner row is a
               clean fixed-height input line so the safe area never distorts it. -->
          <div class="search-inputbar flex-shrink-0">
            <div class="flex items-center gap-2.5 px-4 h-12">
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
/* Divider under the input row. This lives on the bar (not the row) because the
   Tailwind border moved off the row in the split; the mobile block below flips
   it to a top border since the bar docks to the bottom there. */
.search-inputbar {
  border-bottom: 1px solid var(--color-default);
}

/* Mobile: static full-screen overlay with the input docked to the bottom and
   riding the keyboard (Firefox/Brave style). Only the input bar tracks the
   keyboard; the overlay/results never resize, which is what kept every
   sheet-resizing approach feeling "forced". */
@media (max-width: 639.98px) {
  .search-overlay {
    height: 100dvh;
  }

  .search-card {
    height: 100%;
    max-height: none;
    border-radius: 0;
    padding-top: env(safe-area-inset-top);
    --input-bar-h: 3rem; /* matches the h-12 input row */
  }

  /* Input bar docked to the bottom, riding the keyboard via ONE compositor
     transform (no transition: visualViewport fires throughout the keyboard
     animation so it tracks 1:1; easing would make it lag). The border flips to
     the top edge, and it needs an opaque background since it now floats over
     the results; its padding-bottom extends that background into the
     home-indicator safe area below the fixed-height row. */
  .search-inputbar {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    transform: translateY(calc(-1 * var(--keyboard-height, 0px)));
    will-change: transform;
    border-bottom: 0;
    border-top: 1px solid var(--color-default);
    background: var(--color-surface);
    padding-bottom: env(safe-area-inset-bottom);
  }
  /* Keyboard up: the bar rests on the keyboard, so drop the home-indicator inset. */
  .search-card:has(input:focus, textarea:focus, [contenteditable]:focus) .search-inputbar {
    padding-bottom: 0;
  }

  /* Results fill the space above the docked input; pad the bottom so the last
     row clears the input bar + keyboard. Padding changes don't move the scroll
     offset, so toggling the keyboard never jumps the list. */
  .search-results {
    padding-bottom: calc(
      var(--input-bar-h) + var(--keyboard-height, 0px) + env(safe-area-inset-bottom)
    );
  }
}

/* Enter/leave motion is driven by the WAAPI hooks in <script> (the
   Transition runs with :css=false), so there are no transition classes here. */

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
