<!--
Quiet placeholder for the volume KPI rail: a static em-dash per numeral
(matching KpiTile's loading state), no pulsing skeleton, in line with
the house "render chrome immediately" style. Same box heights as the
populated rail so the swap produces no layout shift.
-->
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
      <div class="h-5 shrink-0 flex items-center">
        <span class="text-tertiary tabular-nums leading-none" aria-hidden="true">&mdash;</span>
      </div>
      <div class="flex-1 min-h-2" />
      <div class="h-2.5 w-14 rounded bg-surface-alt shrink-0" />
    </div>
  </div>
</template>
