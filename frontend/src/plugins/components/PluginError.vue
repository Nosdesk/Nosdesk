<script setup lang="ts">
/**
 * Plugin Error Component
 *
 * Displayed when a plugin component fails to load.
 */
import { computed } from 'vue';
import { useFluent } from 'fluent-vue';

const fluent = useFluent();
const t = (key: string) => fluent.$t(key);

const props = defineProps<{
  error?: Error;
}>();

const errorMessage = computed(() => {
  if (!props.error) {
    return t('plugin-error-load-failed');
  }

  // Show user-friendly messages for common errors
  const message = props.error.message;

  if (message.includes('Community plugins cannot render')) {
    return t('plugin-error-pending-review');
  }

  if (message.includes('no uploaded bundle')) {
    return t('plugin-error-not-installed');
  }

  if (message.includes('Component not found')) {
    return t('plugin-error-component-not-found');
  }

  if (message.includes('timeout')) {
    return t('plugin-error-timeout');
  }

  // Generic error for other cases
  return t('plugin-error-failed');
});
</script>

<template>
  <div class="plugin-error p-3 flex items-center gap-2">
    <svg
      class="w-4 h-4 text-warning flex-shrink-0"
      fill="none"
      stroke="currentColor"
      viewBox="0 0 24 24"
    >
      <path
        stroke-linecap="round"
        stroke-linejoin="round"
        stroke-width="2"
        d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"
      />
    </svg>
    <span class="text-sm text-secondary">{{ errorMessage }}</span>
  </div>
</template>

<style scoped>
.plugin-error {
  background: var(--surface-alt);
  border: 1px solid var(--warning);
  border-radius: 0.5rem;
}
</style>
