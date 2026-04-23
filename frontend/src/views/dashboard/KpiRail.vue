<!--
Shared horizontal KPI rail for stat widgets. Each entry renders as a
divided column with a large number above a small label, links to a
drill-down page on click. Layout + typography live here so the three
stat widgets stay visually locked; their only job is to supply the
`kpis` array.
-->
<script setup lang="ts">
export interface Kpi {
  /** Unique key. Falls back to `label` if omitted. */
  id?: string
  label: string
  value: number
  to: string
  /** Tailwind text-color class on the number. Defaults to `text-primary`. */
  tone?: string
  /** Optional native-tooltip copy for the whole cell. */
  description?: string
}

defineProps<{
  kpis: Kpi[]
}>()
</script>

<template>
  <div class="flex-1 flex items-center divide-x divide-default">
    <router-link
      v-for="stat in kpis"
      :key="stat.id ?? stat.label"
      :to="stat.to"
      :title="stat.description"
      class="flex-1 px-2 py-3 flex flex-col items-center justify-center hover:bg-surface-hover transition-colors group min-w-0"
    >
      <span
        :class="[
          'text-xl font-semibold tabular-nums leading-none group-hover:text-accent transition-colors',
          stat.tone ?? 'text-primary',
        ]"
      >
        {{ stat.value }}
      </span>
      <span class="mt-1.5 text-[10px] font-medium uppercase tracking-wider text-tertiary truncate max-w-full">
        {{ stat.label }}
      </span>
    </router-link>
  </div>
</template>
