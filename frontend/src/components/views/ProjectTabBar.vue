<script setup lang="ts">
/**
 * Segmented tab bar for the project's three view modes:
 * Board (kanban), Gantt (timeline), Cycles (iteration planning).
 *
 * Each tab is a real route so the URL is bookmarkable, browser
 * back/forward behave as you'd expect, and route-level
 * code-splitting keeps the initial bundle lean. The bar is
 * stateless — just renders links + active state from the
 * current route.
 *
 * Pattern follows Linear / Asana / Monday: a tight segmented
 * control sits below the page header, owns the view-mode
 * switch, and stays out of the header (which keeps the page
 * identity uncluttered).
 */
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'

const props = defineProps<{ projectId: number | string }>()

const route = useRoute()
const router = useRouter()

interface Tab {
  id: 'board' | 'gantt' | 'cycles'
  label: string
  to: string
}

const tabs = computed<Tab[]>(() => [
  { id: 'board', label: 'Board', to: `/projects/${props.projectId}` },
  { id: 'gantt', label: 'Gantt', to: `/projects/${props.projectId}/gantt` },
  { id: 'cycles', label: 'Cycles', to: `/projects/${props.projectId}/cycles` },
])

const activeId = computed<string>(() => {
  const path = route.path
  if (path.endsWith('/gantt')) return 'gantt'
  if (path.endsWith('/cycles')) return 'cycles'
  return 'board'
})

function go(tab: Tab): void {
  if (tab.id === activeId.value) return
  router.push(tab.to)
}
</script>

<template>
  <nav
    class="flex items-center gap-0.5 px-6 border-b border-subtle bg-app"
    role="tablist"
    aria-label="Project view"
  >
    <button
      v-for="tab in tabs"
      :key="tab.id"
      type="button"
      role="tab"
      :aria-selected="tab.id === activeId"
      class="text-sm font-medium px-3 py-2 -mb-px border-b-2 transition-colors"
      :class="tab.id === activeId
        ? 'text-primary border-accent'
        : 'text-tertiary border-transparent hover:text-secondary hover:border-subtle'"
      @click="go(tab)"
    >
      {{ tab.label }}
    </button>
  </nav>
</template>
