<script setup lang="ts">
/**
 * Tab strip across the top of the asset section. Lets the
 * user flip between the two views that share the underlying
 * asset data (inventory list, capacity planner) without
 * leaning on the sidebar — the planner used to be a sidebar
 * sibling of `/assets`, which highlighted both when the planner
 * was open. Routes stay the same so deep links keep working.
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
  { to: '/assets/planner', labelKey: 'asset-tabs-planner', exact: true },
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
  <nav
    class="flex items-center gap-1 px-2 pt-2 border-b border-default bg-surface"
    aria-label="Asset section views"
  >
    <RouterLink
      v-for="tab in tabs"
      :key="tab.to"
      :to="tab.to"
      class="px-3 py-2 text-sm font-medium border-b-2 -mb-px transition-colors"
      :class="isActive(tab)
        ? 'text-primary border-accent'
        : 'text-secondary border-transparent hover:text-primary hover:border-default'"
    >
      {{ $t(tab.labelKey) }}
    </RouterLink>
  </nav>
</template>
