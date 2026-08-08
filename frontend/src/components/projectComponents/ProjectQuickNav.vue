<script setup lang="ts">
/**
 * Per-project deep links to the project's three view routes
 * (Board / Gantt / Cycles). Low-emphasis until hovered. Clicks are
 * stopped from bubbling so they navigate to the sub-route instead of
 * triggering the surrounding row/card's open-project handler.
 */
defineProps<{ projectId: number | string }>()

const links = [
  { suffix: '', labelKey: 'views-project-tab-board' },
  { suffix: '/gantt', labelKey: 'views-project-tab-gantt' },
  { suffix: '/cycles', labelKey: 'views-project-tab-cycles' },
] as const
</script>

<template>
  <nav class="flex items-center gap-1 text-xs" @click.stop>
    <RouterLink
      v-for="link in links"
      :key="link.suffix"
      :to="`/projects/${projectId}${link.suffix}`"
      class="rounded px-1.5 py-0.5 min-h-[44px] sm:min-h-0 inline-flex items-center text-tertiary transition-colors hover:bg-surface-hover hover:text-accent focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
    >
      {{ $t(link.labelKey) }}
    </RouterLink>
  </nav>
</template>
