<!--
Quiet placeholder for `KpiRail`: a static em-dash where each numeral
loads (matching KpiTile's own loading state), no pulsing skeleton, in
line with the house "render chrome immediately" style. Mirrors the
rail's responsive grid so the placeholder → data swap produces no
layout shift.
-->
<script setup lang="ts">
withDefaults(defineProps<{ count?: number }>(), { count: 4 })
</script>

<template>
  <div class="@container flex-1 min-h-0 h-full w-full">
    <div
      class="grid h-full min-h-0 grid-cols-2 @sm:grid-cols-4 auto-rows-fr gap-px bg-default"
    >
      <div
        v-for="i in count"
        :key="i"
        class="flex min-h-0 min-w-0 flex-col bg-surface px-2 py-2 sm:px-3"
      >
        <div class="h-5 shrink-0 flex items-center">
          <span class="text-tertiary tabular-nums leading-none" aria-hidden="true">&mdash;</span>
        </div>
        <span class="flex-1 min-h-0" aria-hidden="true" />
        <div class="h-2 w-12 shrink-0 rounded bg-surface-alt" />
      </div>
    </div>
  </div>
</template>
