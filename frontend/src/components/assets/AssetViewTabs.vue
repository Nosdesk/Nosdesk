<script setup lang="ts">
/**
 * Tab strip across the top of the asset section. Flips between the
 * views that share the underlying asset data (inventory list, make/
 * model catalog) without leaning on the sidebar. The fleet-planning
 * lenses that used to be a separate "Planner" view now live inside the
 * inventory list as group-by axes.
 */
import { RouterLink, useRoute } from 'vue-router'

const route = useRoute()

interface Tab {
  to: string
  labelKey: string
  /** Active when route.path equals this string. Sub-routes
   *  under the asset detail page (e.g. /assets/:id) should
   *  fall under "Inventory" rather than highlighting nothing,
   *  so the inventory tab uses a prefix-match below. */
  exact?: boolean
}

const tabs: Tab[] = [
  { to: '/assets', labelKey: 'asset-tabs-inventory' },
  { to: '/assets/catalog', labelKey: 'asset-tabs-catalog', exact: true },
  { to: '/assets/groups', labelKey: 'asset-tabs-groups', exact: true },
]

function isActive(tab: Tab): boolean {
  if (tab.exact) return route.path === tab.to
  // For non-exact tabs (e.g. inventory), match the prefix but
  // exclude paths that another tab claims via `exact`.
  if (!route.path.startsWith(tab.to)) return false
  return !tabs.some((other) => other !== tab && other.exact && route.path === other.to)
}
</script>

<template>
  <!-- Segmented-pill tab strip matching the tickets header's
       TicketsViewTabs treatment: a small rounded group with the
       active tab on `bg-surface shadow-sm` and inactive tabs on
       `text-secondary`. Sits inline with the filter chrome
       rather than dominating its own band the way a heavy
       underline-style tab strip would. -->
  <div
    class="inline-flex items-center gap-0.5 rounded-md bg-surface-alt p-0.5"
    role="tablist"
    aria-label="Asset section views"
  >
    <RouterLink
      v-for="tab in tabs"
      :key="tab.to"
      :to="tab.to"
      role="tab"
      :aria-selected="isActive(tab)"
      class="inline-flex items-center px-2.5 h-7 rounded text-sm font-medium transition-colors whitespace-nowrap shrink-0"
      :class="isActive(tab)
        ? 'bg-surface text-primary shadow-sm'
        : 'text-secondary hover:text-primary hover:bg-surface/60'"
    >
      {{ $t(tab.labelKey) }}
    </RouterLink>
  </div>
</template>
