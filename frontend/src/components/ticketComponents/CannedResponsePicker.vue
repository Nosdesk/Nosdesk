<!--
Dropdown button that lists the team's canned responses and emits the
selected body (with template variables already substituted) back up
to the composer.

Search box appears at the top of the dropdown once any templates
exist, focused on open (combobox pattern). Substring match on title
+ first 150 chars of body, case-insensitive, multi-term AND. Arrow
keys navigate the filtered list whether focus is in the input or
the panel; Enter inserts the active item.
-->
<template>
  <div>
    <button
      ref="triggerEl"
      type="button"
      @click="toggleOpen"
      :disabled="loading"
      class="h-9 px-2.5 bg-surface-alt border border-default text-secondary rounded-md hover:bg-surface-hover hover:text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-status-info transition-colors flex items-center justify-center"
      :aria-expanded="isOpen"
      aria-haspopup="listbox"
      :aria-label="$t('ticket-picker-canned-trigger-aria')"
      :title="$t('ticket-picker-canned-trigger-title', { shortcut: shortcutLabel })"
    >
      <svg
        class="h-5 w-5"
        fill="none"
        stroke="currentColor"
        viewBox="0 0 24 24"
      >
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          stroke-width="2"
          d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2m-6 9l2 2 4-4"
        />
      </svg>
    </button>

    <!--
      Dropdown panel teleported to <body> so it escapes any ancestor
      `overflow: hidden` (the surrounding `SectionCard` clips its
      contents). Positioning is computed from the trigger's bounding
      rect every time the panel opens and on resize / scroll, mirroring
      the pattern already used by `SimpleEditor`'s mention dropdown.

      `bottom`/`right` anchor the panel's bottom-right to the trigger's
      top-right so it opens upward and right-aligned, matching the
      original anchor without the clipping problem.
    -->
    <Teleport to="body">
      <div
        v-if="isOpen"
        ref="panelEl"
        :style="panelStyle"
        class="w-72 max-w-[calc(100vw-1rem)] max-h-80 overflow-y-auto bg-surface border border-default rounded-lg shadow-lg flex flex-col"
        role="listbox"
        tabindex="-1"
        :aria-label="$t('ticket-picker-canned-listbox-aria')"
        :aria-activedescendant="activeOptionId"
        @keydown="onPanelKeydown"
      >
        <div v-if="loading" class="px-4 py-3 text-sm text-tertiary">
          {{ $t('ticket-picker-canned-loading') }}
        </div>
        <div v-else-if="error" class="px-4 py-3 text-sm text-status-error" role="alert">
          {{ error }}
        </div>
        <div v-else-if="responses.length === 0" class="px-4 py-3 flex flex-col gap-2">
          <p class="text-sm text-secondary">{{ $t('ticket-picker-canned-empty-title') }}</p>
          <p class="text-xs text-tertiary">
            {{ $t('ticket-picker-canned-empty-hint') }}
          </p>
        </div>
        <template v-else>
          <div class="px-3 py-2 border-b border-default">
            <input
              ref="searchInputEl"
              v-model="searchQuery"
              type="text"
              :placeholder="$t('ticket-picker-canned-search-placeholder')"
              :aria-label="$t('ticket-picker-canned-search-aria')"
              aria-autocomplete="list"
              autocomplete="off"
              spellcheck="false"
              class="w-full bg-surface-alt border border-default rounded-md px-2 py-1.5 text-sm text-primary placeholder:text-tertiary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-status-info"
            />
          </div>
          <!-- One-line warning when the active row references a
               variable not bound in the current ticket context.
               Updates live as the user arrows through matches so the
               warning travels with the active row. -->
          <div
            v-if="activeMissingVars.length > 0"
            class="px-3 py-1.5 text-xs text-status-warning bg-status-warning/10 border-b border-default"
            role="status"
          >
            {{
              $t('ticket-picker-canned-missing-vars', {
                names: activeMissingVars.join(', '),
              })
            }}
          </div>
          <ul
            v-if="filteredResponses.length > 0"
            class="flex flex-col"
            role="presentation"
          >
            <li
              v-for="(r, i) in filteredResponses"
              :id="optionId(i)"
              :key="r.id"
              role="option"
              :aria-selected="i === activeIndex"
              @mousemove="activeIndex = i"
              @click="choose(r)"
              :class="[
                'w-full text-left px-4 py-2.5 cursor-pointer flex flex-col gap-0.5 transition-colors',
                i === activeIndex ? 'bg-surface-hover' : 'hover:bg-surface-hover',
              ]"
            >
              <span
                class="text-sm font-medium text-primary truncate"
                v-html="highlightTitle(r.title)"
              />
              <!-- Render the substituted body so the agent sees the
                   final text they're about to insert; variables that
                   would resolve are visible inline. -->
              <span
                class="text-xs text-tertiary line-clamp-2"
                v-html="highlightPreview(previewBody(r))"
              />
            </li>
          </ul>
          <div
            v-else
            class="px-4 py-3 text-sm text-tertiary"
            role="status"
          >
            {{ $t('ticket-picker-canned-no-matches', { query: searchQuery }) }}
          </div>
        </template>
      </div>
    </Teleport>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery } from '@pinia/colada';
import {
  cannedResponsesService,
  renderTemplate,
  variablesUsed,
  type CannedResponseListItem,
  type TemplateVars,
} from '@/services/cannedResponsesService';
import { highlightTerms } from '@/utils/highlight';

const { $t } = useFluent();

// Detect modifier label. Help Scout / Front / Zendesk all expose a
// canned-response keybind in the composer; we pick Ctrl+/ (Cmd+/ on
// Mac) because it mirrors "show shortcuts" palettes the user already
// knows from VS Code / GitHub and doesn't clash with plain typing.
// Prefer `userAgentData` where available since `navigator.platform`
// is deprecated and being frozen by Safari.
const isMac = (() => {
  if (typeof navigator === 'undefined') return false;
  const uad = (navigator as Navigator & { userAgentData?: { platform?: string } }).userAgentData;
  const platform = uad?.platform ?? navigator.platform ?? '';
  return /Mac|iPhone|iPad/i.test(platform);
})();
const shortcutLabel = isMac ? '⌘/' : 'Ctrl+/';

const props = defineProps<{
  /** Template context for `{{variable}}` substitution on insert. */
  vars: TemplateVars;
  /** Optional ticket id passed through to the workspace-local
   * insertion log so the admin page can correlate which templates
   * are inserted on which tickets. Fire-and-forget; logging
   * failures never block the insert. */
  ticketId?: number;
}>();

const emit = defineEmits<{
  (e: 'insert', text: string): void;
}>();

const triggerEl = ref<HTMLButtonElement | null>(null);
const panelEl = ref<HTMLDivElement | null>(null);
const searchInputEl = ref<HTMLInputElement | null>(null);
const isOpen = ref(false);
const searchQuery = ref('');
const activeIndex = ref(0);

// Shared with the admin CannedResponsesView and EditView so an
// admin save invalidates the picker's view for every open composer
// in the session, no manual refetch needed. Eager-fetches once per
// session on first picker mount; subsequent ticket views read the
// cached list instantly.
const CANNED_RESPONSES_KEY = ['canned-responses'] as const;
const listQuery = useQuery({
  key: CANNED_RESPONSES_KEY,
  query: () => cannedResponsesService.list(),
});
const responses = computed<CannedResponseListItem[]>(() =>
  Array.isArray(listQuery.data.value) ? listQuery.data.value : [],
);
const loading = computed(
  () => listQuery.status.value === 'pending' && listQuery.data.value === undefined,
);
const error = computed(() =>
  listQuery.error.value ? $t('ticket-picker-canned-load-error') : '',
);

// Parsed search terms shared by the filter and the hit highlighter.
const searchTerms = computed<string[]>(() =>
  searchQuery.value
    .toLowerCase()
    .split(/\s+/)
    .map((s) => s.trim())
    .filter(Boolean),
);

// Substring filter on title + first 150 chars of body, case-
// insensitive, multi-term AND. Body slice is enough to disambiguate
// titles without scanning huge templates on every keystroke; <1000
// items is instant client-side so no debounce needed.
const filteredResponses = computed(() => {
  if (searchTerms.value.length === 0) return responses.value;
  return responses.value.filter((r) => {
    const haystack = `${r.title.toLowerCase()} ${r.body.slice(0, 150).toLowerCase()}`;
    return searchTerms.value.every((t) => haystack.includes(t));
  });
});

/**
 * The template body rendered against the current ticket context.
 * The picker displays this in each row so the agent sees the final
 * text they're about to insert; `{{customer_name}}` becomes the
 * customer's actual name in the preview (or vanishes to the empty
 * string when the value is missing, which the warn-banner below
 * also surfaces).
 */
function previewBody(r: CannedResponseListItem): string {
  return renderTemplate(r.body, props.vars);
}

/**
 * Allow-list variables the active row references that don't have a
 * value bound in the current ticket context. If non-empty, the
 * picker shows a one-line warning so the agent knows the rendered
 * text will have empty slots where those names would go.
 */
const activeMissingVars = computed<string[]>(() => {
  const r = filteredResponses.value[activeIndex.value];
  if (!r) return [];
  const used = variablesUsed(r.body);
  return used.filter((name) => {
    const v = (props.vars as Record<string, unknown>)[name];
    return v == null || v === '';
  });
});

const highlightTitle = (text: string): string => highlightTerms(text, searchTerms.value);
const highlightPreview = (text: string): string => highlightTerms(text, searchTerms.value);

// Reset highlight to the top of the (newly filtered) list whenever
// the query changes so Enter picks the most relevant match.
watch(searchQuery, () => {
  activeIndex.value = 0;
});
/**
 * Live bounding rect of the trigger button. Updated whenever the
 * panel opens, the window resizes, or any ancestor scrolls — keeping
 * the teleported panel anchored to the button it logically belongs to.
 */
const triggerRect = ref<DOMRect | null>(null);
/**
 * Inline style for the teleported dropdown. Positions the panel's
 * bottom-right against the trigger's top-right with an 8px gap so it
 * opens upward + right-aligned, the same anchor the absolute version
 * had before. Falls back to `display: none` between renders so the
 * teleported node doesn't briefly flash at (0,0) before the rect is
 * read on first open.
 */
const panelStyle = computed(() => {
  if (!isOpen.value || !triggerRect.value) return { display: 'none' };
  const rect = triggerRect.value;
  const GAP_PX = 8;
  return {
    position: 'fixed' as const,
    bottom: `${window.innerHeight - rect.top + GAP_PX}px`,
    right: `${window.innerWidth - rect.right}px`,
    zIndex: 50,
  };
});

function captureTriggerRect() {
  triggerRect.value = triggerEl.value?.getBoundingClientRect() ?? null;
}

const uid = Math.random().toString(36).slice(2, 8);
const optionId = (i: number) => `canned-response-opt-${uid}-${i}`;
const activeOptionId = computed(() =>
  filteredResponses.value.length > 0 ? optionId(activeIndex.value) : undefined,
);

async function toggleOpen() {
  if (isOpen.value) {
    closePicker(false);
    return;
  }
  // Read the rect synchronously before flipping `isOpen` so the
  // teleported panel renders in the right place on its first paint
  // (instead of briefly flashing at the document origin while we
  // wait for the next tick).
  captureTriggerRect();
  isOpen.value = true;
  activeIndex.value = 0;
  // The list query auto-fetches on first picker mount; nothing to
  // kick off here. Focus the search input so the user can start
  // typing immediately (standard combobox UX). Falls back to the
  // panel itself when the input isn't rendered yet (loading / error
  // / empty-library states), so arrow keys / Esc / Enter still work.
  await nextTick();
  (searchInputEl.value ?? panelEl.value)?.focus();
}

function closePicker(returnFocus: boolean) {
  isOpen.value = false;
  // Reset query so the next open starts with the full list. The
  // template cache (`loaded`) is preserved.
  searchQuery.value = '';
  if (returnFocus) triggerEl.value?.focus();
}

function choose(r: CannedResponseListItem) {
  // Render variables now so the tech sees the final text in the
  // composer before sending. Unknown tokens are preserved so they
  // can edit if they want to.
  emit('insert', renderTemplate(r.body, props.vars));
  // Fire-and-forget usage log so the admin page's "Inserts (30d)"
  // column tracks this use. The service swallows transport errors
  // and the backend treats every failure path as 200, so this never
  // blocks the user-facing insert.
  void cannedResponsesService.recordInsertion(r.id, props.ticketId);
  closePicker(true);
}

// Keydown lives on the panel so it fires whether focus is in the
// search input or the listbox; arrow keys always navigate the
// filtered list. Home/End only apply when focus is NOT in the
// input — otherwise they jump the text caret, which the user
// expects in a text field.
function onPanelKeydown(e: KeyboardEvent) {
  const n = filteredResponses.value.length;
  const inSearch = e.target === searchInputEl.value;
  switch (e.key) {
    case 'Escape':
      e.preventDefault();
      closePicker(true);
      break;
    case 'ArrowDown':
      if (n === 0) return;
      e.preventDefault();
      activeIndex.value = (activeIndex.value + 1) % n;
      scrollActiveIntoView();
      break;
    case 'ArrowUp':
      if (n === 0) return;
      e.preventDefault();
      activeIndex.value = (activeIndex.value - 1 + n) % n;
      scrollActiveIntoView();
      break;
    case 'Home':
      if (n === 0 || inSearch) return;
      e.preventDefault();
      activeIndex.value = 0;
      scrollActiveIntoView();
      break;
    case 'End':
      if (n === 0 || inSearch) return;
      e.preventDefault();
      activeIndex.value = n - 1;
      scrollActiveIntoView();
      break;
    case 'Enter':
      if (n === 0) return;
      e.preventDefault();
      choose(filteredResponses.value[activeIndex.value]);
      break;
    case ' ':
      // Space inserts only when the listbox itself has focus —
      // otherwise we'd block typing a space in the search query.
      if (n === 0 || inSearch) return;
      e.preventDefault();
      choose(filteredResponses.value[activeIndex.value]);
      break;
  }
}

function scrollActiveIntoView() {
  const el = document.getElementById(optionId(activeIndex.value));
  el?.scrollIntoView({ block: 'nearest' });
}

// Global shortcut — Ctrl+/ (Cmd+/ on Mac) toggles the picker.
// Only fires when the composer area has focus, so it doesn't hijack
// the shortcut globally across the app.
function onKeydown(e: KeyboardEvent) {
  const mod = isMac ? e.metaKey : e.ctrlKey;
  if (!mod || e.key !== '/') return;
  const active = document.activeElement as HTMLElement | null;
  if (!active?.closest('form, [contenteditable], textarea, input')) return;
  e.preventDefault();
  void toggleOpen();
}
onMounted(() => window.addEventListener('keydown', onKeydown));
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown);
  document.removeEventListener('pointerdown', onOutsidePointerDown, true);
  // Symmetric cleanup of the anchor-tracking listeners in case the
  // component unmounts while the panel is still open.
  window.removeEventListener('resize', captureTriggerRect);
  window.removeEventListener('scroll', captureTriggerRect, { capture: true } as EventListenerOptions);
});

// Close on outside pointer-down. `pointerdown` covers mouse + touch
// in one listener, and firing on the down-edge avoids the case where
// a click on the trigger slips through between open and listener-
// attachment. Using capture phase so we run before app click handlers.
function onOutsidePointerDown(ev: PointerEvent) {
  if (!isOpen.value) return;
  const target = ev.target as Node | null;
  if (!target) return;
  if (panelEl.value?.contains(target)) return;
  if (triggerEl.value?.contains(target)) return;
  isOpen.value = false;
}

watch(isOpen, (open) => {
  if (open) {
    document.addEventListener('pointerdown', onOutsidePointerDown, true);
    // Keep the teleported panel anchored to the trigger as the user
    // scrolls or resizes. `passive: true` keeps scroll smooth — we
    // only read the rect, never call `preventDefault`. Capture phase
    // catches scrolls inside any ancestor, not just the document.
    window.addEventListener('resize', captureTriggerRect, { passive: true });
    window.addEventListener('scroll', captureTriggerRect, { capture: true, passive: true });
  } else {
    document.removeEventListener('pointerdown', onOutsidePointerDown, true);
    window.removeEventListener('resize', captureTriggerRect);
    window.removeEventListener('scroll', captureTriggerRect, { capture: true } as EventListenerOptions);
  }
});
</script>

<style scoped>
/* `<mark>` lives inside the teleported panel. Vue 3 carries scoped
   data-attributes to teleported descendants, so this still applies
   despite the dropdown rendering outside the picker's DOM subtree. */
:deep(mark) {
  background-color: rgb(var(--color-accent) / 0.25);
  color: inherit;
  padding: 0 2px;
  border-radius: 2px;
}
</style>
