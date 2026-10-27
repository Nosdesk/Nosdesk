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
  /** Route names that light this tab. Matched by name, not by path:
   *  the hosted agent app prefixes every path with the workspace slug
   *  (/acme/assets/catalog), so a path literal never matches there. */
  routes: string[]
}

const tabs: Tab[] = [
  // The asset detail page (/assets/:id) falls under Inventory.
  { to: '/assets', labelKey: 'asset-tabs-inventory', routes: ['assets', 'asset-view'] },
  { to: '/assets/catalog', labelKey: 'asset-tabs-catalog', routes: ['asset-catalog'] },
  { to: '/assets/groups', labelKey: 'asset-tabs-groups', routes: ['asset-groups'] },
]

const activeTo = computed(() => {
  const name = typeof route.name === 'string' ? route.name : ''
  return tabs.find((tab) => tab.routes.includes(name))?.to ?? tabs[0].to
})
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
