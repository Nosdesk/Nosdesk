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
    :scroll-content="false"
    @close="emit('close')"
  >
    <div class="flex flex-col flex-1 min-h-0 gap-4">
      <!-- Preview (left) + colour picker (right) -->
      <section class="shrink-0 rounded-xl border border-subtle bg-surface-alt/60 p-3">
        <div class="flex items-center gap-3 sm:gap-4">
          <div
            class="shrink-0 flex flex-col items-center gap-1 pt-0.5"
            :title="$t('docs-collection-appearance-preview')"
          >
            <CollectionIcon
              :icon="draftIcon"
              :color="draftColor"
              size="lg"
            />
          </div>

          <div class="flex-1 min-w-0">
            <ColorHueSlider
              v-model="draftColor"
              layout="stacked"
              hide-swatch
              :label="$t('docs-edit-collection-color')"
            />
          </div>
        </div>
      </section>

      <!-- Icon picker — sole scroll region -->
      <section class="flex flex-col flex-1 min-h-0 gap-2">
        <span class="shrink-0 text-xs font-medium text-secondary px-0.5">
          {{ $t('docs-edit-collection-icon') }}
        </span>
        <div class="flex flex-col flex-1 min-h-0 rounded-xl border border-subtle overflow-hidden bg-surface">
          <DocumentIconPickerPanel
            v-model="draftIcon"
            :active="show"
            embedded
            fill-height
            :close-on-select="false"
          />
        </div>
      </section>
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
