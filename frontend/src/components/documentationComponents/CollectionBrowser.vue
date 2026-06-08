<script setup lang="ts">
import { computed, ref, onMounted } from 'vue'
import { RouterLink } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { getCollections, reorderCollections } from '@/services/collectionService'
import type { CollectionWithDetails } from '@/services/collectionService'
import { useAuthStore } from '@/stores/auth'
import Skeleton from '@/components/common/Skeleton.vue'
import SkeletonBar from '@/components/common/SkeletonBar.vue'
import Button from '@/components/common/Button.vue'
import Icon from '@/components/common/Icon.vue'
import AvatarStack from '@/components/common/AvatarStack.vue'
import CollectionIcon from '@/components/documentationComponents/CollectionIcon.vue'

const emit = defineEmits<{
  (e: 'create'): void
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const authStore = useAuthStore()
const collections = ref<CollectionWithDetails[]>([])
const loading = ref(true)

const dragIndex = ref<number | null>(null)
const dropIndex = ref<number | null>(null)

const canReorder = authStore.isTechnician

const showSkeleton = computed(() => loading.value && collections.value.length === 0)
const skeletonCount = computed(() =>
  collections.value.length > 0 ? Math.min(collections.value.length, 6) : 3,
)

function memberUuids(collection: CollectionWithDetails): string[] {
  return collection.visible_to_users.map((u) => u.uuid)
}

function audienceLabel(collection: CollectionWithDetails): string {
  const groups = collection.visible_to_groups.length
  const users = collection.visible_to_users.length

  if (groups === 0 && users === 0) {
    return t('docs-collection-browser-restricted-badge')
  }
  if (groups === 1 && users === 0) {
    return collection.visible_to_groups[0].name
  }
  if (groups === 0 && users === 1) {
    return collection.visible_to_users[0].name
  }
  if (groups > 0 && users === 0) {
    return t('docs-collection-browser-groups-count', { count: groups })
  }
  if (groups === 0 && users > 0) {
    return t('docs-collection-browser-members-count', { count: users })
  }
  return t('docs-collection-browser-audience-mixed', { groups, users })
}

function pageCountLabel(count: number): string {
  return t('docs-collection-browser-pages', { count })
}

const loadCollections = async () => {
  loading.value = true
  try {
    collections.value = await getCollections()
  } finally {
    loading.value = false
  }
}

defineExpose({ reload: loadCollections })

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
  items.splice(targetIndex, 0, moved)
  collections.value = items

  dragIndex.value = null
  dropIndex.value = null

  const orders = items.map((c, i) => ({ collection_id: c.id, display_order: i }))
  await reorderCollections(orders)
}

const onDragEnd = () => {
  dragIndex.value = null
  dropIndex.value = null
}

onMounted(loadCollections)
</script>

<template>
  <section class="flex flex-col gap-3">
    <header class="flex items-center justify-between gap-3 min-h-8">
      <div class="flex items-center gap-2 min-w-0">
        <h2 class="text-sm font-semibold text-primary tracking-tight truncate">
          {{ $t('docs-collection-browser-heading') }}
        </h2>
        <span
          v-if="collections.length > 0"
          class="shrink-0 inline-flex items-center h-5 px-1.5 rounded-md bg-surface-alt border border-subtle text-[11px] text-tertiary tabular-nums"
        >
          {{ collections.length }}
        </span>
      </div>
      <Button
        size="sm"
        variant="ghost"
        icon="add"
        class="shrink-0 -mr-2"
        @click="emit('create')"
      >
        {{ $t('docs-collection-browser-new') }}
      </Button>
    </header>

    <Skeleton
      v-if="showSkeleton"
      :label="$t('docs-collection-browser-loading-label')"
      class="docs-collection-grid"
    >
      <div
        v-for="i in skeletonCount"
        :key="i"
        class="flex flex-col gap-2 p-2.5 rounded-xl bg-surface border border-default"
      >
        <div class="flex items-start gap-2">
          <SkeletonBar class="w-5 h-5 rounded shrink-0" />
          <div class="flex-1 flex flex-col gap-1.5 min-w-0">
            <div class="flex items-center gap-2">
              <SkeletonBar class="h-3 flex-1" />
              <SkeletonBar class="h-4 w-7 rounded shrink-0" />
            </div>
            <SkeletonBar class="h-2.5 w-full" />
          </div>
        </div>
        <div class="flex items-center gap-2 pl-7">
          <SkeletonBar class="h-4 w-4 rounded-full shrink-0" />
          <SkeletonBar class="h-2.5 w-20" />
        </div>
      </div>
    </Skeleton>

    <ul
      v-else-if="collections.length > 0"
      class="docs-collection-grid"
    >
      <li v-for="(collection, index) in collections" :key="collection.id">
        <RouterLink
          :to="`/documentation/collections/${collection.slug}`"
          :draggable="canReorder"
          @dragstart="canReorder && onDragStart(index, $event)"
          @dragover="canReorder && onDragOver(index, $event)"
          @dragleave="canReorder && onDragLeave()"
          @drop.prevent="canReorder && onDrop(index)"
          @dragend="canReorder && onDragEnd()"
          class="group flex flex-col gap-1.5 p-2.5 h-full rounded-xl bg-surface border border-default hover:border-accent/50 hover:shadow-sm transition-[border-color,box-shadow,background-color]"
          :class="{
            'opacity-50': dragIndex === index,
            'ring-2 ring-inset ring-accent/40': dropIndex === index,
            'cursor-grab': canReorder,
          }"
        >
          <div class="flex items-start gap-2 min-w-0">
            <CollectionIcon
              :icon="collection.icon"
              :color="collection.color"
              size="md"
            />
            <div class="min-w-0 flex-1">
              <div class="flex items-start gap-2 min-w-0">
                <h3 class="text-[13px] font-medium text-primary truncate leading-snug flex-1 min-w-0 group-hover:text-accent transition-colors">
                  {{ collection.name }}
                </h3>
                <span
                  class="shrink-0 inline-flex items-center gap-0.5 h-5 px-1.5 rounded-md bg-surface-alt border border-subtle text-[11px] font-medium text-secondary tabular-nums"
                  :title="pageCountLabel(collection.page_count)"
                >
                  <Icon name="document" size="xs" class="opacity-60" aria-hidden="true" />
                  {{ collection.page_count }}
                </span>
              </div>
              <p
                v-if="collection.description"
                class="text-[11px] text-tertiary mt-0.5 line-clamp-1 leading-snug"
              >
                {{ collection.description }}
              </p>
            </div>
          </div>

          <div class="flex items-center gap-1.5 min-w-0 pl-7 text-[11px] leading-none">
            <template v-if="collection.is_public">
              <Icon name="team" size="xs" class="shrink-0 text-tertiary opacity-70" aria-hidden="true" />
              <span class="truncate text-tertiary">{{ $t('collection-badge-public') }}</span>
            </template>
            <template v-else>
              <AvatarStack
                v-if="memberUuids(collection).length > 0"
                :uuids="memberUuids(collection)"
                :max="3"
                size="xxs"
              />
              <Icon
                v-else-if="collection.visible_to_groups.length > 0"
                name="team"
                size="xs"
                class="shrink-0 text-tertiary opacity-70"
                aria-hidden="true"
              />
              <Icon
                v-else
                name="lock"
                size="xs"
                class="shrink-0 text-status-warning/80"
                aria-hidden="true"
              />
              <span class="truncate min-w-0 text-tertiary">{{ audienceLabel(collection) }}</span>
            </template>

            <span
              v-if="collection.is_system"
              class="shrink-0 ml-auto px-1.5 py-0.5 rounded bg-surface-hover text-tertiary"
            >
              {{ $t('docs-collection-browser-system-badge') }}
            </span>
          </div>
        </RouterLink>
      </li>
    </ul>

    <p v-else class="text-[13px] text-tertiary py-2">
      {{ $t('docs-collection-browser-empty') }}
    </p>
  </section>
</template>

<style scoped>
.docs-collection-grid {
  display: grid;
  gap: 0.75rem;
  grid-template-columns: 1fr;
}

@media (min-width: 480px) {
  .docs-collection-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 0.875rem;
  }
}

@media (min-width: 768px) {
  .docs-collection-grid {
    grid-template-columns: repeat(auto-fill, minmax(14rem, 1fr));
  }
}

@media (min-width: 1280px) {
  .docs-collection-grid {
    grid-template-columns: repeat(auto-fill, minmax(16rem, 1fr));
    gap: 1rem;
  }
}
</style>
