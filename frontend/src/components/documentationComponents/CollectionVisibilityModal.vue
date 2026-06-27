<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useFluent } from 'fluent-vue'
import { setCollectionVisibility } from '@nosdesk/core/services/collectionService'
import type { VisibleUser } from '@nosdesk/core/services/collectionService'
import AssignmentPicker from '@/components/common/AssignmentPicker.vue'
import type { SelectedPrincipal } from '@/components/common/AssignmentPicker.vue'
import Modal from '@/components/Modal.vue'
import Button from '@/components/common/Button.vue'

useFluent()

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
  const { groupService } = await import('@nosdesk/core/services/groupService')
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
  <Modal :show="true" :title="$t('docs-collection-visibility-title')" size="sm" @close="emit('close')">
    <div v-if="loading" class="flex flex-col gap-3">
      <div v-for="i in 4" :key="i" class="h-10 rounded-lg bg-surface-alt animate-pulse"></div>
    </div>

    <div v-else class="flex flex-col gap-3">
      <p class="text-xs text-tertiary">
        {{ $t('docs-collection-visibility-description') }}
      </p>

      <!-- Public indicator -->
      <div v-if="isPublic" class="flex items-center gap-2 p-2.5 rounded-lg bg-status-success/10 border border-status-success/20">
        <svg class="w-4 h-4 text-status-success flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3.055 11H5a2 2 0 012 2v1a2 2 0 002 2 2 2 0 012 2v2.945M8 3.935V5.5A2.5 2.5 0 0010.5 8h.5a2 2 0 012 2 2 2 0 104 0 2 2 0 012-2h1.064M15 20.488V18a2 2 0 012-2h3.064M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
        <span class="text-xs text-status-success">{{ $t('docs-collection-visibility-public') }}</span>
      </div>

      <AssignmentPicker
        :selectedItems="selectedItems"
        @update:selectedItems="selectedItems = $event"
        :placeholder="$t('docs-collection-visibility-picker-placeholder')"
      />
    </div>

    <template #footer>
      <div class="flex items-center justify-end gap-2">
        <Button variant="ghost" size="sm" @click="emit('close')">
          {{ $t('docs-collection-visibility-cancel') }}
        </Button>
        <Button size="sm" :loading="saving" :disabled="!hasChanges" @click="save">
          {{ $t('docs-collection-visibility-save') }}
        </Button>
      </div>
    </template>
  </Modal>
</template>
