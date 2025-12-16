<script setup lang="ts">
defineProps<{
  description?: string;
}>();
</script>

<template>
  <div class="p-4 bg-surface-alt rounded-xl border border-default mb-4">
    <p class="font-medium text-primary">Configuration via Environment Variables</p>
    <p class="text-sm text-secondary mt-1">
      <slot>
        Settings are configured through environment variables in your
        <code class="bg-surface px-1 rounded text-primary">.env</code> file or Docker environment.
      </slot>
    </p>
  </div>
</template>
