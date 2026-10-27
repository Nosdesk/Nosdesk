<script setup lang="ts">
/** Four count tiles: two across on a phone, four on a wider page. */
defineProps<{
  tiles: { label: string; value: number; sub?: string }[]
}>()
</script>

<template>
  <div class="grid grid-cols-2 sm:grid-cols-4 gap-3" data-test="workspace-import-tiles">
    <div
      v-for="tile in tiles"
      :key="tile.label"
      class="flex flex-col gap-1 min-w-0 rounded-xl border border-default bg-surface px-3.5 py-3"
    >
      <span class="text-2xs uppercase tracking-wide text-tertiary truncate">{{ tile.label }}</span>
      <span class="text-xl font-semibold text-primary leading-tight">{{ tile.value.toLocaleString() }}</span>
      <span v-if="tile.sub" class="text-xs text-tertiary truncate">{{ tile.sub }}</span>
    </div>
  </div>
</template>
