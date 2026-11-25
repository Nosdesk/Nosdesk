<!--
Click-to-edit text on Reka's Editable. The preview is a tab stop that
opens the editor on focus (keyboard users get in, which the old click-only
div never allowed), the input takes focus with its text selected, Enter
and blur submit, Escape cancels, and Reka's dismiss layers handle the
outside click.

Two contracts this keeps from the old component, both above Reka's
model:
- `update:modelValue` fires once per edit session and only when the
  value changed; this is the event that writes and audits. Reka's submit
  writes unconditionally, so the wrapper compares against the value at
  edit start.
- The parent's value is held constant while editing. The per-keystroke
  `preview` echoes back through `modelValue` (and a remote update can
  arrive mid-edit); Reka would copy either into the input and clobber
  the draft.

`multiline` is the page title on a phone: the display wraps to as many
lines as the text needs and the editor is a textarea of the same box, so
entering the edit moves nothing. The value is still one line: Enter
submits, and a pasted line break becomes a space.
-->
<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import { EditableArea, EditableInput, EditablePreview, EditableRoot } from 'reka-ui';
import { useFluent } from 'fluent-vue';

interface Props {
  modelValue: string;
  placeholder?: string;
  /** Accessible name for the editor; defaults to the placeholder. */
  label?: string;
  textSize?: 'sm' | 'base' | 'lg' | 'xl' | '2xl';
  canEdit?: boolean;
  prefix?: string;
  showEditHint?: boolean;
  truncate?: boolean;
  /** Cap the display mode to N lines with ellipsis past that.
   * Overrides `truncate` when set. `2` is the recommended value
   * for header titles: a long title wraps once instead of being
   * lost to ellipsis, but never grows the row beyond two lines.
   * Unset = unlimited wrap (the previous `truncate: false` default). */
  maxLines?: 1 | 2;
  /** Wrap without limit and edit in a textarea that grows with the text.
   *  Overrides `maxLines` and `truncate`. */
  multiline?: boolean;
  /** The element the display renders as. `h1` makes it the page heading. */
  as?: 'div' | 'h1';
}

const props = withDefaults(defineProps<Props>(), {
  placeholder: undefined,
  textSize: 'base',
  canEdit: true,
  prefix: '',
  showEditHint: true,
  truncate: false,
  multiline: false,
  as: 'div',
});

const emit = defineEmits<{
  // Committed once per edit session (blur / Enter), only when the value
  // actually changed. This is the one that should write/audit.
  'update:modelValue': [value: string];
  // Transient draft on every keystroke. For live display and optional
  // SSE field-preview broadcast; never commits.
  preview: [value: string];
}>();

const fluent = useFluent();

const isEditing = ref(false);
// What Reka sees. Follows the parent while idle, frozen while editing.
const held = ref(props.modelValue);
const originalValue = ref(props.modelValue);

watch(
  () => props.modelValue,
  (next) => {
    if (!isEditing.value) held.value = next;
  },
);

// The textarea's text, mirrored into the sizer that gives it its height.
const draft = ref(props.modelValue);
const textarea = ref<HTMLTextAreaElement | null>(null);

function onState(state: 'edit' | 'submit' | 'cancel') {
  if (state === 'edit') {
    originalValue.value = props.modelValue;
    held.value = props.modelValue;
    draft.value = props.modelValue;
    isEditing.value = true;
    // A long title selected whole is one stray key from gone, and a phone
    // has no Escape. The caret goes to the end instead. Reka focuses the
    // field on the next tick; this runs after it.
    if (props.multiline) {
      void nextTick(() => nextTick(() => {
        const end = textarea.value?.value.length ?? 0;
        textarea.value?.setSelectionRange(end, end);
      }));
    }
    return;
  }
  isEditing.value = false;
  held.value = props.modelValue;
  if (state === 'cancel') emit('preview', originalValue.value);
}

// Reka submits unconditionally; only a real change reaches the parent.
function onSubmit(raw: string) {
  const value = props.multiline ? oneLine(raw) : raw;
  if (value !== originalValue.value) emit('update:modelValue', value);
}

// A title is one line of text however many lines it wraps to.
const oneLine = (text: string) => text.replace(/\s*[\r\n]+\s*/g, ' ');

function onInput(event: Event) {
  emit('preview', (event.target as HTMLInputElement).value);
}

// A pasted line break becomes a space as it lands. Reka has already read
// the raw text by the time this runs, so the cleaned text goes round once
// more as its own input event for Reka to read too.
function onTextareaInput(event: Event) {
  const field = event.target as HTMLTextAreaElement;
  if (/[\r\n]/.test(field.value)) {
    const caret = oneLine(field.value.slice(0, field.selectionStart)).length;
    field.value = oneLine(field.value);
    field.setSelectionRange(caret, caret);
    field.dispatchEvent(new Event('input', { bubbles: true }));
    return;
  }
  draft.value = field.value;
  emit('preview', field.value);
}

// Enter submits (Reka's handler) and never types a line break, except
// while an input method is composing, where Enter picks the candidate.
function onTextareaEnter(event: KeyboardEvent) {
  if (!event.isComposing) event.preventDefault();
}

// One paragraph style for the wrapped display, the textarea and its sizer,
// so all three break their lines in the same places. A word longer than
// the line (a pasted URL, a serial) breaks anywhere instead of widening
// the page.
const MULTILINE_TEXT = 'leading-snug whitespace-pre-wrap [overflow-wrap:anywhere]';
const BOX = 'w-full font-semibold px-1 py-0.5 rounded-lg border-2';

const textSizeClasses = {
  sm: 'text-sm',
  base: 'text-base',
  lg: 'text-lg',
  xl: 'text-xl',
  '2xl': 'text-2xl',
};

// `maxLines` takes precedence over `truncate`. line-clamp-2 wraps to
// two lines with ellipsis past that, paired with leading-tight so two
// lines fit in a header row sized for one.
const clampClass = computed(() =>
  props.multiline
    ? MULTILINE_TEXT
    : props.maxLines === 2
    ? 'line-clamp-2 leading-tight break-words'
    : props.maxLines === 1 || props.truncate
      ? 'truncate'
      : 'break-words',
);
const resolvedPlaceholder = computed(() => props.placeholder ?? fluent.$t('inline-edit-placeholder'));
const accessibleName = computed(() => props.label ?? resolvedPlaceholder.value);
</script>

<template>
  <div class="flex items-center gap-3 group flex-1 min-w-0">
    <span
      v-if="prefix"
      class="text-tertiary font-medium flex items-center select-none flex-shrink-0"
      :class="[textSizeClasses[textSize], { 'opacity-50': isEditing }]"
    >
      {{ prefix }}
    </span>

    <EditableRoot
      :model-value="held"
      :placeholder="resolvedPlaceholder"
      :activation-mode="canEdit ? 'focus' : 'none'"
      submit-mode="both"
      select-on-focus
      class="flex-1 relative min-w-0"
      @update:model-value="onSubmit"
      @update:state="onState"
    >
      <EditableArea class="w-full">
        <EditablePreview as-child>
          <component
            :is="as"
            class="w-full font-semibold px-1 py-0.5 rounded-lg transition-all duration-150 border-2 border-transparent outline-none focus-visible:ring-2 focus-visible:ring-accent"
            :class="[
              textSizeClasses[textSize],
              clampClass,
              canEdit ? 'cursor-pointer hover:bg-surface-hover' : 'cursor-default',
              held ? 'text-primary' : 'text-tertiary italic',
            ]"
            :tabindex="canEdit ? 0 : -1"
            :title="(maxLines || truncate) && held ? held : undefined"
          >
            {{ held || resolvedPlaceholder }}
          </component>
        </EditablePreview>
        <!-- The textarea takes its size from a hidden copy of its text in
             the same grid cell: no measuring, and the same box as the
             display it replaced. -->
        <div v-if="multiline" v-show="isEditing" class="grid">
          <div
            aria-hidden="true"
            class="invisible col-start-1 row-start-1 border-transparent"
            :class="[BOX, textSizeClasses[textSize], MULTILINE_TEXT]"
          >{{ draft || resolvedPlaceholder }}&#8203;</div>
          <EditableInput as-child>
            <textarea
              ref="textarea"
              rows="1"
              enterkeyhint="done"
              autocapitalize="sentences"
              :aria-label="accessibleName"
              class="col-start-1 row-start-1 h-full resize-none overflow-hidden bg-surface text-primary focus:outline-none border-accent/50 placeholder:text-tertiary placeholder:italic cursor-text"
              :class="[BOX, textSizeClasses[textSize], MULTILINE_TEXT]"
              @keydown.enter="onTextareaEnter"
              @input="onTextareaInput"
            />
          </EditableInput>
        </div>
        <EditableInput v-else as-child>
          <input
            type="text"
            :aria-label="accessibleName"
            class="w-full bg-surface-hover text-primary font-semibold px-1 py-0.5 rounded-lg focus:bg-surface focus:outline-none transition-all duration-150 border-2 border-transparent focus:border-accent/50 cursor-text"
            :class="[textSizeClasses[textSize], textSize === 'sm' && 'pointer-coarse:text-base']"
            @input="onInput"
          />
        </EditableInput>
      </EditableArea>

      <!-- Edit hint, pointer only (the preview's focus ring covers keyboard). -->
      <span
        v-if="!isEditing && canEdit && showEditHint && !multiline"
        class="absolute right-3 top-1/2 -translate-y-1/2 text-tertiary text-sm opacity-0 group-hover:opacity-100 transition-opacity duration-200 pointer-events-none select-none"
        aria-hidden="true"
      >
        {{ fluent.$t('inline-edit-hint') }}
      </span>
    </EditableRoot>
  </div>
</template>

<style scoped>
@media (prefers-reduced-motion: reduce) {
  .transition-all {
    transition: opacity 0.1s ease-in-out;
    transform: none;
  }
}
</style>
