<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { getPageVisibility, setPageVisibility } from '@/services/documentationService'
import { getCollectionsForPage } from '@/services/collectionService'
import type { Collection } from '@/services/collectionService'
import AssignmentPicker from '@/components/common/AssignmentPicker.vue'
import type { SelectedPrincipal } from '@/components/common/AssignmentPicker.vue'

const props = defineProps<{
  pageId: number
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'updated'): void
}>()

const loading = ref(true)
const saving = ref(false)

// Mode: 'inherit' (no page-level override) or 'custom' (page-level groups/users)
const mode = ref<'inherit' | 'custom'>('inherit')
const originalMode = ref<'inherit' | 'custom'>('inherit')

// Selected principals for custom mode
const selectedItems = ref<SelectedPrincipal[]>([])
const originalItems = ref<SelectedPrincipal[]>([])

// Collections this page belongs to (for display in inherit mode)
const pageCollections = ref<Collection[]>([])

const hasChanges = computed(() => {
  if (mode.value !== originalMode.value) return true
  if (mode.value === 'custom') {
    if (selectedItems.value.length !== originalItems.value.length) return true
    const origKeys = new Set(originalItems.value.map(i => `${i.type}:${i.id}`))
    return selectedItems.value.some(i => !origKeys.has(`${i.type}:${i.id}`))
  }
  return false
})

const save = async () => {
  saving.value = true
  let groupIds: number[] = []
  let userUuids: string[] = []

  if (mode.value === 'custom') {
    groupIds = selectedItems.value
      .filter(i => i.type === 'group')
      .map(i => parseInt(i.id))
    userUuids = selectedItems.value
      .filter(i => i.type === 'user')
      .map(i => i.id)
  }
  // 'inherit' mode sends empty arrays to clear override

  const success = await setPageVisibility(props.pageId, groupIds, userUuids)
  saving.value = false
  if (success) {
    emit('updated')
    emit('close')
  }
}

onMounted(async () => {
  // Load all data in parallel
  const [visibility, collections] = await Promise.all([
    getPageVisibility(props.pageId),
    getCollectionsForPage(props.pageId),
  ])

  pageCollections.value = collections

  const hasOverride = visibility.groups.length > 0 || visibility.users.length > 0

  if (hasOverride) {
    mode.value = 'custom'
    originalMode.value = 'custom'

    const items: SelectedPrincipal[] = []
    for (const group of visibility.groups) {
      items.push({
        type: 'group',
        id: String(group.id),
        name: group.name,
      })
    }
    for (const user of visibility.users) {
      items.push({
        type: 'user',
        id: user.uuid,
        name: user.name,
        avatar: user.avatar_url,
      })
    }
    selectedItems.value = [...items]
    originalItems.value = [...items]
  } else {
    mode.value = 'inherit'
    originalMode.value = 'inherit'
  }

  loading.value = false
})
</script>

<template>
  <!-- Backdrop -->
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/40" @click.self="emit('close')">
    <div class="bg-surface border border-default rounded-xl shadow-2xl w-full max-w-md mx-4 max-h-[80vh] flex flex-col">
      <!-- Header -->
      <div class="flex items-center justify-between p-4 border-b border-default">
        <h3 class="text-sm font-semibold text-primary">Page Permissions</h3>
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
          <!-- Mode Toggle -->
          <div class="flex gap-1 p-1 bg-surface-alt rounded-lg mb-4">
            <button
              @click="mode = 'inherit'"
              class="flex-1 px-3 py-1.5 text-xs font-medium rounded-md transition-colors"
              :class="mode === 'inherit'
                ? 'bg-surface text-primary shadow-sm'
                : 'text-tertiary hover:text-secondary'"
            >
              Inherit from collections
            </button>
            <button
              @click="mode = 'custom'"
              class="flex-1 px-3 py-1.5 text-xs font-medium rounded-md transition-colors"
              :class="mode === 'custom'
                ? 'bg-surface text-primary shadow-sm'
                : 'text-tertiary hover:text-secondary'"
            >
              Custom access
            </button>
          </div>

          <!-- Inherit Mode -->
          <div v-if="mode === 'inherit'" class="space-y-3">
            <p class="text-xs text-tertiary">
              This page inherits visibility from its collections. Users who can access any of the page's collections can see this page.
            </p>
            <div v-if="pageCollections.length === 0" class="p-3 rounded-lg bg-surface-alt text-xs text-tertiary text-center">
              Not in any collection -- visible to everyone.
            </div>
            <div v-else class="space-y-1.5">
              <div
                v-for="collection in pageCollections"
                :key="collection.id"
                class="flex items-center gap-2.5 p-2.5 rounded-lg bg-surface-alt"
              >
                <span class="text-lg flex-shrink-0">{{ collection.icon || '\uD83D\uDCC1' }}</span>
                <span class="text-sm text-primary truncate">{{ collection.name }}</span>
              </div>
            </div>
          </div>

          <!-- Custom Mode -->
          <div v-if="mode === 'custom'" class="space-y-3">
            <p class="text-xs text-tertiary">
              Select which groups and users can access this page. This overrides collection-level permissions.
            </p>

            <AssignmentPicker
              :selectedItems="selectedItems"
              @update:selectedItems="selectedItems = $event"
              placeholder="Search users and groups..."
            />

            <p v-if="mode === 'custom' && selectedItems.length === 0" class="text-xs text-status-warning">
              No groups or users selected -- no one except admins will be able to see this page.
            </p>
          </div>
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
