<script setup lang="ts">
withDefaults(defineProps<{ count?: number }>(), { count: 3 })
</script>

<template>
  <div class="flex-1 flex min-h-0 divide-x divide-default">
    <div
      v-for="i in count"
      :key="i"
      class="flex-1 flex flex-col min-h-0 min-w-0 px-2 py-2 sm:px-3 gap-1"
    >
      <div class="h-5 w-10 rounded bg-surface-alt animate-pulse shrink-0" />
      <div class="flex-1 min-h-2 rounded bg-surface-alt animate-pulse" />
      <div class="h-2.5 w-14 rounded bg-surface-alt animate-pulse shrink-0" />
    </div>
  </div>
</template>
