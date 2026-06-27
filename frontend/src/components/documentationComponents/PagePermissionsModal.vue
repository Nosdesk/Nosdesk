<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useFluent } from 'fluent-vue'
import { getPageVisibility, setPageVisibility } from '@/services/documentationService'
import { getCollectionsForPage } from '@nosdesk/core/services/collectionService'
import type { Collection } from '@nosdesk/core/services/collectionService'
import AssignmentPicker from '@/components/common/AssignmentPicker.vue'
import type { SelectedPrincipal } from '@/components/common/AssignmentPicker.vue'
import Modal from '@/components/Modal.vue'
import Button from '@/components/common/Button.vue'
import CollectionIcon from '@/components/documentationComponents/CollectionIcon.vue'

useFluent()

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
  <Modal :show="true" :title="$t('docs-page-permissions-title')" size="sm" @close="emit('close')">
    <div v-if="loading" class="flex flex-col gap-3">
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
              {{ $t('docs-page-permissions-mode-inherit') }}
            </button>
            <button
              @click="mode = 'custom'"
              class="flex-1 px-3 py-1.5 text-xs font-medium rounded-md transition-colors"
              :class="mode === 'custom'
                ? 'bg-surface text-primary shadow-sm'
                : 'text-tertiary hover:text-secondary'"
            >
              {{ $t('docs-page-permissions-mode-custom') }}
            </button>
          </div>

          <!-- Inherit Mode -->
          <div v-if="mode === 'inherit'" class="flex flex-col gap-3">
            <p class="text-xs text-tertiary">
              {{ $t('docs-page-permissions-inherit-description') }}
            </p>
            <div v-if="pageCollections.length === 0" class="p-3 rounded-lg bg-surface-alt text-xs text-tertiary text-center">
              {{ $t('docs-page-permissions-no-collections') }}
            </div>
            <div v-else class="flex flex-col gap-1.5">
              <div
                v-for="collection in pageCollections"
                :key="collection.id"
                class="flex items-center gap-2.5 p-2.5 rounded-lg bg-surface-alt"
              >
                <CollectionIcon
                  :icon="collection.icon"
                  :color="collection.color"
                  size="sm"
                />
                <span class="text-sm text-primary truncate">{{ collection.name }}</span>
              </div>
            </div>
          </div>

          <!-- Custom Mode -->
          <div v-if="mode === 'custom'" class="flex flex-col gap-3">
            <p class="text-xs text-tertiary">
              {{ $t('docs-page-permissions-custom-description') }}
            </p>

            <AssignmentPicker
              :selectedItems="selectedItems"
              @update:selectedItems="selectedItems = $event"
              :placeholder="$t('docs-page-permissions-picker-placeholder')"
            />

            <p v-if="mode === 'custom' && selectedItems.length === 0" class="text-xs text-status-warning">
              {{ $t('docs-page-permissions-no-selection-warning') }}
            </p>
          </div>
    </template>

    <template #footer>
      <div class="flex items-center justify-end gap-2">
        <Button variant="ghost" size="sm" @click="emit('close')">
          {{ $t('docs-page-permissions-cancel') }}
        </Button>
        <Button size="sm" :loading="saving" :disabled="!hasChanges" @click="save">
          {{ $t('docs-page-permissions-save') }}
        </Button>
      </div>
    </template>
  </Modal>
</template>
