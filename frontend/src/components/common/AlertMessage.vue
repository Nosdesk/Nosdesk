<script setup lang="ts">
import Icon from '@/components/common/Icon.vue'

defineProps<{
  type: 'success' | 'error';
  message: string;
}>();
</script>

<template>
  <div
    class="p-4 rounded-xl border mb-4 flex items-start"
    :class="type === 'success'
      ? 'bg-status-success/30 text-status-success border-status-success/50'
      : 'bg-status-error/30 text-status-error border-status-error/50'"
  >
    <div class="mr-3 mt-0.5 flex-shrink-0">
      <Icon v-if="type === 'success'" name="checkCircle" size="md" />
      <Icon v-else name="exclamationCircle" size="md" />
    </div>
    <div>{{ message }}</div>
  </div>
</template>
