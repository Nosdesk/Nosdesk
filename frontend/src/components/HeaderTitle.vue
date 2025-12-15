<!-- HeaderTitle.vue -->
<script setup lang="ts">
import { ref, watch, nextTick } from 'vue';
import InlineEdit from '@/components/common/InlineEdit.vue';

interface Props {
  initialTitle: string;
  prefix?: string;
  placeholderText?: string;
  truncate?: boolean;
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
    :placeholder="placeholderText || 'Enter title...'"
    text-size="xl"
    :truncate="truncate"
    @update:modelValue="handleTitleUpdate"
  />
</template>