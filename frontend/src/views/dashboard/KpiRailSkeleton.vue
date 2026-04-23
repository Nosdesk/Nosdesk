<!--
Skeleton for `KpiRail`. Mirrors the rail's layout so skeleton → data
swap produces no column shift. `count` should match the caller's
expected KPI count so the placeholder column count matches the
eventual data column count.
-->
<script setup lang="ts">
withDefaults(defineProps<{ count?: number }>(), { count: 4 })
</script>

<template>
  <div class="flex-1 flex items-stretch divide-x divide-default">
    <div
      v-for="i in count"
      :key="i"
      class="flex-1 px-2 py-3 flex flex-col items-center justify-center gap-2"
    >
      <div class="h-5 w-8 rounded bg-surface-alt animate-pulse" />
      <div class="h-2 w-12 rounded bg-surface-alt animate-pulse" />
    </div>
  </div>
</template>
