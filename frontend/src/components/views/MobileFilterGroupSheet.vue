<script setup lang="ts" generic="T extends object, C extends DataTableColumnLike, S extends BaseListViewShape = BaseListViewShape">
/**
 * Mobile bottom sheet for a `useListView` surface: surfaces the
 * group-by lenses and filters that the desktop toolbar packs into one
 * cramped row. Group-by is the hero here — on assets it's the entry
 * point to the planning lenses (and therefore the rollout), which was
 * undiscoverable when buried in the wrapping desktop toolbar. The
 * desktop-only column picker is intentionally dropped.
 */
import { useFluent } from 'fluent-vue'
import Modal from '@/components/Modal.vue'
import Button from '@/components/common/Button.vue'
import Icon from '@/components/common/Icon.vue'
import ChipFilterStrip from '@/components/views/ChipFilterStrip.vue'
import type { UseListView, BaseListViewShape } from '@/composables/useListView'
import type { DataTableColumnLike } from '@/composables/useDataTableColumns'

defineProps<{
  show: boolean
  listView: UseListView<T, C, S>
}>()

const emit = defineEmits<{ (e: 'close'): void }>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
</script>

<template>
  <Modal :show="show" :title="t('list-mobile-filter-group-title')" size="sm" @close="emit('close')">
    <div class="flex flex-col gap-4">
      <!-- Group by (hero) -->
      <section class="flex flex-col gap-1.5">
        <h3 class="text-xs font-medium uppercase tracking-wide text-tertiary">
          {{ t('list-mobile-group-by') }}
        </h3>
        <div class="flex flex-col divide-y divide-default overflow-hidden rounded-lg border border-default">
          <button
            v-for="opt in listView.grouping.axisOptions.value"
            :key="opt.key"
            type="button"
            class="flex items-center justify-between px-3 py-2.5 text-left text-sm transition-colors"
            :class="
              opt.key === listView.grouping.groupBy.value
                ? 'bg-accent/10 text-accent font-medium'
                : 'text-primary hover:bg-surface-hover'
            "
            @click="listView.grouping.setGroupBy(opt.key)"
          >
            {{ opt.label }}
            <Icon v-if="opt.key === listView.grouping.groupBy.value" name="check" size="sm" />
          </button>
        </div>
      </section>

      <!-- Filters -->
      <section class="flex flex-col gap-1.5">
        <h3 class="text-xs font-medium uppercase tracking-wide text-tertiary">
          {{ t('list-mobile-filters') }}
        </h3>
        <ChipFilterStrip
          :pills="listView.chipFilters.pills.value"
          :add-filter-facets="listView.chipFilters.addFilterFacets.value"
          :active-facets="listView.chipFilters.activeFacets.value"
          :options-for="listView.chipFilters.optionsFor"
          :selected-for="listView.chipFilters.selectedFor"
          :text-value-for="listView.chipFilters.textValueFor"
          :on-toggle="listView.chipFilters.toggleValue"
          :on-clear="listView.chipFilters.clearFacet"
          :on-set-text="listView.chipFilters.setText"
        />
      </section>

      <Button variant="primary" block @click="emit('close')">
        {{ t('list-mobile-filter-done') }}
      </Button>
    </div>
  </Modal>
</template>
