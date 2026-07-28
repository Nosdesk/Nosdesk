<!--
Picker for adding a widget back to the dashboard. Up to three tabs:

  * **System widgets** — the static registry entries the current
    role can use that aren't already visible on the canvas (the
    existing pre-Wave-3 behaviour).
  * **Your saved views** — workspace saved views whose viz_type is
    something other than the default list (the workspace's pickable
    chart-backed views). Clicking one drops a SavedViewWidget onto
    the canvas with the synthetic id `saved_view:<uuid>`.
  * **Plugins** — dashboard widgets contributed by installed plugins
    (technician / admin only); adds the synthetic id
    `plugin_widget:<uuid>:<component>`. Shown only when at least one
    is available.

Adding via any tab writes through `store.show`, which goes via the
transactional working copy so the add can be undone / discarded like
any other edit.
-->
<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import Modal from '@/components/Modal.vue'
import { useFluent } from 'fluent-vue'
import { useQuery } from '@pinia/colada'
import { useDashboardLayoutStore } from '@/stores/dashboardLayout'
import { savedViewWidgetId, widgetPreviewKind, savedViewPreviewKind, pluginWidgetId } from './widgets'
import WidgetPreview from './WidgetPreview.vue'
import { savedViewsService, type SavedView } from '@/services/savedViewsService'
import { getSlotRegistrations, type PluginSlotRegistration } from '@/plugins/loader'
import { useAuthStore } from '@/stores/auth'
import { effectiveRole } from '@nosdesk/core/types/user'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const store = useDashboardLayoutStore()
const auth = useAuthStore()

type Tab = 'system' | 'saved-views' | 'plugins'
const tab = ref<Tab>('system')

// Plugin dashboard widgets are opt-in and gated to technician / admin (matching
// the synthesised widget's roles). Only offer ones not already on the canvas.
const canUsePluginWidgets = computed(() => {
  const role = auth.user ? effectiveRole(auth.user) : 'user'
  return role === 'technician' || role === 'admin'
})
const pluginWidgets = computed<PluginSlotRegistration[]>(() => {
  if (!canUsePluginWidgets.value) return []
  return getSlotRegistrations('dashboard.widget').filter((r) => {
    const id = pluginWidgetId(r.pluginUuid, r.componentName)
    const entry = store.layout.widgets.find((w) => w.id === id)
    return !entry || !entry.visible
  })
})

// Refetch the pickable saved views every time the modal opens so a
// view promoted to chart-shape in another tab shows up here without
// a page reload. While closed, the query stays disabled to avoid
// background polling for a list the user can't see.
const savedViewsQuery = useQuery({
  key: () => ['saved-views', 'pickable'],
  query: () => savedViewsService.listPickable(),
  enabled: () => props.show,
})

watch(
  () => props.show,
  (open) => {
    if (!open) tab.value = 'system'
  },
)

const pickableSavedViews = computed<SavedView[]>(() => {
  const rows = savedViewsQuery.data.value ?? []
  // Saved views already pinned (and visible) shouldn't appear in
  // the picker. A view that is in the layout but hidden still
  // shows — the user explicitly chose to add it back.
  return rows.filter((v) => {
    const id = savedViewWidgetId(v.uuid)
    const entry = store.layout.widgets.find((w) => w.id === id)
    return !entry || !entry.visible
  })
})

const isLoadingSavedViews = computed(
  () => savedViewsQuery.status.value === 'pending' && !savedViewsQuery.data.value,
)
const savedViewsError = computed(() =>
  savedViewsQuery.error.value ? t('dashboard-saved-view-error') : null,
)

function chooseSystem(id: string) {
  store.show(id)
  emit('close')
}

function chooseSavedView(view: SavedView) {
  store.show(savedViewWidgetId(view.uuid))
  emit('close')
}

function choosePlugin(reg: PluginSlotRegistration) {
  store.show(pluginWidgetId(reg.pluginUuid, reg.componentName))
  emit('close')
}

// Adding from a tab the user can't see is confusing — make sure
// the modal's tab matches what's actually pickable when opened. If
// system widgets are exhausted but the user has pickable saved
// views, jump to that tab.
watch(
  () => props.show,
  (open) => {
    if (!open) return
    if (store.addable.length === 0 && pickableSavedViews.value.length > 0) {
      tab.value = 'saved-views'
    }
  },
)
</script>

<template>
  <Modal :show="show" :title="t('dashboard-add-widget-title')" size="lg" @close="emit('close')">
    <div class="flex flex-col gap-3">
      <!-- Tabs. Inline counts beside each tab label so the user
           sees what's pickable in each category without flicking
           between them. -->
      <div role="tablist" class="flex gap-1 border-b border-default">
        <button
          type="button"
          role="tab"
          :aria-selected="tab === 'system'"
          :class="[
            'px-3 py-1.5 text-xs font-medium transition-colors',
            tab === 'system'
              ? 'text-primary border-b-2 border-accent -mb-px'
              : 'text-tertiary hover:text-primary',
          ]"
          @click="tab = 'system'"
        >
          {{ t('dashboard-add-widget-tab-system') }}
          <span class="text-tertiary ml-1">({{ store.addable.length }})</span>
        </button>
        <button
          type="button"
          role="tab"
          :aria-selected="tab === 'saved-views'"
          :class="[
            'px-3 py-1.5 text-xs font-medium transition-colors',
            tab === 'saved-views'
              ? 'text-primary border-b-2 border-accent -mb-px'
              : 'text-tertiary hover:text-primary',
          ]"
          @click="tab = 'saved-views'"
        >
          {{ t('dashboard-add-widget-tab-saved-views') }}
          <span class="text-tertiary ml-1">({{ pickableSavedViews.length }})</span>
        </button>
        <button
          v-if="pluginWidgets.length > 0"
          type="button"
          role="tab"
          :aria-selected="tab === 'plugins'"
          :class="[
            'px-3 py-1.5 text-xs font-medium transition-colors',
            tab === 'plugins'
              ? 'text-primary border-b-2 border-accent -mb-px'
              : 'text-tertiary hover:text-primary',
          ]"
          @click="tab = 'plugins'"
        >
          {{ t('dashboard-add-widget-tab-plugins') }}
          <span class="text-tertiary ml-1">({{ pluginWidgets.length }})</span>
        </button>
      </div>

      <!-- System widgets tab -->
      <div v-show="tab === 'system'">
        <div
          v-if="store.addable.length === 0"
          class="text-sm text-tertiary py-4 text-center"
        >
          {{ t('dashboard-add-widget-all-added') }}
        </div>
        <ul v-else class="grid grid-cols-2 gap-2 sm:grid-cols-3">
          <li v-for="w in store.addable" :key="w.id">
            <button
              type="button"
              class="group flex h-full w-full flex-col overflow-hidden rounded-lg border border-default text-left transition-colors hover:border-accent"
              @click="chooseSystem(w.id)"
            >
              <div class="aspect-[16/9] w-full border-b border-default bg-surface-alt p-2.5">
                <WidgetPreview :kind="widgetPreviewKind(w.id)" />
              </div>
              <div class="flex flex-1 flex-col gap-0.5 p-2.5 group-hover:bg-surface-hover">
                <span class="text-sm font-medium text-primary">{{ t(w.titleKey) }}</span>
                <span class="line-clamp-2 text-xs text-tertiary">{{ t(w.descriptionKey) }}</span>
              </div>
            </button>
          </li>
        </ul>
      </div>

      <!-- Your saved views tab -->
      <div v-show="tab === 'saved-views'">
        <div v-if="isLoadingSavedViews" class="text-sm text-tertiary py-4 text-center">
          {{ t('dashboard-add-widget-saved-views-loading') }}
        </div>
        <div
          v-else-if="savedViewsError"
          class="text-sm text-status-error py-4 text-center"
        >
          {{ savedViewsError }}
        </div>
        <div
          v-else-if="pickableSavedViews.length === 0"
          class="text-sm text-tertiary py-4 text-center"
        >
          {{ t('dashboard-add-widget-saved-views-empty') }}
        </div>
        <ul v-else class="grid grid-cols-2 gap-2 sm:grid-cols-3">
          <li v-for="v in pickableSavedViews" :key="v.uuid">
            <button
              type="button"
              class="group flex h-full w-full flex-col overflow-hidden rounded-lg border border-default text-left transition-colors hover:border-accent"
              @click="chooseSavedView(v)"
            >
              <div class="aspect-[16/9] w-full border-b border-default bg-surface-alt p-2.5">
                <WidgetPreview :kind="savedViewPreviewKind(v.viz_type)" />
              </div>
              <div class="flex flex-1 flex-col gap-0.5 p-2.5 group-hover:bg-surface-hover">
                <span class="text-sm font-medium text-primary">{{ v.name }}</span>
                <span class="text-xs text-tertiary">
                  {{ t(`dashboard-saved-view-viz-label-${v.viz_type ?? 'list'}`) }}
                </span>
              </div>
            </button>
          </li>
        </ul>
      </div>

      <!-- Plugin widgets tab -->
      <div v-show="tab === 'plugins'">
        <ul class="grid grid-cols-2 gap-2 sm:grid-cols-3">
          <li v-for="w in pluginWidgets" :key="`${w.pluginUuid}:${w.componentName}`">
            <button
              type="button"
              class="group flex h-full w-full flex-col overflow-hidden rounded-lg border border-default text-left transition-colors hover:border-accent"
              @click="choosePlugin(w)"
            >
              <div class="aspect-[16/9] w-full border-b border-default bg-surface-alt p-2.5">
                <WidgetPreview kind="status" />
              </div>
              <div class="flex flex-1 flex-col gap-0.5 p-2.5 group-hover:bg-surface-hover">
                <span class="text-sm font-medium text-primary">{{ w.label ?? w.pluginName }}</span>
                <span class="text-xs text-tertiary">{{ w.pluginName }}</span>
              </div>
            </button>
          </li>
        </ul>
      </div>
    </div>
  </Modal>
</template>
