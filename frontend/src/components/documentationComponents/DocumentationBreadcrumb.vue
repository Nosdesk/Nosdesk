<script setup lang="ts">
import { ref, watch } from 'vue'
import { RouterLink } from 'vue-router'
import { getCollectionsForPage, getCollection } from '@/services/collectionService'
import type { CollectionPage } from '@/services/collectionService'
import { getAncestorChain } from '@/utils/treeUtils'
import { docUrl } from '@/utils/docUrl'

const props = defineProps<{
  pageId: number | string
  parentId: number | string | null
}>()

interface BreadcrumbItem {
  label: string
  to: string | null
  icon?: string | null
}

const breadcrumbs = ref<BreadcrumbItem[]>([])
const loading = ref(false)

const buildBreadcrumbs = async () => {
  if (!props.pageId) {
    breadcrumbs.value = []
    return
  }

  loading.value = true

  try {
    const items: BreadcrumbItem[] = [
      { label: 'Documentation', to: '/documentation', icon: null }
    ]

    // Find a collection this page belongs to
    const collections = await getCollectionsForPage(Number(props.pageId))

    if (collections.length > 0) {
      const collection = collections[0]
      items.push({
        label: collection.name,
        to: `/documentation/collections/${collection.slug}`,
        icon: collection.icon,
      })

      // Get full collection data to find ancestors
      const fullCollection = await getCollection(collection.id)
      if (fullCollection?.pages) {
        const ancestors = getAncestorChain(props.pageId, fullCollection.pages)
        for (const ancestor of ancestors) {
          items.push({
            label: ancestor.title,
            to: docUrl(ancestor),
            icon: ancestor.icon,
          })
        }
      }
    }

    breadcrumbs.value = items
  } catch (error) {
    breadcrumbs.value = [{ label: 'Documentation', to: '/documentation', icon: null }]
  } finally {
    loading.value = false
  }
}

watch(
  () => props.pageId,
  () => buildBreadcrumbs(),
  { immediate: true }
)
</script>

<template>
  <nav v-if="breadcrumbs.length > 0 && !loading" aria-label="Breadcrumb">
    <ol class="flex items-center flex-wrap gap-1.5 sm:gap-2 text-xs text-tertiary min-w-0">
      <li v-for="(item, index) in breadcrumbs" :key="index" class="flex items-center gap-1.5 sm:gap-2 min-w-0">
        <span v-if="index > 0" class="text-tertiary select-none">/</span>
        <RouterLink
          v-if="item.to"
          :to="item.to"
          class="flex items-center gap-1 hover:text-accent transition-colors min-w-0 max-w-[200px]"
          :title="item.label"
        >
          <span v-if="item.icon" class="flex-shrink-0 leading-none">{{ item.icon }}</span>
          <span class="truncate">{{ item.label }}</span>
        </RouterLink>
        <span v-else class="truncate max-w-[200px]" :title="item.label">{{ item.label }}</span>
      </li>
    </ol>
  </nav>
</template>
