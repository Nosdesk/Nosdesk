/**
 * Chip-style filter UI driven by `useListControls`.
 *
 * Server-paginated list views (assets, users, projects, ...)
 * keep their filter state in `controls.filters` so the URL sync
 * + backend query keep working. This composable adapts that
 * state to the dataset-agnostic FilterPill / AddFilterMenu
 * components so every list view gets the same chip UX without
 * each view re-implementing the encoding/decoding glue.
 *
 * State lives in `controls.filters`, NOT in this composable.
 * That keeps the source of truth single (URL sync, request
 * params, infinite-scroll cache key all derive from
 * `controls.filters`) and means saved views can later round-
 * trip through the same Filters record without a separate
 * mirror.
 *
 * Encoding: today only CSV multi-select is used (assets warranty,
 * users role, single-option toggles like lowStock / deleted).
 * Text facets and array-valued filters can be added later by
 * extending the FacetEncoding union; the API is shaped to
 * accept them.
 */
import { computed, type ComputedRef } from 'vue'
import type { Ref } from 'vue'
import type {
  FacetKind,
  FilterOption,
} from '@/composables/useListFilters'
import type { AddFilterFacet } from '@/components/views/AddFilterMenu.vue'

/** Subset of `useListControls`'s return that this composable
 *  needs. Declaring the dependency narrowly keeps the composable
 *  callable from tests without standing up the full controls
 *  composable. */
export interface ChipControlsLike {
  filters: Ref<Record<string, string | string[]>>
  handleFilterUpdate: (name: string, value: string | string[]) => void
}

/** Per-facet declaration. `options` is a getter so consumers can
 *  read live translations (Fluent's $t isn't safe to call at
 *  module-load) and reactive sources (user list, etc.) without
 *  the composable having to know about reactivity. */
export interface ChipFacetDef {
  key: string
  /** Fluent key for the facet's display name. Resolved via the
   *  `t` function the composable receives. */
  labelKey: string
  kind: FacetKind
  options: () => FilterOption[]
}

export interface ChipPill {
  facet: string
  kind: FacetKind
  label: string
  valueSummary: string
  options: FilterOption[]
  selected: Set<string>
  textValue: string
}

export interface UseChipFiltersFromControlsOptions {
  controls: ChipControlsLike
  /** Reactive facet list. Use a computed when the facets depend
   *  on locale or workspace capabilities; pass a static array
   *  wrapped in `computed(() => [...])` otherwise. */
  facets: ComputedRef<ChipFacetDef[]>
  /** Translate function. Pass `fluent.$t` from the caller; the
   *  composable doesn't import fluent-vue directly so it stays
   *  testable without the i18n shell. */
  t: (key: string, args?: Record<string, string | number>) => string
}

export interface UseChipFiltersFromControls {
  activeFacets: ComputedRef<string[]>
  addFilterFacets: ComputedRef<AddFilterFacet[]>
  pills: ComputedRef<ChipPill[]>
  optionsFor: (key: string) => FilterOption[]
  selectedFor: (key: string) => Set<string>
  textValueFor: (key: string) => string
  summariseFor: (key: string) => string
  toggleValue: (key: string, value: string) => void
  setText: (key: string, value: string) => void
  clearFacet: (key: string) => void
}

/** Default selection summariser. Lists up to two labels inline,
 *  collapses to "N selected" beyond that or when the joined
 *  string would overflow the pill (32-char budget). */
function summariseSelection(
  selected: Set<string>,
  options: FilterOption[],
  t: (key: string, args?: Record<string, string | number>) => string,
): string {
  if (selected.size === 0) return ''
  if (selected.size > 2) {
    return t('filter-summary-n-selected', { count: selected.size })
  }
  const labels: string[] = []
  for (const v of selected) {
    const opt = options.find((o) => o.value === v)
    labels.push(opt?.label ?? v)
  }
  const joined = labels.join(', ')
  if (joined.length > 32) {
    return t('filter-summary-n-selected', { count: selected.size })
  }
  return joined
}

export function useChipFiltersFromControls(
  options: UseChipFiltersFromControlsOptions,
): UseChipFiltersFromControls {
  const { controls, facets, t } = options

  function facetByKey(key: string): ChipFacetDef | undefined {
    return facets.value.find((f) => f.key === key)
  }

  /** Decode `controls.filters[key]` to a Set<string>. Both the
   *  CSV-string encoding (handleFilterUpdate(key, "a,b,c")) and
   *  the array encoding (string[]) round-trip cleanly. "" and
   *  "all" both mean no filter so they normalise to an empty
   *  set. */
  function selectedFor(key: string): Set<string> {
    const raw = controls.filters.value[key]
    if (typeof raw === 'string') {
      if (raw === '' || raw === 'all') return new Set()
      return new Set(raw.split(',').map((s) => s.trim()).filter(Boolean))
    }
    if (Array.isArray(raw)) return new Set(raw)
    return new Set()
  }

  function optionsFor(key: string): FilterOption[] {
    return facetByKey(key)?.options() ?? []
  }

  function textValueFor(_key: string): string {
    // No text facets in the server-paginated flow yet. When a
    // consumer needs one (eg. backend full-text search exposed
    // as a chip), extend FacetKind handling here.
    return ''
  }

  function summariseFor(key: string): string {
    return summariseSelection(selectedFor(key), optionsFor(key), t)
  }

  function writeSelection(key: string, next: Set<string>): void {
    // "all" is the legacy no-filter sentinel from FilterRow; the
    // requestParams computed in useListControls strips it out
    // before the request goes to the backend, and URL sync omits
    // it so cleared facets don't pollute the URL.
    if (next.size === 0) {
      controls.handleFilterUpdate(key, 'all')
      return
    }
    controls.handleFilterUpdate(key, [...next].join(','))
  }

  function toggleValue(key: string, value: string): void {
    const next = new Set(selectedFor(key))
    if (next.has(value)) next.delete(value)
    else next.add(value)
    writeSelection(key, next)
  }

  function setText(_key: string, _value: string): void {
    // No-op until text-facet support lands. Kept as a stable
    // surface so the AddFilterMenu's @set-text emit can wire to
    // this composable without per-view branching.
  }

  function clearFacet(key: string): void {
    writeSelection(key, new Set())
  }

  const activeFacets = computed<string[]>(() =>
    facets.value
      .filter((f) => selectedFor(f.key).size > 0)
      .map((f) => f.key),
  )

  const addFilterFacets = computed<AddFilterFacet[]>(() =>
    facets.value.map((f) => ({
      key: f.key,
      label: t(f.labelKey),
      kind: f.kind,
    })),
  )

  const pills = computed<ChipPill[]>(() =>
    activeFacets.value.map((key) => {
      const def = facetByKey(key)
      return {
        facet: key,
        kind: def?.kind ?? 'multi',
        label: def ? t(def.labelKey) : key,
        valueSummary: summariseFor(key),
        options: optionsFor(key),
        selected: selectedFor(key),
        textValue: textValueFor(key),
      }
    }),
  )

  return {
    activeFacets,
    addFilterFacets,
    pills,
    optionsFor,
    selectedFor,
    textValueFor,
    summariseFor,
    toggleValue,
    setText,
    clearFacet,
  }
}
