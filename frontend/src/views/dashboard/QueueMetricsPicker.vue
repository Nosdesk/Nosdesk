<!--
Picker modal for the Queue widget's KPI rail. Lets the user pick up
to `max` metrics from the widget's catalog. The Queue widget owns the
catalog and the persistence call — this component is a presentational
wrapper, so the same pattern can be reused for future configurable
widgets without touching the dashboard store.
-->
<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import Modal from '@/components/Modal.vue'
import Checkbox from '@/components/common/Checkbox.vue'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

interface CatalogItem {
  id: string
  label: string
  description: string
}

const props = defineProps<{
  show: boolean
  catalog: CatalogItem[]
  selectedIds: string[]
  max: number
}>()

const emit = defineEmits<{
  (e: 'save', ids: string[]): void
  (e: 'close'): void
}>()

// Working copy of the selection — discarded on cancel so the user's
// persisted selection stays untouched until they hit Save.
const draft = ref<Set<string>>(new Set(props.selectedIds))

// Reseed the draft every time the modal opens so reopening after a
// cancel starts from the persisted state, not the previous draft.
watch(
  () => props.show,
  (open) => {
    if (open) draft.value = new Set(props.selectedIds)
  },
)

function toggle(id: string) {
  const next = new Set(draft.value)
  if (next.has(id)) {
    next.delete(id)
  } else {
    if (next.size >= props.max) return
    next.add(id)
  }
  draft.value = next
}

const atCap = computed(() => draft.value.size >= props.max)
const canSave = computed(() => draft.value.size > 0)

function save() {
  // Preserve catalog order so rail rendering is deterministic
  // regardless of the order the user clicked checkboxes.
  const ordered = props.catalog.map((c) => c.id).filter((id) => draft.value.has(id))
  emit('save', ordered)
}
</script>

<template>
  <Modal
    :show="show"
    :title="t('dashboard-queue-metrics-picker-title')"
    size="sm"
    @close="emit('close')"
  >
    <div class="flex flex-col gap-3">
      <p class="text-xs text-secondary">
        {{ t('dashboard-queue-metrics-picker-hint', { max }) }}
        <span class="text-tertiary">{{ t('dashboard-queue-metrics-picker-count', { count: draft.size, max }) }}</span>
      </p>

      <ul class="flex flex-col gap-1 -mx-1">
        <li v-for="item in catalog" :key="item.id">
          <label
            :class="[
              'flex items-start gap-3 px-3 py-2 rounded-md cursor-pointer transition-colors',
              draft.has(item.id)
                ? 'bg-accent/10 border border-accent/30'
                : 'border border-transparent hover:bg-surface-hover',
              atCap && !draft.has(item.id) ? 'opacity-50 cursor-not-allowed' : '',
            ]"
          >
            <Checkbox
              :model-value="draft.has(item.id)"
              :disabled="atCap && !draft.has(item.id)"
              size="sm"
              :aria-label="t('dashboard-queue-metrics-picker-toggle-aria', { label: item.label })"
              class="mt-0.5"
              @change="toggle(item.id)"
            />
            <div class="flex-1 min-w-0">
              <p class="text-sm font-medium text-primary">{{ item.label }}</p>
              <p class="text-xs text-tertiary mt-0.5">{{ item.description }}</p>
            </div>
          </label>
        </li>
      </ul>

      <div class="flex justify-end gap-2 pt-2 border-t border-default">
        <button
          type="button"
          class="px-3 py-1.5 text-xs rounded-md text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
          @click="emit('close')"
        >
          {{ t('dashboard-queue-metrics-picker-cancel') }}
        </button>
        <button
          type="button"
          :disabled="!canSave"
          :class="[
            'px-3 py-1.5 text-xs font-medium rounded-md transition-opacity',
            canSave ? 'bg-accent text-on-accent hover:opacity-90' : 'bg-accent/40 text-on-accent cursor-not-allowed',
          ]"
          @click="save"
        >
          {{ t('dashboard-queue-metrics-picker-save') }}
        </button>
      </div>
    </div>
  </Modal>
</template>
