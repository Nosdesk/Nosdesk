<!--
Shared multi-line text input: the FormInput field styling applied
to a <textarea>, with label + hint/error and programmatic label
association. `mono` switches to a monospace face (signatures,
code-ish content).

Auto-resizes from `rows` (minimum visible) up to `maxRows` (cap;
content past this scrolls internally). Pass `maxRows: 0` for a
fixed `rows` height. `resize` controls whether the user can drag
the corner grip (defaults to vertical-only).

Arbitrary native attributes (name, maxlength, readonly, @blur, ...)
fall through to the <textarea>; a `class` on the component lands
on the wrapper.
-->
<script setup lang="ts">
import { computed, nextTick, onMounted, ref, useId, watch } from 'vue';

interface Props {
  label?: string;
  placeholder?: string;
  description?: string;
  error?: string;
  required?: boolean;
  disabled?: boolean;
  /** Starting / minimum row count. */
  rows?: number;
  /** Maximum rows before internal scroll. 0 disables auto-resize
   *  entirely (fixed `rows` height). */
  maxRows?: number;
  /** User-controlled corner-grip resize. Defaults to vertical. */
  resize?: 'none' | 'vertical';
  /** Render the textarea in a monospace face. */
  mono?: boolean;
  id?: string;
}

defineOptions({ inheritAttrs: false });

// Default `resize: 'none'`. Convergent practice in 2026 (Adobe
// Spectrum, Mantine, Fluent, shadcn v4): once auto-grow with a
// maxRows cap is wired correctly, the manual grip is redundant
// chrome, costs WCAG 2.5.7 (Dragging Movements) compliance, and
// produces cross-browser-inconsistent native grips. Long-form
// authoring surfaces (the ticket Resolution field, the article
// editor) explicitly opt back in with `resize="vertical"`.
const props = withDefaults(defineProps<Props>(), {
  rows: 4,
  maxRows: 0,
  resize: 'none',
  mono: false,
});

const model = defineModel<string>({ required: true });

const generatedId = useId();
const textareaId = computed(() => props.id ?? generatedId);
const describedById = computed(() =>
  props.error || props.description ? `${textareaId.value}-desc` : undefined,
);

const textareaRef = ref<HTMLTextAreaElement | null>(null);

// Track the floor for auto-resize. Starts at the natural `rows`-
// derived minimum; bumps up whenever the user drags the custom
// grip taller, so once they've made it bigger by hand the field
// doesn't snap back to small when content shrinks or they delete
// text. Resets to null only on explicit clear (not exposed yet).
const manualMinHeight = ref<number | null>(null);

function autoSizeBounds(): { min: number; max: number } | null {
  const el = textareaRef.value;
  if (!el) return null;
  const styles = getComputedStyle(el);
  const lineHeight = parseFloat(styles.lineHeight) || 20;
  const paddingY =
    parseFloat(styles.paddingTop) + parseFloat(styles.paddingBottom);
  const min = lineHeight * props.rows + paddingY;
  const max = props.maxRows > 0
    ? lineHeight * props.maxRows + paddingY
    : Number.POSITIVE_INFINITY;
  return { min, max };
}

// Auto-grow. Set height to 'auto' first so the browser recomputes
// scrollHeight against the natural content size; otherwise removing
// content wouldn't shrink the field. Then clamp the next height to
// the [min, max] range, treating the user's last drag as a floor.
function resize(): void {
  if (props.maxRows === 0 && manualMinHeight.value === null) return;
  const el = textareaRef.value;
  if (!el) return;
  el.style.height = 'auto';
  const bounds = autoSizeBounds();
  if (!bounds) return;
  const floor = Math.max(bounds.min, manualMinHeight.value ?? 0);
  const next = Math.min(Math.max(el.scrollHeight, floor), bounds.max);
  el.style.height = `${next}px`;
}

onMounted(() => {
  void nextTick(resize);
});

watch(model, () => {
  void nextTick(resize);
});

// --- Custom drag-to-resize -------------------------------------
//
// Hides the native ::-webkit-resizer + Firefox handle (via the
// `resize-none` class below) and renders a styled SVG grip in the
// bottom-right corner. `pointerdown/move/up` handlers cover mouse
// + touch + pen in one wiring. While dragging:
//
//   - `setPointerCapture` on the grip element keeps move events
//     flowing even if the pointer leaves the grip's bounds
//     (otherwise a fast drag escapes the 16x16 hit target and the
//     resize stops mid-gesture).
//   - height is set in px directly, bypassing auto-resize so the
//     field tracks the cursor smoothly.
//   - the final height is recorded as `manualMinHeight` so future
//     auto-resizes won't shrink below the user's chosen size.

const gripEl = ref<HTMLElement | null>(null);
const isDragging = ref(false);
let dragStartY = 0;
let dragStartHeight = 0;

// rAF throttling for pointermove. High-refresh-rate pointers
// (Magic Trackpad on a 120Hz display, gaming mice) fire move
// events faster than the browser can lay out, and unthrottled
// `style.height = ...` thrashes layout. Coalescing pointer events
// into a single per-frame update batches the layout work to the
// browser's render cadence — smoother visible motion, less CPU.
let pendingClientY = 0;
let rafId: number | null = null;

function applyDrag(clientY: number): void {
  const el = textareaRef.value;
  if (!el) return;
  const delta = clientY - dragStartY;
  const bounds = autoSizeBounds();
  // Min: never let the user drag below the `rows`-derived floor.
  // Max: respect maxRows when set; otherwise let the user drag
  // arbitrarily (a multi-paragraph paste case).
  const minH = bounds?.min ?? 40;
  const maxH = bounds?.max ?? Number.POSITIVE_INFINITY;
  const next = Math.min(Math.max(dragStartHeight + delta, minH), maxH);
  el.style.height = `${next}px`;
}

function onGripPointerDown(event: PointerEvent): void {
  const el = textareaRef.value;
  if (!el) return;
  event.preventDefault();
  dragStartY = event.clientY;
  dragStartHeight = el.offsetHeight;
  isDragging.value = true;
  // `will-change: height` hints the browser to optimise for
  // height changes and promotes the textarea to its own
  // compositor layer for the drag, so reflow doesn't cascade to
  // ancestor repaints. Cleared on pointerup — keeping it set
  // permanently wastes layer memory.
  el.style.willChange = 'height';
  gripEl.value?.setPointerCapture(event.pointerId);
  gripEl.value?.addEventListener('pointermove', onGripPointerMove);
  gripEl.value?.addEventListener('pointerup', onGripPointerUp, { once: true });
  gripEl.value?.addEventListener('pointercancel', onGripPointerUp, { once: true });
}

function onGripPointerMove(event: PointerEvent): void {
  pendingClientY = event.clientY;
  if (rafId !== null) return;
  rafId = requestAnimationFrame(() => {
    rafId = null;
    applyDrag(pendingClientY);
  });
}

function onGripPointerUp(event: PointerEvent): void {
  const el = textareaRef.value;
  isDragging.value = false;
  if (rafId !== null) {
    cancelAnimationFrame(rafId);
    rafId = null;
  }
  // Flush any final pending update so the released height matches
  // the cursor's last position exactly, not the last rAF tick.
  if (el) applyDrag(pendingClientY);
  gripEl.value?.releasePointerCapture(event.pointerId);
  gripEl.value?.removeEventListener('pointermove', onGripPointerMove);
  if (el) {
    el.style.willChange = 'auto';
    manualMinHeight.value = el.offsetHeight;
  }
}
</script>

<template>
  <div class="flex flex-col gap-1.5" :class="$attrs.class">
    <label
      v-if="label"
      :for="textareaId"
      class="text-xs font-medium text-tertiary uppercase tracking-wide"
    >
      {{ label }}<span v-if="required" class="text-status-error ml-0.5" aria-hidden="true">*</span>
    </label>
    <!-- Relative container holds the textarea and the custom grip.
         The native resize handle is suppressed (`resize-none`) so
         the SVG grip below is the only manual-resize affordance —
         consistent across Chrome / Safari / Firefox where the
         native handles all look different.
         `contain: layout` scopes reflow during drag-resize to this
         wrapper, so the parent flex/grid layout doesn't re-pass
         on every height tick — visibly smoother on tall pages. -->
    <div class="relative" style="contain: layout">
      <textarea
        :id="textareaId"
        ref="textareaRef"
        v-model="model"
        :placeholder="placeholder"
        :required="required"
        :disabled="disabled"
        :rows="rows"
        :aria-invalid="error ? 'true' : undefined"
        :aria-describedby="describedById"
        :class="[
          'w-full bg-surface-alt border rounded-lg text-primary placeholder-tertiary transition-colors',
          'focus:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:border-accent',
          'disabled:opacity-50 disabled:cursor-not-allowed',
          'px-3 py-2',
          // Always suppress the native resize chrome; the custom
          // grip below is the single source of resize affordance.
          'resize-none',
          // Symmetric 12px inside padding both axes when manual
          // resize is enabled. The grip sits flush-right and
          // visually overlaps the typing area at the bottom-right,
          // but the corner-padding is small enough that real
          // content rarely reaches it. Default (`resize: 'none'`)
          // reclaims even this and uses the standard `px-3 py-2`.
          resize === 'none' ? '' : 'pe-3 pb-3',
          mono ? 'font-mono text-sm' : 'text-sm',
          error ? 'border-status-error' : 'border-subtle hover:border-default',
        ]"
        v-bind="{ ...$attrs, class: undefined }"
      />
      <!-- Custom drag-to-resize grip. 24×24 hit target (WCAG 2.5.8)
           with an 8×8 visual in the bottom-right corner. Two short
           diagonal strokes are the convergent grip iconography
           (Carbon, Polaris, native browser grips all use this
           shape); dots / triangles / pull-tabs test as decoration.
           Glyph is tertiary at rest, secondary on hover with a
           subtle hover-bg to confirm the hit target, accent while
           dragging. `touch-action: none` keeps the browser's
           gesture pipeline from competing with our pointer
           handlers. Hidden on coarse pointers (touch): no surveyed
           mobile composer exposes manual resize, the corner is
           hard to hit precisely with a finger, and it conflicts
           with OS keyboard-avoidance chrome.
           `tabindex="-1"`: keyboard users can't usefully drag; the
           auto-grow + maxRows cap is the keyboard path. Hidden
           when resize is disabled or the field is disabled. -->
      <button
        v-if="resize !== 'none' && !disabled"
        ref="gripEl"
        type="button"
        class="absolute bottom-1.5 right-0 w-6 h-6 flex items-center justify-center cursor-ns-resize touch-none rounded transition-colors motion-reduce:transition-none focus:outline-none focus-visible:ring-2 focus-visible:ring-accent [@media(pointer:coarse)]:hidden"
        :class="isDragging
          ? 'text-accent'
          : 'text-tertiary hover:text-secondary hover:bg-surface-hover/60'"
        :aria-label="$t('form-textarea-resize-grip-label')"
        tabindex="-1"
        @pointerdown="onGripPointerDown"
      >
        <svg viewBox="0 0 8 8" class="w-2 h-2" aria-hidden="true">
          <line x1="6.5" y1="1.5" x2="1.5" y2="6.5" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" />
          <line x1="6.5" y1="4.5" x2="4.5" y2="6.5" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" />
        </svg>
      </button>
    </div>
    <p v-if="error" :id="describedById" class="text-xs text-status-error">{{ error }}</p>
    <p v-else-if="description" :id="describedById" class="text-xs text-tertiary">
      {{ description }}
    </p>
  </div>
</template>
