<script setup lang="ts">
import { ref, watch } from 'vue'
import Modal from '@/components/Modal.vue'
import Button from '@/components/common/Button.vue'
import ColorHueSlider from '@/components/common/ColorHueSlider.vue'
import DocumentIconPickerPanel from '@/components/DocumentIconPickerPanel.vue'
import CollectionIcon from '@/components/documentationComponents/CollectionIcon.vue'

const props = defineProps<{
  show: boolean
  icon: string | null
  color: string | null
  saving?: boolean
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'save', payload: { icon: string; color: string }): void
}>()

const draftIcon = ref('📁')
const draftColor = ref('#6366f1')

watch(
  () => props.show,
  (open) => {
    if (!open) return
    draftIcon.value = props.icon || '📁'
    draftColor.value = props.color || '#6366f1'
  },
  { immediate: true },
)

function handleSave() {
  emit('save', {
    icon: draftIcon.value,
    color: draftColor.value,
  })
}
</script>

<template>
  <Modal
    :show="show"
    :title="$t('docs-collection-appearance-title')"
    size="lg"
    @close="emit('close')"
  >
    <div class="grid grid-cols-1 lg:grid-cols-[12rem_minmax(0,1fr)] gap-4">
      <div class="flex flex-col gap-4">
        <div class="flex flex-col items-center gap-2 py-1">
          <CollectionIcon
            :icon="draftIcon"
            :color="draftColor"
            size="xl"
          />
          <span class="text-[11px] text-tertiary text-center">
            {{ $t('docs-collection-appearance-preview') }}
          </span>
        </div>
        <ColorHueSlider
          v-model="draftColor"
          :label="$t('docs-edit-collection-color')"
        />
      </div>

      <DocumentIconPickerPanel
        v-model="draftIcon"
        :active="show"
        :close-on-select="false"
        grid-max-class="max-h-72 lg:max-h-80"
      />
    </div>

    <template #footer>
      <div class="flex items-center justify-end gap-2">
        <Button variant="ghost" size="sm" :disabled="saving" @click="emit('close')">
          {{ $t('docs-edit-collection-cancel') }}
        </Button>
        <Button size="sm" :loading="saving" @click="handleSave">
          {{ saving ? $t('docs-edit-collection-saving') : $t('docs-edit-collection-save') }}
        </Button>
      </div>
    </template>
  </Modal>
</template>
