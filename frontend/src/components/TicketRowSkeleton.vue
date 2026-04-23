<!--
Row-shaped skeleton mirroring `TicketRow`'s layout so the swap from
skeleton to data produces no column-level shift.

Rendering a fixed number of rows (`count`, default 5) at the same
`h-10` height as the real rows means the card reserves the same
vertical space regardless of what the eventual fetch returns — see
the dashboard widget's `min-body-height="200px"` for why this matters
in a row-stretch grid.
-->
<script setup lang="ts">
withDefaults(defineProps<{ count?: number }>(), { count: 5 })
</script>

<template>
  <ul class="divide-y divide-default">
    <li
      v-for="i in count"
      :key="i"
      class="relative h-10 flex items-center gap-3 pl-4 pr-3"
    >
      <span aria-hidden="true" class="absolute left-0 top-0 bottom-0 w-[3px] bg-surface-alt" />
      <span class="h-2.5 w-6 rounded bg-surface-alt animate-pulse" />
      <span
        class="h-2.5 rounded bg-surface-alt animate-pulse flex-1"
        :style="{ maxWidth: `${72 - (i % 5) * 8}%` }"
      />
      <span class="h-5 w-5 rounded-full bg-surface-alt animate-pulse" />
      <span class="h-2.5 w-6 rounded bg-surface-alt animate-pulse" />
    </li>
  </ul>
</template>
