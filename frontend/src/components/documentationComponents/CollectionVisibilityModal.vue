<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { setCollectionVisibility } from '@/services/collectionService'
import type { VisibleUser } from '@/services/collectionService'
import AssignmentPicker from '@/components/common/AssignmentPicker.vue'
import type { SelectedPrincipal } from '@/components/common/AssignmentPicker.vue'

const props = defineProps<{
  collectionId: number
  currentGroupIds: number[]
  currentUsers: VisibleUser[]
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'updated'): void
}>()

const saving = ref(false)
const loading = ref(true)

const selectedItems = ref<SelectedPrincipal[]>([])
const originalItems = ref<SelectedPrincipal[]>([])

const hasChanges = computed(() => {
  if (selectedItems.value.length !== originalItems.value.length) return true
  const origKeys = new Set(originalItems.value.map(i => `${i.type}:${i.id}`))
  return selectedItems.value.some(i => !origKeys.has(`${i.type}:${i.id}`))
})

const isPublic = computed(() => selectedItems.value.length === 0)

const save = async () => {
  saving.value = true
  const groupIds = selectedItems.value
    .filter(i => i.type === 'group')
    .map(i => parseInt(i.id))
  const userUuids = selectedItems.value
    .filter(i => i.type === 'user')
    .map(i => i.id)

  const success = await setCollectionVisibility(props.collectionId, groupIds, userUuids)
  saving.value = false
  if (success) {
    emit('updated')
    emit('close')
  }
}

onMounted(async () => {
  // Build initial selected items from props
  const items: SelectedPrincipal[] = []

  // We need group names — for now, we'll use the AssignmentPicker's loaded groups
  // But we need to know the names for the initial chips. We can fetch them from the
  // collection data which already has visible_to_groups with names.
  // The parent (CollectionView) passes currentGroupIds, but we can also get names
  // from the collection's visible_to_groups. Let's load them via the groupService.
  const { groupService } = await import('@/services/groupService')
  const allGroups = await groupService.getGroups()
  const groupMap = new Map(allGroups.map(g => [g.id, g.name]))

  for (const gid of props.currentGroupIds) {
    items.push({
      type: 'group',
      id: String(gid),
      name: groupMap.get(gid) || `Group ${gid}`,
    })
  }

  for (const user of props.currentUsers) {
    items.push({
      type: 'user',
      id: user.uuid,
      name: user.name,
      avatar: user.avatar_url,
    })
  }

  selectedItems.value = [...items]
  originalItems.value = [...items]
  loading.value = false
})
</script>

<template>
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/40" @click.self="emit('close')">
    <div class="bg-surface border border-default rounded-xl shadow-2xl w-full max-w-md mx-4 max-h-[80vh] flex flex-col">
      <!-- Header -->
      <div class="flex items-center justify-between p-4 border-b border-default">
        <h3 class="text-sm font-semibold text-primary">Collection Access</h3>
        <button
          @click="emit('close')"
          class="text-tertiary hover:text-primary p-1 rounded-md hover:bg-surface-hover transition-colors"
        >
          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>
      </div>

      <!-- Content -->
      <div class="flex-1 overflow-y-auto p-4">
        <div v-if="loading" class="space-y-3">
          <div v-for="i in 4" :key="i" class="h-10 rounded-lg bg-surface-alt animate-pulse"></div>
        </div>

        <template v-else>
          <p class="text-xs text-tertiary mb-3">
            Select which groups and users can access this collection. Empty selection means the collection is public (visible to everyone).
          </p>

          <!-- Public indicator -->
          <div v-if="isPublic" class="flex items-center gap-2 p-2.5 rounded-lg bg-emerald-500/10 border border-emerald-500/20 mb-3">
            <svg class="w-4 h-4 text-emerald-500 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3.055 11H5a2 2 0 012 2v1a2 2 0 002 2 2 2 0 012 2v2.945M8 3.935V5.5A2.5 2.5 0 0010.5 8h.5a2 2 0 012 2 2 2 0 104 0 2 2 0 012-2h1.064M15 20.488V18a2 2 0 012-2h3.064M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
            <span class="text-xs text-emerald-600 dark:text-emerald-400">Public -- visible to all users</span>
          </div>

          <AssignmentPicker
            :selectedItems="selectedItems"
            @update:selectedItems="selectedItems = $event"
            placeholder="Search users and groups..."
          />
        </template>
      </div>

      <!-- Footer -->
      <div class="flex items-center justify-end gap-2 p-3 border-t border-default">
        <button
          @click="emit('close')"
          class="px-3 py-1.5 text-xs rounded-md text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
        >
          Cancel
        </button>
        <button
          @click="save"
          :disabled="!hasChanges || saving"
          class="px-3 py-1.5 text-xs rounded-md bg-accent text-white hover:opacity-90 transition-opacity disabled:opacity-50"
        >
          {{ saving ? 'Saving...' : 'Save' }}
        </button>
      </div>
    </div>
  </div>
</template>
