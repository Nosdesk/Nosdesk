<!--
Compact action bar shown while the dashboard is in edit mode. Sits
inline above the grid as a narrow pill so it reads as a
mode-indicator, not a large form. Actions are Add / Reset / Done with
Done as the primary.
-->
<script setup lang="ts">
import { ref } from 'vue'
import { useDashboardLayoutStore } from '@/stores/dashboardLayout'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import AddWidgetModal from './AddWidgetModal.vue'
import Icon from '@/components/common/Icon.vue'

const store = useDashboardLayoutStore()

const showAdd = ref(false)
const showResetConfirm = ref(false)

async function done() {
  await store.persistNow()
  store.editMode = false
}

function doReset() {
  showResetConfirm.value = false
  store.resetToDefaults()
}
</script>

<template>
  <div class="flex items-center gap-2 px-3 py-2 rounded-lg bg-surface-alt border border-default">
    <span class="inline-flex items-center gap-1.5 text-xs font-medium text-secondary">
      <span class="w-1.5 h-1.5 rounded-full bg-accent animate-pulse" aria-hidden="true" />
      Editing dashboard
    </span>

    <span class="flex-1" />

    <button
      type="button"
      class="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-md text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
      @click="showAdd = true"
    >
      <Icon name="add" />
      Add widget
    </button>

    <button
      type="button"
      class="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-md text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
      @click="showResetConfirm = true"
    >
      Reset
    </button>

    <button
      type="button"
      class="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md bg-accent text-white hover:opacity-90 transition-opacity"
      @click="done"
    >
      Done
    </button>
  </div>

  <AddWidgetModal :show="showAdd" @close="showAdd = false" />

  <ConfirmModal
    :show="showResetConfirm"
    variant="warning"
    title="Reset dashboard layout?"
    message="Your customised layout will be replaced with the default for your role."
    confirm-label="Reset"
    @confirm="doReset"
    @close="showResetConfirm = false"
  />
</template>
