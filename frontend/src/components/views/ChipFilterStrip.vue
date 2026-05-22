<script setup lang="ts">
/**
 * Chip-strip surface used by server-paginated list views (assets,
 * users, ...). Renders the active-filter pills inline with the
 * "+ Add filter" affordance, identical in shape to the tickets
 * header's filter strip, so all list views share one chip vocabulary.
 *
 * Wire it up with `useChipFiltersFromControls`: that composable
 * produces every prop this component needs from a facet config
 * plus the view's `useListControls` instance.
 */
import FilterPill from '@/components/views/FilterPill.vue'
import AddFilterMenu, {
  type AddFilterFacet,
} from '@/components/views/AddFilterMenu.vue'
import type { FilterOption } from '@/composables/useListFilters'
import type { ChipPill } from '@/composables/useChipFiltersFromControls'

defineProps<{
  pills: ChipPill[]
  addFilterFacets: AddFilterFacet[]
  activeFacets: string[]
  optionsFor: (key: string) => FilterOption[]
  selectedFor: (key: string) => Set<string>
  textValueFor: (key: string) => string
  onToggle: (key: string, value: string) => void
  onClear: (key: string) => void
  onSetText: (key: string, value: string) => void
}>()
</script>

<template>
  <TransitionGroup
    v-if="activeFacets.length > 0"
    name="filter-pill"
    tag="div"
    class="flex items-center gap-1.5 flex-wrap"
  >
    <FilterPill
      v-for="pill in pills"
      :key="pill.facet"
      :facet="pill.facet"
      :kind="pill.kind"
      :label="pill.label"
      :value-summary="pill.valueSummary"
      :options="pill.options"
      :selected="pill.selected"
      :text-value="pill.textValue"
      @toggle="(v: string) => onToggle(pill.facet, v)"
      @clear="onClear(pill.facet)"
      @set-text="(v: string) => onSetText(pill.facet, v)"
      @remove="onClear(pill.facet)"
    />
  </TransitionGroup>
  <AddFilterMenu
    :facets="addFilterFacets"
    :active-facets="activeFacets"
    :options-for="optionsFor"
    :selected-for="selectedFor"
    :text-value-for="textValueFor"
    @toggle="onToggle"
    @clear="onClear"
    @set-text="onSetText"
  />
</template>
