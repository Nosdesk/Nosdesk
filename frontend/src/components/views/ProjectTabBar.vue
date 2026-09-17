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
import { useFluent } from 'fluent-vue'
import TabBar, { type TabBarItem } from '@/components/common/TabBar.vue'

const props = defineProps<{ projectId: number | string }>()

const route = useRoute()
const router = useRouter()
const fluent = useFluent()

interface Tab {
  id: 'board' | 'gantt' | 'cycles'
  label: string
  to: string
}

const tabs = computed<Tab[]>(() => [
  { id: 'board', label: fluent.$t('views-project-tab-board'), to: `/projects/${props.projectId}` },
  { id: 'gantt', label: fluent.$t('views-project-tab-gantt'), to: `/projects/${props.projectId}/gantt` },
  { id: 'cycles', label: fluent.$t('views-project-tab-cycles'), to: `/projects/${props.projectId}/cycles` },
])

const activeId = computed<string>(() => {
  const path = route.path
  if (path.endsWith('/gantt')) return 'gantt'
  if (path.endsWith('/cycles')) return 'cycles'
  return 'board'
})

const tabItems = computed<TabBarItem[]>(() =>
  tabs.value.map((tab) => ({ value: tab.id, label: tab.label })),
)

function go(id: string): void {
  const tab = tabs.value.find((t) => t.id === id)
  if (!tab || tab.id === activeId.value) return
  router.push(tab.to)
}
</script>

<template>
  <nav class="flex flex-wrap items-center justify-between gap-y-1 px-3 sm:px-6 border-b border-subtle bg-app">
    <TabBar
      :model-value="activeId"
      :items="tabItems"
      variant="underline"
      :label="$t('views-project-tab-aria')"
      list-class="border-b-0 -mb-px"
      @update:model-value="go"
    />

    <!-- View-shape controls (group-by, gantt viewport, …) ride the
         same row as the tabs, pinned to the right. -->
    <div v-if="$slots.actions" class="flex items-center gap-2">
      <slot name="actions" />
    </div>
  </nav>
</template>
