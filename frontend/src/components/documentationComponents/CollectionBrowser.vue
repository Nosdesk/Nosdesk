<script setup lang="ts">
/**
 * The documentation index's library: one ledger row per collection with its
 * top pages inline, so a known page is one click from the index instead of
 * a click into each collection. Everything derives from the sync pool, which
 * holds only what this reader may open: a collection they lose access to
 * leaves the Library as it leaves the sidebar.
 */
import { computed, ref } from 'vue'
import { RouterLink } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { reorderCollections } from '@nosdesk/core/services/collectionService'
import * as pool from '@nosdesk/core/sync/pool'
import {
  useSyncDocsStore,
  isActivePage,
  type DocCollectionRow,
  type DocPageRow,
} from '@nosdesk/core/sync/stores/documentation'
import { docUrl } from '@nosdesk/core/utils/docUrl'
import { formatRelativeTime } from '@nosdesk/core/utils/dateUtils'
import { useAuthStore } from '@/stores/auth'
import Icon from '@/components/common/Icon.vue'
import CollectionIcon from '@/components/documentationComponents/CollectionIcon.vue'

const emit = defineEmits<{
  (e: 'create'): void
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const authStore = useAuthStore()
const docs = useSyncDocsStore()

const dragIndex = ref<number | null>(null)
const dropIndex = ref<number | null>(null)

const canReorder = authStore.isTechnician

/** How many page links a ledger row carries before "N more". */
const TOP_PAGES = 3

interface LedgerDerived {
  topPages: DocPageRow[]
  moreCount: number
  updatedAt: string | null
}

/** Active pages per collection from the sync pool, newest first. */
const derivedByCollection = computed<Map<number, LedgerDerived>>(() => {
  const byCollection = new Map<number, DocPageRow[]>()
  for (const page of docs.allPages) {
    if (page.collection_id == null || !isActivePage(page)) continue
    const rows = byCollection.get(page.collection_id)
    if (rows) rows.push(page)
    else byCollection.set(page.collection_id, [page])
  }
  const out = new Map<number, LedgerDerived>()
  for (const [id, rows] of byCollection) {
    rows.sort((a, b) => (b.updated_at ?? '').localeCompare(a.updated_at ?? ''))
    out.set(id, {
      topPages: rows.slice(0, TOP_PAGES),
      moreCount: Math.max(rows.length - TOP_PAGES, 0),
      updatedAt: rows[0]?.updated_at ?? null,
    })
  }
  return out
})

const NO_PAGES: LedgerDerived = { topPages: [], moreCount: 0, updatedAt: null }

function derived(collection: DocCollectionRow): LedgerDerived {
  return derivedByCollection.value.get(collection.id) ?? NO_PAGES
}

const collections = computed(() => docs.collectionsSorted)

function pageCount(collection: DocCollectionRow): number {
  const ledger = derived(collection)
  return ledger.topPages.length + ledger.moreCount
}

/**
 * Who a restricted collection is open to. People are named from the pool;
 * one it no longer holds (their account was erased) is left out.
 */
function audienceLabel(collection: DocCollectionRow): string {
  // A server older than this client (mid rolling deploy) sends no grants;
  // the row still shows, as plainly restricted.
  const groups = collection.grants?.groups ?? []
  const users = (collection.grants?.users ?? [])
    .map((uuid) => pool.get<{ name: string }>('user', uuid))
    .filter((u) => u != null)

  if (groups.length === 0 && users.length === 0) {
    return t('docs-collection-browser-restricted-badge')
  }
  if (groups.length === 1 && users.length === 0) {
    return groups[0]!.name
  }
  if (groups.length === 0 && users.length === 1) {
    return users[0]!.name
  }
  if (groups.length > 0 && users.length === 0) {
    return t('docs-collection-browser-groups-count', { count: groups.length })
  }
  if (groups.length === 0 && users.length > 0) {
    return t('docs-collection-browser-members-count', { count: users.length })
  }
  return t('docs-collection-browser-audience-mixed', { groups: groups.length, users: users.length })
}

const onDragStart = (index: number, event: DragEvent) => {
  dragIndex.value = index
  if (event.dataTransfer) {
    event.dataTransfer.effectAllowed = 'move'
    event.dataTransfer.setData('text/plain', String(index))
  }
}

const onDragOver = (index: number, event: DragEvent) => {
  event.preventDefault()
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = 'move'
  }
  if (dragIndex.value !== null && dragIndex.value !== index) {
    dropIndex.value = index
  }
}

const onDragLeave = () => {
  dropIndex.value = null
}

const onDrop = async (targetIndex: number) => {
  if (dragIndex.value === null || dragIndex.value === targetIndex) {
    dragIndex.value = null
    dropIndex.value = null
    return
  }

  const items = [...collections.value]
  const [moved] = items.splice(dragIndex.value, 1)
  items.splice(targetIndex, 0, moved!)

  dragIndex.value = null
  dropIndex.value = null

  // Reorder in the pool now; the server's sync echo confirms it.
  const previous = items.map((c) => [c.id, c.display_order] as const)
  items.forEach((c, i) =>
    pool.patch<DocCollectionRow>('documentation_collection', c.id, { display_order: i }),
  )

  const orders = items.map((c, i) => ({ collection_id: c.id, display_order: i }))
  if (!(await reorderCollections(orders))) {
    for (const [id, order] of previous) {
      pool.patch<DocCollectionRow>('documentation_collection', id, { display_order: order })
    }
  }
}

const onDragEnd = () => {
  dragIndex.value = null
  dropIndex.value = null
}

</script>

<template>
  <!-- @container: row extras (description, freshness) key off the card's own
       width, so the ledger degrades gracefully in a narrow main column. -->
  <section class="@container bg-surface rounded-xl border border-default overflow-hidden">
    <header class="flex items-center gap-2 px-3 h-9 border-b border-default bg-surface-alt">
      <h2 class="text-xs-plus font-semibold text-primary tracking-tight truncate flex-1">
        {{ $t('docs-index-library-heading') }}
      </h2>
      <span v-if="collections.length > 0" class="text-2xs text-tertiary tabular-nums">
        {{ $t('docs-index-library-count', { count: collections.length }) }}
      </span>
      <button
        type="button"
        class="text-2xs font-medium text-accent hover:underline whitespace-nowrap"
        @click="emit('create')"
      >
        {{ $t('docs-collection-browser-new') }}
      </button>
    </header>

    <ul v-if="collections.length > 0" class="flex flex-col">
      <li
        v-for="(collection, index) in collections"
        :key="collection.id"
        :draggable="canReorder"
        class="group flex items-start gap-3.5 px-4 py-3.5 border-b border-subtle last:border-b-0 hover:bg-surface-hover/50 transition-colors"
        :class="{
          'opacity-50': dragIndex === index,
          'ring-2 ring-inset ring-accent/40': dropIndex === index,
          'cursor-grab': canReorder,
        }"
        @dragstart="canReorder && onDragStart(index, $event)"
        @dragover="canReorder && onDragOver(index, $event)"
        @dragleave="canReorder && onDragLeave()"
        @drop.prevent="canReorder && onDrop(index)"
        @dragend="canReorder && onDragEnd()"
      >
        <CollectionIcon :icon="collection.icon" :color="collection.color" size="md" />

        <div class="flex flex-col gap-1 min-w-0 flex-1">
          <div class="flex items-baseline gap-2 min-w-0">
            <RouterLink
              :to="`/documentation/collections/${collection.slug}`"
              class="text-sm font-semibold text-primary tracking-tight hover:text-accent transition-colors shrink-0 max-w-full truncate"
            >
              {{ collection.name }}
            </RouterLink>
            <span
              v-if="collection.restricted"
              class="shrink-0 inline-flex items-center gap-1 self-center text-2xs text-tertiary border border-subtle rounded-md px-1.5 py-px bg-surface-alt"
            >
              <Icon name="lock" size="xs" class="text-status-warning/80" aria-hidden="true" />
              {{ audienceLabel(collection) }}
            </span>
            <span
              v-if="collection.description"
              class="hidden @2xl:inline text-xs text-tertiary truncate min-w-0"
            >
              {{ collection.description }}
            </span>
          </div>

          <div
            v-if="derived(collection).topPages.length > 0"
            class="flex items-center gap-x-1.5 gap-y-0.5 flex-wrap text-xs"
          >
            <!-- Separator rides inside each item so a wrap never strands a
                 lone dot at a line edge. -->
            <span
              v-for="(page, i) in derived(collection).topPages"
              :key="page.id"
              class="flex items-center gap-1.5 min-w-0"
            >
              <span v-if="i > 0" class="text-strong" aria-hidden="true">·</span>
              <RouterLink
                :to="docUrl(page)"
                class="text-secondary hover:text-accent transition-colors truncate max-w-[16rem]"
              >
                {{ page.title }}
              </RouterLink>
            </span>
            <span v-if="derived(collection).moreCount > 0" class="flex items-center gap-1.5">
              <span class="text-strong" aria-hidden="true">·</span>
              <RouterLink
                :to="`/documentation/collections/${collection.slug}`"
                class="text-tertiary hover:text-accent transition-colors whitespace-nowrap"
              >
                {{ $t('docs-index-library-more-pages', { count: derived(collection).moreCount }) }}
              </RouterLink>
            </span>
          </div>
        </div>

        <div class="flex flex-col items-end gap-1 shrink-0 text-2xs text-tertiary whitespace-nowrap">
          <span class="font-medium text-secondary tabular-nums">
            {{ $t('docs-collection-browser-pages', { count: pageCount(collection) }) }}
          </span>
          <span v-if="derived(collection).updatedAt" class="hidden @xl:inline">
            {{ $t('docs-index-library-updated', { time: formatRelativeTime(derived(collection).updatedAt!, { addSuffix: true }) }) }}
          </span>
        </div>
      </li>
    </ul>

    <p v-else class="text-xs-plus text-tertiary px-4 py-3">
      {{ $t('docs-collection-browser-empty') }}
    </p>
  </section>
</template>
