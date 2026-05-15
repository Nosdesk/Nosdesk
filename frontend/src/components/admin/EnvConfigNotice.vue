<script setup lang="ts">
defineProps<{
  description?: string;
}>();
</script>

<template>
  <div class="p-4 bg-surface-alt rounded-xl border border-default mb-4">
    <p class="font-medium text-primary">{{ $t('admin-env-notice-title') }}</p>
    <p class="text-sm text-secondary mt-1">
      <slot>
        {{ $t('admin-env-notice-prefix') }}
        <code class="bg-surface px-1 rounded text-primary">.env</code>
        {{ $t('admin-env-notice-suffix') }}
      </slot>
    </p>
  </div>
</template>
