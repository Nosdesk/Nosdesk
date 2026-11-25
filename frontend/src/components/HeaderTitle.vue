<!-- HeaderTitle.vue -->
<script setup lang="ts">
import { ref, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import type { FluentVariable } from '@fluent/bundle';
import InlineEdit from '@/components/common/InlineEdit.vue';

const fluent = useFluent();
const t = (k: string, args?: Record<string, FluentVariable>) => fluent.$t(k, args);

interface Props {
  initialTitle: string;
  prefix?: string;
  placeholderText?: string;
  truncate?: boolean;
  /** Cap display to N lines, ellipsis past that. See InlineEdit. */
  maxLines?: 1 | 2;
  /** The title as the page's heading on a phone: an `h1` that wraps
   *  without limit and edits in place. See InlineEdit. */
  pageHeading?: boolean;
  /** What a screen reader hears for an empty title, in place of the
   *  placeholder shown. See InlineEdit. */
  emptyLabel?: string;
}

const props = withDefaults(defineProps<Props>(), {
  truncate: false,
  pageHeading: false,
});
const emit = defineEmits(['updateTitle', 'updateTitlePreview']);

// Use a ref that syncs with the prop for immediate reactivity
const displayTitle = ref(props.initialTitle);

// Watch for prop changes and update immediately (this handles SSE updates)
watch(() => props.initialTitle, (newTitle) => {
  displayTitle.value = newTitle;
}, { immediate: true });

// Commit (blur / Enter): persist the new title. InlineEdit only emits this
// on a real change (its localValue vs the pre-edit snapshot), so it is the
// single client-side change gate. Forward unconditionally rather than
// re-diffing against initialTitle, which tracks the live preview/SSE display
// value and so would wrongly suppress a genuine commit.
const handleTitleUpdate = (newValue: string) => {
  emit('updateTitle', newValue);
};

// Transient draft (per keystroke): live display + optional SSE preview,
// no persistence.
const handleTitlePreview = (newValue: string) => {
  displayTitle.value = newValue;
  emit('updateTitlePreview', newValue);
};
</script>

<template>
  <InlineEdit
    :modelValue="displayTitle"
    :prefix="prefix"
    :placeholder="placeholderText || t('ui-header-title-placeholder')"
    text-size="xl"
    :truncate="truncate"
    :max-lines="maxLines"
    :multiline="pageHeading"
    :as="pageHeading ? 'h1' : 'div'"
    :empty-label="emptyLabel"
    @update:modelValue="handleTitleUpdate"
    @preview="handleTitlePreview"
  />
</template>