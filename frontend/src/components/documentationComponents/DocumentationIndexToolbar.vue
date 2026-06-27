<script setup lang="ts">
/**
 * Documentation hub toolbar. Surfaces actions that aren't already
 * represented as browse sections below — scoped search, collection
 * creation, and an overflow menu for maintenance / admin tasks.
 */
import { computed, onMounted, ref } from 'vue'
import { storeToRefs } from 'pinia'
import { useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { useAuthStore } from '@/stores/auth'
import { usePublicSettingsStore } from '@nosdesk/core/stores/publicSettings'
import { useSyncDocsStore } from '@/sync/stores/documentation'
import { useGlobalSearch } from '@/composables/useGlobalSearch'
import { useDetectClustersMutation } from '@/composables/useKnowledgeGaps'
import { useToastStore } from '@nosdesk/core/stores/toast'
import Button from '@/components/common/Button.vue'
import Icon from '@/components/common/Icon.vue'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import MenuList, { type MenuItem } from '@/components/common/MenuList.vue'
import { ICON_REGISTRY } from '@/components/common/icons'

const emit = defineEmits<{
  (e: 'create-collection'): void
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const authStore = useAuthStore()
const { isAdmin, isTechnician } = storeToRefs(authStore)
const router = useRouter()
const publicSettings = usePublicSettingsStore()
const docs = useSyncDocsStore()
const { openSearch } = useGlobalSearch()
const detectMutation = useDetectClustersMutation()
const toast = useToastStore()

const menuOpen = ref(false)
const menuTriggerRef = ref<HTMLElement | null>(null)

const archivedCount = computed(
  () => docs.allPages.filter((p) => p.status === 'archived').length,
)
const trashCount = computed(
  () => docs.allPages.filter((p) => p.status === 'deleted').length,
)

const publicDocsEnabled = computed(
  () => publicSettings.settings?.guest_public_docs_enabled === true,
)

const menuAnchor = computed(() => ({
  type: 'element' as const,
  element: () => menuTriggerRef.value,
}))

const menuItems = computed((): MenuItem[] => {
  const items: MenuItem[] = []

  if (isTechnician.value) {
    items.push({
      id: 'scan-gaps',
      label: t('docs-index-toolbar-scan-gaps'),
      icon: ICON_REGISTRY.search.d,
    })
  }

  if (archivedCount.value > 0 || trashCount.value > 0) {
    if (items.length > 0) {
      items.push({ id: 'maintenance-heading', label: t('docs-index-toolbar-maintenance-heading'), heading: true, divider: true })
    }

    if (archivedCount.value > 0) {
      items.push({
        id: 'archived',
        label: t('docs-index-toolbar-archived'),
        icon: ICON_REGISTRY.archive.d,
        trailing: String(archivedCount.value),
      })
    }

    if (trashCount.value > 0) {
      items.push({
        id: 'trash',
        label: t('docs-index-toolbar-trash'),
        icon: ICON_REGISTRY.trash.d,
        trailing: String(trashCount.value),
        danger: true,
      })
    }
  }

  if (publicDocsEnabled.value) {
    if (items.length > 0) {
      items.push({ id: 'publish-heading', label: t('docs-index-toolbar-publish-heading'), heading: true, divider: true })
    }

    items.push({
      id: 'public-site',
      label: t('docs-index-toolbar-public-site'),
      icon: ICON_REGISTRY.link.d,
    })
  }

  if (isAdmin.value) {
    if (items.length > 0) {
      items.push({ id: 'admin-heading', label: t('docs-index-toolbar-admin-heading'), heading: true, divider: true })
    }

    items.push({
      id: 'guest-settings',
      label: t('docs-index-toolbar-guest-settings'),
      icon: ICON_REGISTRY.settings.d,
    })
  }

  return items
})

const showMoreButton = computed(() => menuItems.value.length > 0)

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
    case 'scan-gaps':
      void runGapScan()
      break
    case 'archived':
      router.push('/documentation/archived')
      break
    case 'trash':
      router.push('/documentation/trash')
      break
    case 'public-site':
      window.open('/docs', '_blank', 'noopener,noreferrer')
      break
    case 'guest-settings':
      router.push('/admin/guest-access')
      break
  }
}

onMounted(() => {
  if (isAdmin.value || isTechnician.value) {
    void publicSettings.load()
  }
})
</script>

<template>
  <div class="flex items-center justify-between gap-4 w-full min-w-0">
    <div class="flex items-center gap-2 min-w-0">
      <Button
        size="sm"
        variant="secondary"
        icon="search"
        class="shrink-0"
        :aria-label="$t('docs-index-toolbar-search')"
        @click="openDocSearch"
      >
        {{ $t('docs-index-toolbar-search') }}
      </Button>

      <Button
        size="sm"
        variant="ghost"
        icon="add"
        class="shrink-0"
        @click="emit('create-collection')"
      >
        {{ $t('docs-index-toolbar-new-collection') }}
      </Button>
    </div>

    <div v-if="showMoreButton" class="relative shrink-0">
      <button
        ref="menuTriggerRef"
        type="button"
        class="inline-flex items-center gap-1.5 h-8 px-2.5 rounded-lg border border-default bg-surface-alt text-xs font-medium text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
        :class="{ 'bg-surface-hover text-primary': menuOpen }"
        :aria-label="$t('docs-index-toolbar-more-aria')"
        :aria-expanded="menuOpen"
        aria-haspopup="menu"
        @click="toggleMenu"
      >
        <Icon name="more" size="sm" aria-hidden="true" />
        <span class="hidden sm:inline">{{ $t('docs-index-toolbar-more') }}</span>
      </button>

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
</template>
