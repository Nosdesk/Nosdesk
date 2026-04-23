<!--
Picker for restoring hidden or newly-available widgets to the
dashboard. Lists every widget that is either missing from the stored
layout or marked `visible: false`. Clicking an entry toggles it on and
closes the modal.
-->
<script setup lang="ts">
import Modal from '@/components/Modal.vue'
import { useDashboardLayoutStore } from '@/stores/dashboardLayout'

defineProps<{ show: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const store = useDashboardLayoutStore()

function choose(id: string) {
  store.show(id)
  emit('close')
}
</script>

<template>
  <Modal :show="show" title="Add widget" size="sm" @close="emit('close')">
    <div v-if="store.addable.length === 0" class="text-sm text-tertiary py-4 text-center">
      All available widgets are already on your dashboard.
    </div>
    <ul v-else class="flex flex-col gap-1">
      <li v-for="w in store.addable" :key="w.id">
        <button
          type="button"
          class="w-full text-left flex flex-col gap-0.5 p-3 rounded-lg border border-default hover:border-accent hover:bg-surface-hover transition-colors"
          @click="choose(w.id)"
        >
          <span class="text-sm font-medium text-primary">{{ w.title }}</span>
          <span class="text-xs text-tertiary">{{ w.description }}</span>
        </button>
      </li>
    </ul>
  </Modal>
</template>
