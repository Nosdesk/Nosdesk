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
-->
<script setup lang="ts">
import { computed, ref, watch } from 'vue';
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
}

const props = withDefaults(defineProps<Props>(), {
  placeholder: 'Enter text...',
  textSize: 'base',
  canEdit: true,
  prefix: '',
  showEditHint: true,
  truncate: false,
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

function onState(state: 'edit' | 'submit' | 'cancel') {
  if (state === 'edit') {
    originalValue.value = props.modelValue;
    held.value = props.modelValue;
    isEditing.value = true;
    return;
  }
  isEditing.value = false;
  held.value = props.modelValue;
  if (state === 'cancel') emit('preview', originalValue.value);
}

// Reka submits unconditionally; only a real change reaches the parent.
function onSubmit(value: string) {
  if (value !== originalValue.value) emit('update:modelValue', value);
}

function onInput(event: Event) {
  emit('preview', (event.target as HTMLInputElement).value);
}

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
  props.maxLines === 2
    ? 'line-clamp-2 leading-tight break-words'
    : props.maxLines === 1 || props.truncate
      ? 'truncate'
      : 'break-words',
);
const accessibleName = computed(() => props.label ?? props.placeholder);
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
      :placeholder="placeholder"
      :activation-mode="canEdit ? 'focus' : 'none'"
      submit-mode="both"
      select-on-focus
      class="flex-1 relative min-w-0"
      @update:model-value="onSubmit"
      @update:state="onState"
    >
      <EditableArea class="w-full">
        <EditablePreview as-child>
          <div
            class="w-full font-semibold px-1 py-0.5 rounded-lg transition-all duration-150 border-2 border-transparent outline-none focus-visible:ring-2 focus-visible:ring-accent/50"
            :class="[
              textSizeClasses[textSize],
              clampClass,
              canEdit ? 'cursor-pointer hover:bg-surface-hover' : 'cursor-default',
              held ? 'text-primary' : 'text-tertiary italic',
            ]"
            :tabindex="canEdit ? 0 : -1"
            :title="(maxLines || truncate) && held ? held : undefined"
          >
            {{ held || placeholder }}
          </div>
        </EditablePreview>
        <EditableInput as-child>
          <input
            type="text"
            :aria-label="accessibleName"
            class="w-full bg-surface-hover text-primary font-semibold px-1 py-0.5 rounded-lg focus:bg-surface focus:outline-none transition-all duration-150 border-2 border-transparent focus:border-accent/50 cursor-text"
            :class="textSizeClasses[textSize]"
            @input="onInput"
          />
        </EditableInput>
      </EditableArea>

      <!-- Edit hint, pointer only (the preview's focus ring covers keyboard). -->
      <span
        v-if="!isEditing && canEdit && showEditHint"
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
