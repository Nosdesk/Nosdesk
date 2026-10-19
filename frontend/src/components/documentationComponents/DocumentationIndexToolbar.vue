<script setup lang="ts">
/**
 * The documentation index band. Library, Drafts, Archived and Trash are
 * states of the same pages, so they lead as pill view tabs (the tickets and
 * asset headers use the same pattern) with their counts on the tabs. The
 * actions sit at the end: a search scoped to documentation, New collection,
 * and an overflow menu grouped as Knowledge, Publishing and Admin.
 *
 * The band renders on all four routes it switches between. The selected tab
 * comes from the route name and the tabs navigate by name, because the hosted
 * agent app prefixes every path with the workspace slug (/acme/documentation)
 * and a bare path literal never matches there.
 *
 * On a phone the tabs scroll inside the band (the page never scrolls
 * sideways), search drops because the bottom nav carries it, and New
 * collection folds into the menu sheet.
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { useRoute, useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { useAuthStore } from '@/stores/auth'
import { useWorkspacePortal } from '@/composables/useWorkspacePortal'
import { useSyncDocsStore, isActivePage } from '@nosdesk/core/sync/stores/documentation'
import { useGlobalSearch } from '@/composables/useGlobalSearch'
import { useDetectClustersMutation } from '@/composables/useKnowledgeGaps'
import { useMobileDetection } from '@/composables/useMobileDetection'
import { useToastStore } from '@nosdesk/core/stores/toast'
import Button from '@/components/common/Button.vue'
import IconButton from '@/components/common/IconButton.vue'
import TabBar, { type TabBarItem } from '@/components/common/TabBar.vue'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import MenuList, { type MenuItem } from '@/components/common/MenuList.vue'
import CollectionModal from '@/components/documentationComponents/CollectionModal.vue'
import { ICON_REGISTRY } from '@/components/common/icons'

export type DocsIndexTab =
  | 'documentation'
  | 'documentation-drafts'
  | 'documentation-archived'
  | 'documentation-trash'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const authStore = useAuthStore()
const { isAdmin, isTechnician } = storeToRefs(authStore)
const route = useRoute()
const router = useRouter()
const { portal, portalUrl } = useWorkspacePortal()
const docs = useSyncDocsStore()
const { openSearch } = useGlobalSearch()
const detectMutation = useDetectClustersMutation()
const toast = useToastStore()
// Below `sm` the app shows its bottom nav, which carries search.
const { isMobile: isPhone } = useMobileDetection('sm')

const menuOpen = ref(false)
const menuWrapRef = ref<HTMLElement | null>(null)
const showCreateCollection = ref(false)

const counts = computed(() => {
  let library = 0
  let drafts = 0
  let archived = 0
  let trash = 0
  for (const p of docs.allPages) {
    if (isActivePage(p)) {
      if (p.collection_id == null) drafts++
      else library++
    } else if (p.status === 'archived') archived++
    else if (p.status === 'deleted') trash++
  }
  return { library, drafts, archived, trash }
})

const TAB_NAMES: readonly DocsIndexTab[] = [
  'documentation',
  'documentation-drafts',
  'documentation-archived',
  'documentation-trash',
]

const activeTab = computed<DocsIndexTab>(() => {
  const name = typeof route.name === 'string' ? route.name : ''
  return (TAB_NAMES as readonly string[]).includes(name) ? (name as DocsIndexTab) : 'documentation'
})

// Archived and Trash show only when they hold pages, or while open, so
// restoring the last page there never leaves the strip with nothing selected.
const tabItems = computed<TabBarItem<DocsIndexTab>[]>(() => {
  const c = counts.value
  const items: TabBarItem<DocsIndexTab>[] = [
    { value: 'documentation', label: t('docs-index-tab-library'), badge: c.library || undefined },
    { value: 'documentation-drafts', label: t('docs-index-tab-drafts'), badge: c.drafts || undefined },
  ]
  if (c.archived > 0 || activeTab.value === 'documentation-archived') {
    items.push({ value: 'documentation-archived', label: t('docs-index-tab-archived'), badge: c.archived || undefined })
  }
  if (c.trash > 0 || activeTab.value === 'documentation-trash') {
    items.push({ value: 'documentation-trash', label: t('docs-index-tab-trash'), badge: c.trash || undefined })
  }
  return items
})

function selectTab(name: DocsIndexTab) {
  if (name === activeTab.value) return
  void router.push({ name })
}

const publicDocsEnabled = computed(() => portal.value?.public_docs_enabled === true)

const menuAnchor = computed(() => ({
  type: 'element' as const,
  element: () => menuWrapRef.value?.querySelector('button') ?? null,
}))

const menuItems = computed((): MenuItem[] => {
  const items: MenuItem[] = []
  const group = (id: string, label: string) => {
    items.push({ id, label, heading: true, divider: items.length > 0 })
  }

  if (isPhone.value) {
    items.push({ id: 'new-collection', label: t('docs-index-toolbar-new-collection'), icon: ICON_REGISTRY.add.d })
  }

  if (isTechnician.value) {
    group('knowledge-heading', t('docs-index-toolbar-knowledge-heading'))
    items.push({ id: 'scan-gaps', label: t('docs-index-toolbar-scan-gaps'), icon: ICON_REGISTRY.search.d })
  }

  if (publicDocsEnabled.value) {
    group('publish-heading', t('docs-index-toolbar-publish-heading'))
    items.push({ id: 'public-site', label: t('docs-index-toolbar-public-site'), icon: ICON_REGISTRY.link.d })
  }

  if (isAdmin.value) {
    group('admin-heading', t('docs-index-toolbar-admin-heading'))
    items.push({ id: 'guest-settings', label: t('docs-index-toolbar-guest-settings'), icon: ICON_REGISTRY.settings.d })
  }

  return items
})

function openDocSearch() {
  openSearch('documentation')
}

function closeMenu() {
  menuOpen.value = false
}

function toggleMenu() {
  menuOpen.value = !menuOpen.value
}

async function runGapScan() {
  try {
    const result = await detectMutation.mutateAsync(undefined)
    if (!result) return
    if (result.gaps_created === 0 && result.gaps_updated === 0) {
      toast.info(t('docs-index-toolbar-scan-no-results'))
      return
    }
    toast.success(
      t('docs-index-toolbar-scan-success', {
        created: result.gaps_created,
        updated: result.gaps_updated,
      }),
    )
  } catch {
    toast.error(t('docs-index-toolbar-scan-error'))
  }
}

function handleMenuSelect(id: string) {
  closeMenu()
  switch (id) {
    case 'new-collection':
      showCreateCollection.value = true
      break
    case 'scan-gaps':
      void runGapScan()
      break
    case 'public-site':
      window.open(portalUrl('/docs'), '_blank', 'noopener,noreferrer')
      break
    case 'guest-settings':
      void router.push({ name: 'admin-guest-access' })
      break
  }
}

// The new collection reaches the Library through the sync pool.
function onCollectionCreated() {
  showCreateCollection.value = false
}

// On a phone the tabs may overflow; fade the trailing edge only then, so a
// strip that fits doesn't look cut off.
const tabsWrapRef = ref<HTMLElement | null>(null)
const tabsOverflow = ref(false)
let resizeObserver: ResizeObserver | null = null

function tabList(): HTMLElement | null {
  return tabsWrapRef.value?.querySelector<HTMLElement>('[role="tablist"]') ?? null
}

function measureTabs() {
  const list = tabList()
  tabsOverflow.value = !!list && list.scrollWidth > list.clientWidth + 1
}

onMounted(() => {
  measureTabs()
  const list = tabList()
  if (list && typeof ResizeObserver === 'function') {
    resizeObserver = new ResizeObserver(measureTabs)
    resizeObserver.observe(list)
  }
})
watch([tabItems, isPhone], () => requestAnimationFrame(measureTabs))
onBeforeUnmount(() => resizeObserver?.disconnect())

defineExpose({ openCreateCollection: () => (showCreateCollection.value = true) })
</script>

<template>
  <header class="shrink-0 border-b border-subtle bg-surface">
    <div class="@container px-4 sm:px-6 py-2 sm:py-2.5 mx-auto w-full max-w-8xl">
      <div class="flex items-center gap-2 sm:gap-3 min-w-0">
        <div
          ref="tabsWrapRef"
          class="min-w-0"
          :class="{ 'flex-1': isPhone, 'docs-index-tabs--fade': isPhone && tabsOverflow }"
        >
          <TabBar
            :model-value="activeTab"
            :items="tabItems"
            variant="pill"
            size="sm"
            :label="$t('docs-index-tabs-aria')"
            list-class="overflow-x-auto scrollbar-hide"
            @update:model-value="selectTab"
          >
            <template #item="{ item, active }">
              <span>{{ item.label }}</span>
              <span
                v-if="item.badge !== undefined"
                class="text-2xs tabular-nums"
                :class="active ? 'text-secondary' : 'text-tertiary'"
                data-testid="docs-tab-count"
              >
                {{ item.badge }}
              </span>
            </template>
          </TabBar>
        </div>

        <div class="ml-auto flex items-center gap-2 shrink-0">
          <Button
            v-if="!isPhone"
            size="sm"
            variant="ghost"
            icon="search"
            :aria-label="$t('docs-index-toolbar-search')"
            data-testid="docs-index-search"
            @click="openDocSearch"
          >
            <span class="hidden @2xl:inline">{{ $t('docs-index-toolbar-search-button') }}</span>
          </Button>

          <Button
            v-if="!isPhone"
            size="sm"
            variant="secondary"
            icon="add"
            data-testid="docs-index-new-collection"
            @click="showCreateCollection = true"
          >
            {{ $t('docs-index-toolbar-new-collection') }}
          </Button>

          <div v-if="menuItems.length > 0" ref="menuWrapRef" class="relative shrink-0">
            <IconButton
              icon="more"
              size="sm"
              :label="$t('docs-index-toolbar-more-actions')"
              :tooltip="false"
              :aria-expanded="menuOpen"
              aria-haspopup="menu"
              data-testid="docs-index-more"
              @click="toggleMenu"
            />

            <ResponsiveMenu
              :open="menuOpen"
              :anchor="menuAnchor"
              :title="$t('docs-index-toolbar-more-aria')"
              placement="bottom-end"
              react-to-scroll="reposition"
              role="menu"
              :auto-focus="false"
              popover-class="bg-surface border border-default rounded-lg shadow-lg py-1 min-w-[13rem]"
              @close="closeMenu"
            >
              <MenuList :items="menuItems" @select="handleMenuSelect" />
            </ResponsiveMenu>
          </div>
        </div>
      </div>
    </div>

    <CollectionModal
      mode="create"
      :show="showCreateCollection"
      @close="showCreateCollection = false"
      @created="onCollectionCreated"
    />
  </header>
</template>

<style scoped>
/* Phone: when the tabs overflow, fade the trailing edge so the strip reads
   as scrollable. */
.docs-index-tabs--fade :deep([role='tablist']) {
  mask-image: linear-gradient(to right, #000 calc(100% - 20px), transparent);
}
</style>
