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
submits, and a pasted line break becomes a space. The caret starts at the
end rather than selecting the title, a plain blur saves (a phone's keyboard
Done, which Reka's dismiss layers don't see), and when the edit ends the
heading takes focus back without reopening the editor.
-->
<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, useTemplateRef, watch } from 'vue';
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
  /** The display's accessible name while the value is empty, in place of
   *  the placeholder it shows. */
  emptyLabel?: string;
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
  emptyLabel: undefined,
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
const root = useTemplateRef<InstanceType<typeof EditableRoot>>('root');
const wrapper = useTemplateRef<HTMLElement>('wrapper');

// A long title selected whole is one stray key from gone, and a phone has
// no Escape, so the multiline editor opens with the caret at the end.
let caretToEnd = false;
// Set for the one focus that hands the heading its focus back, which would
// otherwise reopen the editor (activation on focus).
let returningFocus = false;

function onState(state: 'edit' | 'submit' | 'cancel') {
  if (state === 'edit') {
    originalValue.value = props.modelValue;
    held.value = props.modelValue;
    draft.value = props.modelValue;
    isEditing.value = true;
    caretToEnd = props.multiline;
    return;
  }
  isEditing.value = false;
  held.value = props.modelValue;
  if (state === 'cancel') emit('preview', originalValue.value);
  if (props.multiline) returnFocus();
}

// The edit ended with focus still in the field, or nowhere (Enter, Escape,
// a keyboard dismissed): the heading takes it back and is read again. Focus
// that went to something else stays there.
function returnFocus() {
  const active = document.activeElement;
  if (active && active !== textarea.value && active !== document.body) return;
  void nextTick(() => {
    const heading = wrapper.value?.querySelector<HTMLElement>('[data-inline-edit-display]');
    if (!heading || isEditing.value) return;
    returningFocus = true;
    try {
      heading.focus({ preventScroll: true });
    } finally {
      returningFocus = false;
    }
  });
}

function onFocusinCapture(event: FocusEvent) {
  if (returningFocus) event.stopPropagation();
}

// A tap on the heading that already has focus (handed back above) fires no
// focus, so the tap itself opens the editor.
function onDisplayClick() {
  if (props.multiline && props.canEdit && !isEditing.value) root.value?.edit();
}

function onTextareaFocus(event: FocusEvent) {
  if (!caretToEnd) return;
  caretToEnd = false;
  const field = event.target as HTMLTextAreaElement;
  field.setSelectionRange(field.value.length, field.value.length);
}

// Reka submits on a pointer down outside or focus moving elsewhere; a blur
// to nowhere (iOS's keyboard Done, the keyboard dismissed) gets neither.
// Removing a focused field blurs it in some engines: an edit whose owner is
// going away (its subject changed) is dropped, not saved.
let unmounting = false;
onBeforeUnmount(() => {
  unmounting = true;
});
function onTextareaBlur(event: FocusEvent) {
  if (event.relatedTarget === null && isEditing.value && !unmounting) root.value?.submit();
}

// Reka submits unconditionally; only a real change reaches the parent.
function onSubmit(raw: string) {
  const value = props.multiline ? oneLine(raw).trim() : raw;
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

// Enter while an input method is composing picks the candidate. Safari ends
// the composition before the keydown, which then only shows as keyCode 229.
const composing = (event: KeyboardEvent) => event.isComposing || event.keyCode === 229;

// Stops a composing Enter on its way down, before Reka's own handler on the
// field submits.
function onKeydownCapture(event: KeyboardEvent) {
  if (event.key === 'Enter' && composing(event)) event.stopPropagation();
}

// Any other Enter submits (Reka's handler) and never types a line break.
function onTextareaEnter(event: KeyboardEvent) {
  if (!composing(event)) event.preventDefault();
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
  <div ref="wrapper" class="flex items-center gap-3 group flex-1 min-w-0" @focusin.capture="onFocusinCapture">
    <span
      v-if="prefix"
      class="text-tertiary font-medium flex items-center select-none flex-shrink-0"
      :class="[textSizeClasses[textSize], { 'opacity-50': isEditing }]"
    >
      {{ prefix }}
    </span>

    <EditableRoot
      ref="root"
      :model-value="held"
      :placeholder="resolvedPlaceholder"
      :activation-mode="canEdit ? 'focus' : 'none'"
      submit-mode="both"
      :select-on-focus="!multiline"
      class="flex-1 relative min-w-0"
      @update:model-value="onSubmit"
      @update:state="onState"
    >
      <EditableArea class="w-full">
        <EditablePreview as-child>
          <component
            :is="as"
            class="w-full font-semibold px-1 py-0.5 rounded-lg transition-all duration-150 border-2 border-transparent outline-none focus-visible:ring-2 focus-visible:ring-accent/50"
            :class="[
              textSizeClasses[textSize],
              clampClass,
              canEdit ? 'cursor-pointer hover:bg-surface-hover' : 'cursor-default',
              held ? 'text-primary' : 'text-tertiary italic',
            ]"
            :tabindex="canEdit ? 0 : -1"
            :title="!multiline && (maxLines || truncate) && held ? held : undefined"
            :aria-label="!held && emptyLabel ? emptyLabel : undefined"
            data-inline-edit-display
            @click="onDisplayClick"
          >
            {{ held || resolvedPlaceholder }}
          </component>
        </EditablePreview>
        <!-- The textarea takes its size from a hidden copy of its text in
             the same grid cell: no measuring, and the same box as the
             display it replaced. -->
        <div v-if="multiline" v-show="isEditing" class="grid" @keydown.capture="onKeydownCapture">
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
              @focus="onTextareaFocus"
              @blur="onTextareaBlur"
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
