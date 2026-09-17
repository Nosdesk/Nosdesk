<!--
Dropdown button that lists the team's canned responses and emits the
selected body (with template variables already substituted) back up
to the composer.

The list is a Reka Listbox in a dialog popover (the SearchableDropdown
shape): a `ListboxFilter` at the top once any templates exist, focused
on open, owning the keyboard (arrows, Home/End, Enter inserts the
highlighted row) and naming the highlighted row through
`aria-activedescendant`. Substring match on title + first 150 chars of
body, case-insensitive, multi-term AND.
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
      aria-haspopup="dialog"
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
      Dropdown panel via the shared <ResponsiveMenu>: an anchored popover on
      desktop (teleports to <body>, stays anchored on scroll/resize, clamps to
      the viewport, dismisses on outside-click) and a bottom sheet on mobile. The
      inner div keeps the listbox semantics, scroll, and keyboard navigation.
    -->
    <ResponsiveMenu
      :open="isOpen"
      :anchor="anchor"
      placement="top-start"
      react-to-scroll="reposition"
      role="dialog"
      :aria-label="$t('ticket-picker-canned-trigger-aria')"
      :auto-focus="false"
      popover-class="w-72 max-w-[calc(100vw-1rem)] bg-surface border border-default rounded-lg shadow-lg overflow-hidden"
      @close="closePicker(false)"
    >
      <ListboxRoot
        :model-value="undefined"
        selection-behavior="replace"
        highlight-on-hover
        class="max-h-80 flex flex-col"
        @update:model-value="onPick"
        @highlight="onHighlight"
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
            <ListboxFilter
              v-model="searchQuery"
              auto-focus
              :placeholder="$t('ticket-picker-canned-search-placeholder')"
              :aria-label="$t('ticket-picker-canned-search-aria')"
              autocomplete="off"
              spellcheck="false"
              class="w-full bg-surface-alt border border-default rounded-md px-2 py-1.5 text-sm text-primary placeholder:text-tertiary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-status-info"
            />
          </div>
          <!-- One-line warning when the highlighted row references a
               variable not bound in the current ticket context.
               Updates live as the user arrows through matches so the
               warning travels with the row. -->
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
          <!-- The filter drives the list, so the rows are not focusable
               and the scrolling list is a tab stop of its own. -->
          <ListboxContent as-child>
            <ul
              class="flex flex-col overflow-y-auto min-h-0 outline-none"
              tabindex="0"
              :aria-label="$t('ticket-picker-canned-listbox-aria')"
            >
              <ListboxItem v-for="r in filteredResponses" :key="r.id" as-child :value="r.id">
                <li
                  class="w-full text-left px-4 py-2.5 cursor-pointer flex flex-col gap-0.5 transition-colors hover:bg-surface-hover data-[highlighted]:bg-surface-hover outline-none"
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
              </ListboxItem>
              <li
                v-if="filteredResponses.length === 0"
                class="px-4 py-3 text-sm text-tertiary"
                role="status"
              >
                {{ $t('ticket-picker-canned-no-matches', { query: searchQuery }) }}
              </li>
            </ul>
          </ListboxContent>
        </template>
      </ListboxRoot>
    </ResponsiveMenu>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { useFluent } from 'fluent-vue';
import { ListboxContent, ListboxFilter, ListboxItem, ListboxRoot } from 'reka-ui';
import { useQuery } from '@pinia/colada';
import {
  cannedResponsesService,
  renderTemplate,
  unboundVariables,
  type CannedResponseListItem,
  type TemplateVars,
} from '@nosdesk/core/services/cannedResponsesService';
import { highlightTerms } from '@nosdesk/core/utils/highlight';
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue';
import type { PopoverAnchor } from '@/composables/popoverAnchor';

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
const isOpen = ref(false);
const searchQuery = ref('');
// The highlighted row's id, from Reka.
const activeId = ref<number | null>(null);

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
 * Allow-list variables the active row references that would
 * substitute to empty against the current ticket context. The
 * resolver in `unboundVariables` handles derived variables
 * (e.g. `customer_first_name` is empty iff `customer_name` is
 * empty) so the picker doesn't need to know which names are
 * derived. If non-empty, a one-line warning above the result
 * list tells the agent which slots will be empty.
 */
const activeMissingVars = computed<string[]>(() => {
  const r = filteredResponses.value.find((x) => x.id === activeId.value);
  if (!r) return [];
  return unboundVariables(r.body, props.vars);
});

function onHighlight(item: { value: unknown } | undefined) {
  activeId.value = typeof item?.value === 'number' ? item.value : null;
}

const highlightTitle = (text: string): string => highlightTerms(text, searchTerms.value);
const highlightPreview = (text: string): string => highlightTerms(text, searchTerms.value);

// Anchor the Popover to the trigger button (resolved lazily so it tracks the
// live element across re-renders).
const anchor = computed<PopoverAnchor>(() => ({
  type: 'element',
  element: () => triggerEl.value,
}));

function toggleOpen() {
  if (isOpen.value) {
    closePicker(false);
    return;
  }
  isOpen.value = true;
  activeId.value = null;
  // The list query auto-fetches on first picker mount; nothing to
  // kick off here. The filter takes focus on mount; Escape reaches
  // the popover from anywhere.
}

function closePicker(returnFocus: boolean) {
  isOpen.value = false;
  // Reset query so the next open starts with the full list. The
  // template cache (`loaded`) is preserved.
  searchQuery.value = '';
  if (returnFocus) triggerEl.value?.focus();
}

function onPick(value: unknown) {
  const r = responses.value.find((x) => x.id === value);
  if (r) choose(r);
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

// Global shortcut — Ctrl+/ (Cmd+/ on Mac) toggles the picker.
// Only fires when the composer area has focus, so it doesn't hijack
// the shortcut globally across the app.
function onKeydown(e: KeyboardEvent) {
  const mod = isMac ? e.metaKey : e.ctrlKey;
  if (!mod || e.key !== '/') return;
  const active = document.activeElement as HTMLElement | null;
  if (!active?.closest('form, [contenteditable], textarea, input')) return;
  e.preventDefault();
  toggleOpen();
}
onMounted(() => window.addEventListener('keydown', onKeydown));
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown);
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
