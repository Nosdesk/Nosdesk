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
}

const props = withDefaults(defineProps<Props>(), {
  truncate: false
});
const emit = defineEmits(['updateTitle', 'updateTitlePreview']);

// Use a ref that syncs with the prop for immediate reactivity
const displayTitle = ref(props.initialTitle);

// Watch for prop changes and update immediately (this handles SSE updates)
watch(() => props.initialTitle, (newTitle) => {
  displayTitle.value = newTitle;
}, { immediate: true });

// Handle title updates
const handleTitleUpdate = (newValue: string) => {
  if (newValue !== props.initialTitle) {
    emit('updateTitle', newValue);
  }
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
    @update:modelValue="handleTitleUpdate"
  />
</template>