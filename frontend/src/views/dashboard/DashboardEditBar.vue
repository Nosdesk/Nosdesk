<!--
Compact action bar shown while the dashboard is in edit mode.
Transactional shape (docs/dashboard-and-analytics-plan.md decision
17): Done writes the working copy to the server; Discard drops it;
Undo/Redo step through the in-session change history. Reset to
defaults is its own thing (replaces the working copy with the
factory layout) and is still gated behind a confirm.

The bar reads as a mode indicator first (Editing dot + dirty
state), affordances second. Done is the primary action only when
there are pending changes; otherwise it's a quiet "Close" — saving
nothing.
-->
<script setup lang="ts">
import { ref } from 'vue'
import { useFluent } from 'fluent-vue'
import { useDashboardLayoutStore } from '@/stores/dashboardLayout'
import { useToastStore } from '@nosdesk/core/stores/toast'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import AddWidgetModal from './AddWidgetModal.vue'
import Icon from '@/components/common/Icon.vue'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

const store = useDashboardLayoutStore()
const toast = useToastStore()

const showAdd = ref(false)
const showResetConfirm = ref(false)

async function done() {
  // store.done() throws on persistence failure (network error,
  // auth lost, server 5xx) and leaves the working copy intact so
  // the user can retry. We surface the failure as an error toast
  // — without this catch the rejection is unhandled and the user
  // sees no signal that their click did anything.
  try {
    await store.done()
  } catch (err) {
    console.error('Dashboard save failed', err)
    toast.error(
      t('dashboard-edit-bar-save-error-title'),
      t('dashboard-edit-bar-save-error-message'),
    )
  }
}

function discard() {
  store.discard()
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
      <span v-if="store.isDirty">{{ t('dashboard-edit-bar-unsaved') }}</span>
      <span v-else>{{ t('dashboard-edit-bar-editing') }}</span>
    </span>

    <span class="flex-1" />

    <!-- Undo / Redo. Disabled-when-empty rather than hidden so the
         shortcuts (Cmd-Z / Cmd-Shift-Z) have a discoverable mouse
         affordance and the bar's chrome doesn't shift width as the
         session progresses. -->
    <button
      type="button"
      class="inline-flex items-center gap-1.5 px-2 py-1.5 text-xs rounded-md text-secondary transition-colors disabled:opacity-40 disabled:cursor-not-allowed enabled:hover:bg-surface-hover enabled:hover:text-primary"
      :disabled="!store.canUndo"
      :title="t('dashboard-edit-bar-undo-tooltip')"
      @click="store.undo"
    >
      <Icon name="refresh" class="w-3.5 h-3.5 scale-x-[-1]" />
      <span class="sr-only">{{ t('dashboard-edit-bar-undo') }}</span>
    </button>

    <button
      type="button"
      class="inline-flex items-center gap-1.5 px-2 py-1.5 text-xs rounded-md text-secondary transition-colors disabled:opacity-40 disabled:cursor-not-allowed enabled:hover:bg-surface-hover enabled:hover:text-primary"
      :disabled="!store.canRedo"
      :title="t('dashboard-edit-bar-redo-tooltip')"
      @click="store.redo"
    >
      <Icon name="refresh" class="w-3.5 h-3.5" />
      <span class="sr-only">{{ t('dashboard-edit-bar-redo') }}</span>
    </button>

    <span class="w-px h-5 bg-default mx-1" aria-hidden="true" />

    <button
      type="button"
      class="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-md text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
      @click="showAdd = true"
    >
      <Icon name="add" />
      {{ t('dashboard-edit-bar-add-widget') }}
    </button>

    <button
      type="button"
      class="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-md text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
      @click="showResetConfirm = true"
    >
      {{ t('dashboard-edit-bar-reset') }}
    </button>

    <!-- Discard is secondary; Done is primary only when there are
         actual changes to save. When the working copy matches
         canonical, Done quietly closes the session without trying
         to claim the user accomplished something. -->
    <button
      type="button"
      class="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-md text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
      :disabled="!store.isDirty"
      @click="discard"
    >
      {{ t('dashboard-edit-bar-discard') }}
    </button>

    <button
      type="button"
      :class="[
        'inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md transition-opacity',
        store.isDirty
          ? 'bg-accent text-on-accent hover:opacity-90'
          : 'border border-default bg-surface text-secondary hover:bg-surface-hover',
      ]"
      :disabled="store.saving"
      @click="done"
    >
      {{ store.isDirty ? t('dashboard-edit-bar-done') : t('dashboard-edit-bar-close') }}
    </button>
  </div>

  <AddWidgetModal :show="showAdd" @close="showAdd = false" />

  <ConfirmModal
    :show="showResetConfirm"
    variant="warning"
    :title="t('dashboard-edit-bar-reset-confirm-title')"
    :message="t('dashboard-edit-bar-reset-confirm-message')"
    :confirm-label="t('dashboard-edit-bar-reset-confirm-label')"
    @confirm="doReset"
    @close="showResetConfirm = false"
  />
</template>
