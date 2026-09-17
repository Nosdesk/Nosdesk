<script setup lang="ts">
/**
 * Tab strip across the top of the asset section. Flips between the
 * views that share the underlying asset data (inventory list, make/
 * model catalog) without leaning on the sidebar. The fleet-planning
 * lenses that used to be a separate "Planner" view now live inside the
 * inventory list as group-by axes.
 */
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import TabBar, { type TabBarItem } from '@/components/common/TabBar.vue'

const route = useRoute()
const router = useRouter()
const fluent = useFluent()

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

const activeTo = computed(() => tabs.find(isActive)?.to ?? tabs[0].to)
const tabItems = computed<TabBarItem[]>(() =>
  tabs.map((tab) => ({ value: tab.to, label: fluent.$t(tab.labelKey) })),
)
</script>

<template>
  <!-- Segmented-pill tab strip matching the tickets header's
       treatment; the tabs navigate rather than switch panels. -->
  <TabBar
    :model-value="activeTo"
    :items="tabItems"
    variant="pill"
    size="sm"
    :label="$t('asset-tabs-aria')"
    @update:model-value="(to) => to !== activeTo && router.push(to)"
  />
</template>
