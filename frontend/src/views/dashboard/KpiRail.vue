<!--
Shared KPI rail for stat widgets (SLA health, etc.). Each cell is a
drill-down link with a headline number and label. The rail fills the
shell body (`flex-1 min-h-0 h-full`) and cells stretch to the full
grid track height so 1-row widgets don't leave dead vertical space.

Layout uses container queries on the rail width: four columns when
there is room, 2×2 when the widget is narrow. Dividers are gap-px
tracks so wrapping doesn't fight `divide-x`.
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
  <div class="@container flex-1 min-h-0 h-full w-full">
    <div
      class="grid h-full min-h-0 grid-cols-2 @sm:grid-cols-4 auto-rows-fr gap-px bg-default"
    >
      <router-link
        v-for="stat in kpis"
        :key="stat.id ?? stat.label"
        :to="stat.to"
        :title="stat.description"
        class="group flex min-h-0 min-w-0 flex-col bg-surface px-2 py-2 sm:px-3 hover:bg-surface-hover transition-colors"
      >
        <span
          :class="[
            'shrink-0 text-xl font-semibold tabular-nums leading-none group-hover:text-accent transition-colors',
            stat.tone ?? 'text-primary',
          ]"
        >
          {{ stat.value }}
        </span>
        <span class="flex-1 min-h-0" aria-hidden="true" />
        <span
          class="shrink-0 text-[10px] font-medium uppercase tracking-wider text-tertiary truncate max-w-full"
        >
          {{ stat.label }}
        </span>
      </router-link>
    </div>
  </div>
</template>
