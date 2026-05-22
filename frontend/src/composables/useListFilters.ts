/**
 * Dataset-agnostic multi-facet filter state.
 *
 * Tickets, assets, and users all want the same chip-based filter
 * UX: a row of removable amber pills, a "+ Add filter" entry
 * point, multi-select popovers, and a single text facet for
 * title/name search. The data shape per facet differs (status is
 * a number id on tickets, kind is a string slug on assets, role
 * is an enum on users), but the predicate composition is
 * uniform: AND across facets, OR within a facet's selection.
 *
 * This composable holds that uniform state. Consumers declare an
 * array of FacetDef<T, Ctx> describing each filter: how to render
 * its options, how to test whether an item matches the current
 * selection, and how to summarise the selection in the pill body.
 * The composable wires up state, the predicate, activeFacets, and
 * the toggle/clear helpers; the FilterPill and AddFilterMenu
 * components consume them generically.
 */
import { computed, reactive, type ComputedRef } from 'vue'
import { translate } from '@/i18n'

/** Option rendered in a value-picker popover. Generic across
 *  datasets: `value` is the canonical string id (number facets
 *  stringify on the way in, parse on the way out via the facet's
 *  matches function). */
export interface FilterOption {
  value: string
  label: string
  swatchClass?: string
  hint?: string
}

/** Per-facet selection state. The composable keeps a uniform
 *  `Set<string>` for multi facets and a plain string for text
 *  facets, so the chip UI can render them without per-facet
 *  branching. */
export type FacetSelection =
  | { kind: 'text'; value: string }
  | { kind: 'multi'; values: Set<string> }

export type FacetKind = 'text' | 'multi'

/**
 * Declarative facet definition. `T` is the item type (CardData,
 * Asset, User); `Ctx` is whatever side context the facet needs
 * to resolve options (eg. a user directory for the assignee
 * facet, the live source list for "derive options from data"
 * facets). Pass `void` for Ctx if no context is needed.
 */
export interface FacetDef<T, Ctx = void> {
  /** Stable string key. Used in the URL, in the pill, in saved
   *  views. Must be unique within a facet set. */
  key: string
  /** Fluent key for the facet's display name. */
  labelKey: string
  kind: FacetKind
  /** Multi facets: produce options from current data + context.
   *  Called every render via a computed, so keep it cheap; use
   *  Map-based deduping for derived-from-data options. */
  optionsFrom?: (items: readonly T[], ctx: Ctx) => FilterOption[]
  /** Predicate. Return true when `item` matches the given
   *  selection. Called inside the main predicate so keep allocs
   *  out of the hot path. The composable filters the call out
   *  entirely when the selection is empty, so this can assume
   *  the selection is non-empty. */
  matches: (item: T, selection: FacetSelection, ctx: Ctx) => boolean
  /** Optional summary override. When omitted the composable uses
   *  the default "N selected" / first-two-labels strategy. Text
   *  facets default to `"value"`. */
  summarise?: (selection: FacetSelection, options: FilterOption[]) => string
}

export interface UseListFiltersOptions<T, Ctx> {
  facets: FacetDef<T, Ctx>[]
  /** Reactive accessor for the side context. Passed by getter so
   *  the composable doesn't pin a snapshot. */
  context: () => Ctx
}

export interface UseListFilters<T, Ctx> {
  /** Reactive per-facet selection state keyed by facet.key. */
  selections: Record<string, FacetSelection>
  predicate: ComputedRef<(item: T) => boolean>
  /** Facet keys with non-empty selections, in declaration order. */
  activeFacets: ComputedRef<string[]>
  /** Resolve a facet's options against current data + context.
   *  Returns [] for text facets. */
  optionsFor: (key: string, items: readonly T[]) => FilterOption[]
  /** Read selection in a popover-friendly shape: always a
   *  Set<string> regardless of underlying type. */
  selectedFor: (key: string) => Set<string>
  /** Read text-facet value (empty string for multi facets). */
  textValueFor: (key: string) => string
  summariseFor: (key: string, items: readonly T[]) => string
  toggleValue: (key: string, value: string) => void
  setText: (key: string, value: string) => void
  clearFacet: (key: string) => void
  clearAll: () => void
  facetDefs: FacetDef<T, Ctx>[]
}

function defaultSummary(
  selection: FacetSelection,
  options: FilterOption[],
): string {
  if (selection.kind === 'text') {
    return selection.value.length > 0 ? `"${selection.value}"` : ''
  }
  const size = selection.values.size
  if (size === 0) return ''
  if (size > 2) {
    return translate(
      'filter-summary-n-selected',
      { count: size },
      `${size} selected`,
    )
  }
  const labels: string[] = []
  for (const v of selection.values) {
    const opt = options.find((o) => o.value === v)
    labels.push(opt?.label ?? v)
  }
  const joined = labels.join(', ')
  if (joined.length > 32) {
    return translate(
      'filter-summary-n-selected',
      { count: size },
      `${size} selected`,
    )
  }
  return joined
}

function isActive(selection: FacetSelection): boolean {
  return selection.kind === 'text'
    ? selection.value.trim().length > 0
    : selection.values.size > 0
}

function emptySelection(kind: FacetKind): FacetSelection {
  return kind === 'text'
    ? { kind: 'text', value: '' }
    : { kind: 'multi', values: new Set<string>() }
}

export function useListFilters<T, Ctx = void>(
  options: UseListFiltersOptions<T, Ctx>,
): UseListFilters<T, Ctx> {
  const { facets, context } = options
  const defsByKey = new Map(facets.map((f) => [f.key, f]))

  const selections = reactive<Record<string, FacetSelection>>({})
  for (const f of facets) {
    selections[f.key] = emptySelection(f.kind)
  }

  function selection(key: string): FacetSelection {
    return selections[key] ?? emptySelection('multi')
  }

  const activeFacets = computed<string[]>(() =>
    facets.filter((f) => isActive(selection(f.key))).map((f) => f.key),
  )

  const predicate = computed<(item: T) => boolean>(() => {
    // Snapshot active facets once so the inner closure isn't
    // chasing reactive sources per item.
    const ctx = context()
    const active = facets
      .filter((f) => isActive(selection(f.key)))
      .map((f) => ({ def: f, sel: selection(f.key) }))
    if (active.length === 0) return () => true
    return (item: T) => {
      for (const { def, sel } of active) {
        if (!def.matches(item, sel, ctx)) return false
      }
      return true
    }
  })

  function optionsFor(key: string, items: readonly T[]): FilterOption[] {
    const def = defsByKey.get(key)
    if (!def || def.kind !== 'multi' || !def.optionsFrom) return []
    return def.optionsFrom(items, context())
  }

  function selectedFor(key: string): Set<string> {
    const sel = selection(key)
    return sel.kind === 'multi' ? sel.values : new Set()
  }

  function textValueFor(key: string): string {
    const sel = selection(key)
    return sel.kind === 'text' ? sel.value : ''
  }

  function summariseFor(key: string, items: readonly T[]): string {
    const def = defsByKey.get(key)
    if (!def) return ''
    const sel = selection(key)
    const opts = def.kind === 'multi' ? optionsFor(key, items) : []
    if (def.summarise) return def.summarise(sel, opts)
    return defaultSummary(sel, opts)
  }

  function toggleValue(key: string, value: string): void {
    const def = defsByKey.get(key)
    if (!def || def.kind !== 'multi') return
    const sel = selection(key)
    if (sel.kind !== 'multi') return
    // Reassign the set so reactivity fires; mutating in place
    // works under Vue 3's deep reactive proxy, but reassigning
    // matches how the tickets composable behaves today and keeps
    // diffing trivial.
    const next = new Set(sel.values)
    if (next.has(value)) next.delete(value)
    else next.add(value)
    selections[key] = { kind: 'multi', values: next }
  }

  function setText(key: string, value: string): void {
    const def = defsByKey.get(key)
    if (!def || def.kind !== 'text') return
    selections[key] = { kind: 'text', value }
  }

  function clearFacet(key: string): void {
    const def = defsByKey.get(key)
    if (!def) return
    selections[key] = emptySelection(def.kind)
  }

  function clearAll(): void {
    for (const f of facets) selections[f.key] = emptySelection(f.kind)
  }

  return {
    selections,
    predicate,
    activeFacets,
    optionsFor,
    selectedFor,
    textValueFor,
    summariseFor,
    toggleValue,
    setText,
    clearFacet,
    clearAll,
    facetDefs: facets,
  }
}
