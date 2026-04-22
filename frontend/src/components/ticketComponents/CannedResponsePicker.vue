<!--
Dropdown button that lists the team's canned responses and emits the
selected body (with template variables already substituted) back up
to the composer.

Deliberately minimal: no search, no categorisation. Most teams have
<30 canned responses and a simple alphabetical list is easier to
scan than a search box with zero matches. Add search once the
template library actually grows.
-->
<template>
  <div class="relative">
    <button
      ref="triggerEl"
      type="button"
      @click="toggleOpen"
      :disabled="loading"
      class="h-10 px-3 bg-surface-alt border border-default text-secondary rounded-md hover:bg-surface-hover hover:text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-status-info transition-colors flex items-center gap-2"
      :aria-expanded="isOpen"
      aria-haspopup="listbox"
      aria-label="Insert canned response"
      :title="`Insert canned response (${shortcutLabel})`"
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
      Dropdown panel. `right-0 bottom-full` anchors top-right above the
      trigger; `max-w-[calc(100vw-1rem)]` keeps the 288px panel from
      overflowing on narrow (<320px) viewports.
    -->
    <div
      v-if="isOpen"
      ref="panelEl"
      class="absolute right-0 bottom-full mb-2 w-72 max-w-[calc(100vw-1rem)] max-h-80 overflow-y-auto bg-surface border border-default rounded-lg shadow-lg z-10 flex flex-col"
      role="listbox"
      tabindex="-1"
      aria-label="Canned responses"
      :aria-activedescendant="activeOptionId"
      @keydown="onPanelKeydown"
    >
      <div v-if="loading" class="px-4 py-3 text-sm text-tertiary">
        Loading…
      </div>
      <div v-else-if="error" class="px-4 py-3 text-sm text-status-error" role="alert">
        {{ error }}
      </div>
      <div v-else-if="responses.length === 0" class="px-4 py-3 flex flex-col gap-2">
        <p class="text-sm text-secondary">No canned responses yet.</p>
        <p class="text-xs text-tertiary">
          Admins can add templates in the admin area.
        </p>
      </div>
      <ul v-else class="flex flex-col" role="presentation">
        <li
          v-for="(r, i) in responses"
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
          <span class="text-sm font-medium text-primary truncate">{{ r.title }}</span>
          <span class="text-xs text-tertiary line-clamp-2">{{ r.body }}</span>
        </li>
      </ul>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import {
  cannedResponsesService,
  renderTemplate,
  type CannedResponse,
  type TemplateVars,
} from '@/services/cannedResponsesService';

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
}>();

const emit = defineEmits<{
  (e: 'insert', text: string): void;
}>();

const triggerEl = ref<HTMLButtonElement | null>(null);
const panelEl = ref<HTMLDivElement | null>(null);
const isOpen = ref(false);
const loading = ref(false);
const error = ref('');
const responses = ref<CannedResponse[]>([]);
const activeIndex = ref(0);
// Cache the fetch — list doesn't change mid-session often enough to
// warrant refetching on every dropdown open.
let loaded = false;

const uid = Math.random().toString(36).slice(2, 8);
const optionId = (i: number) => `canned-response-opt-${uid}-${i}`;
const activeOptionId = computed(() =>
  responses.value.length > 0 ? optionId(activeIndex.value) : undefined,
);

async function toggleOpen() {
  if (isOpen.value) {
    closePicker(false);
    return;
  }
  isOpen.value = true;
  activeIndex.value = 0;
  if (!loaded) {
    loading.value = true;
    error.value = '';
    try {
      responses.value = await cannedResponsesService.list();
      loaded = true;
    } catch {
      error.value = 'Failed to load templates';
    } finally {
      loading.value = false;
    }
  }
  // Move focus into the panel so arrow keys / Esc / Enter work
  // immediately — standard listbox behaviour.
  await nextTick();
  panelEl.value?.focus();
}

function closePicker(returnFocus: boolean) {
  isOpen.value = false;
  if (returnFocus) triggerEl.value?.focus();
}

function choose(r: CannedResponse) {
  // Render variables now so the tech sees the final text in the
  // composer before sending. Unknown tokens are preserved so they
  // can edit if they want to.
  emit('insert', renderTemplate(r.body, props.vars));
  closePicker(true);
}

function onPanelKeydown(e: KeyboardEvent) {
  const n = responses.value.length;
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
      if (n === 0) return;
      e.preventDefault();
      activeIndex.value = 0;
      scrollActiveIntoView();
      break;
    case 'End':
      if (n === 0) return;
      e.preventDefault();
      activeIndex.value = n - 1;
      scrollActiveIntoView();
      break;
    case 'Enter':
    case ' ':
      if (n === 0) return;
      e.preventDefault();
      choose(responses.value[activeIndex.value]);
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
  } else {
    document.removeEventListener('pointerdown', onOutsidePointerDown, true);
  }
});
</script>
