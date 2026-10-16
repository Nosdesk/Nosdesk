<script setup lang="ts">
/**
 * Match values step: what each distinct value in a mapped column means.
 * One card per column with something to read, in source order: values
 * with no match first, then Guessed ones, then the rest; values matched
 * without a question are a count behind "Show". Every value is pre-filled,
 * so the step is for confirming the uncertain few. "Check again" stores
 * the answers and checks the rows with them. The rules live in
 * `valuesModel.ts`.
 */
import { computed, nextTick, ref } from 'vue'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import FormInput from '@/components/common/FormInput.vue'
import SearchableDropdown from '@/components/common/SearchableDropdown.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import SegmentedControl from '@/components/common/SegmentedControl.vue'
import StatusPill from '@/components/common/StatusPill.vue'
import Tooltip from '@/components/common/Tooltip.vue'
import type { DropdownOption } from '@/components/common/dropdownOption'
import { useMobileDetection } from '@/composables/useMobileDetection'
import { useAuthStore } from '@/stores/auth'
import { formatCompactDate } from '@nosdesk/core/utils/dateUtils'
import type {
  ImportField,
  ImportValue,
  ImportValueColumn,
  ImportValues,
  ValueAnswer,
  ValueAnswers,
} from '@nosdesk/core/services/importService'

import {
  PAGE,
  answerFromKey,
  answerKey,
  answersToSend,
  applyBulk,
  bulkActions,
  cards,
  columnsNeeding,
  consequence,
  currentAnswer,
  displayOrder,
  emptyReplacements,
  filterValues,
  isAnswered,
  matchedCount,
  matchedValues,
  nextProblem as nextProblemId,
  problemIds,
  settledColumns,
  skippedNow,
  cardId,
  idOf,
  valuesCsv,
  type BulkAction,
  type Card,
  type Edits,
  type ValueFilter,
} from './valuesModel'

const props = defineProps<{
  view: ImportValues
  fields: ImportField[]
  checking: boolean
}>()

const emit = defineEmits<{
  (e: 'check', answers: ValueAnswers): void
  (e: 'back'): void
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const { isMobile } = useMobileDetection('md')
const authStore = useAuthStore()

const lang = typeof document === 'undefined' ? undefined : document.documentElement.lang || undefined
const fmt = (n: number) => new Intl.NumberFormat(lang).format(n)

const edits = ref<Edits>({})
const filters = ref<Record<string, ValueFilter>>({})
const expanded = ref<Record<string, boolean>>({})
const showMatched = ref<Record<string, boolean>>({})
const tried = ref(false)

const shown = computed<Card[]>(() => cards(props.view, edits.value))
const settled = computed(() => settledColumns(props.view, edits.value))
const needing = computed(() => columnsNeeding(props.view, edits.value))
const skipped = computed(() => skippedNow(props.view, edits.value))
const order = computed(() => displayOrder(shown.value))
const problems = computed(() => problemIds(shown.value, edits.value))
const unfinished = computed(() => emptyReplacements(props.view, edits.value))

const fieldById = computed(() => new Map(props.fields.map((f) => [f.id, f])))

function fieldLabel(id: string): string {
  const field = fieldById.value.get(id)
  if (!field) return id
  return field.label_key ? t(field.label_key) : field.label
}

function title(col: ImportValueColumn): string {
  const args = { header: col.header, field: fieldLabel(col.field) }
  return col.context_label
    ? t('import-values-card-title-kind', { ...args, kind: col.context_label })
    : t('import-values-card-title', args)
}

const isParsed = (col: ImportValueColumn) => col.choices.length === 0

// ---- answers ----

function answerOf(col: ImportValueColumn, v: ImportValue): ValueAnswer | undefined {
  return currentAnswer(col, v, edits.value)
}

function setAnswer(col: ImportValueColumn, v: ImportValue, key: string) {
  const id = idOf(col, v)
  edits.value = { ...edits.value, [id]: answerFromKey(key, answerOf(col, v)) }
}

function replacementOf(col: ImportValueColumn, v: ImportValue): string {
  const a = answerOf(col, v)
  return a?.action === 'replace' ? a.value : ''
}

function setReplacement(col: ImportValueColumn, v: ImportValue, value: string) {
  edits.value = { ...edits.value, [idOf(col, v)]: { action: 'replace', value } }
}

function pickerOptions(col: ImportValueColumn, v: ImportValue): DropdownOption[] {
  const options: DropdownOption[] = col.choices.map((c) => ({
    value: `match:${c.value}`,
    label: c.label,
    description: c.disabled,
    disabled: !!c.disabled,
  }))
  const or = t('import-values-group-or')
  if (isParsed(col)) options.push({ value: 'replace', label: t('import-values-replace') })
  if (v.blank) {
    options.push({
      value: 'leave_blank',
      label: t('import-values-leave-as-is'),
      description: t('import-values-leave-as-is-help'),
      group: or,
    })
  } else if (!col.required) {
    options.push({ value: 'leave_blank', label: t('import-values-leave-blank'), group: or })
  }
  options.push({ value: 'skip', label: t('import-values-skip'), group: or })
  return options
}

/** What a matched value became, for the read-only list. */
function answerLabel(col: ImportValueColumn, v: ImportValue): string {
  const a = answerOf(col, v)
  if (!a) return v.text
  switch (a.action) {
    case 'match':
      return col.choices.find((c) => c.value === String(a.target))?.label ?? String(a.target)
    case 'leave_blank':
      return v.blank ? t('import-values-leave-as-is') : t('import-values-leave-blank')
    case 'skip':
      return t('import-values-skip')
    case 'replace':
      return a.value
    default:
      return ''
  }
}

type Pill = 'guessed' | 'saved' | 'no_match' | null

function pillOf(col: ImportValueColumn, v: ImportValue): Pill {
  if (isAnswered(col, v, edits.value)) return null
  if (v.match === 'guessed' || v.match === 'saved' || v.match === 'no_match') return v.match
  return null
}

function savedText(v: ImportValue): string {
  const p = v.saved
  if (!p) return ''
  const args = { date: formatCompactDate(p.at) }
  if (p.by && p.by === authStore.user?.uuid) return t('import-map-saved-you', args)
  if (p.by_name) return t('import-map-saved-from', { ...args, name: p.by_name })
  return t('import-map-saved-unknown', args)
}

function noteOf(v: ImportValue): string {
  if (v.reason?.code === 'close_to') return t('import-values-close-to', { label: v.reason.label })
  if (v.reason?.code === 'disabled') return v.reason.note
  return ''
}

const CONSEQUENCE_KEYS = {
  skipped: 'import-values-consequence-skipped',
  blank: 'import-values-consequence-blank',
  as_is: 'import-values-consequence-as-is',
} as const

function consequenceText(col: ImportValueColumn, v: ImportValue): string {
  const c = consequence(col, v, edits.value)
  return c ? t(CONSEQUENCE_KEYS[c]) : ''
}

function valueText(v: ImportValue): string {
  return v.blank ? t('import-values-blank') : v.text
}

// ---- cards ----

function filterOf(col: ImportValueColumn): ValueFilter {
  return filters.value[cardId(col)] ?? 'all'
}

function setFilter(col: ImportValueColumn, value: ValueFilter) {
  filters.value = { ...filters.value, [cardId(col)]: value }
}

function filterOptions(card: Card) {
  const count = (m: ImportValue['match']) => card.listed.filter((v) => v.match === m).length
  return [
    { value: 'no_match' as ValueFilter, label: t('import-values-filter-no-match', { count: count('no_match') }) },
    { value: 'guessed' as ValueFilter, label: t('import-values-filter-guessed', { count: count('guessed') }) },
    { value: 'all' as ValueFilter, label: t('import-values-filter-all') },
  ]
}

/** Values above which a card offers the filter. */
const FILTER_MIN = 4

function filtered(card: Card): ImportValue[] {
  return filterValues(card.listed, filterOf(card.column))
}

function visible(card: Card): ImportValue[] {
  const list = filtered(card)
  return expanded.value[cardId(card.column)] ? list : list.slice(0, PAGE)
}

function hiddenCount(card: Card): number {
  return filtered(card).length - visible(card).length
}

function bulkFor(card: Card) {
  return bulkActions(card.column, card.listed, edits.value)
}

function bulkLabel(action: BulkAction, count: number): string {
  return action === 'skip'
    ? t('import-values-bulk-skip', { count })
    : t('import-values-bulk-blank', { count })
}

function bulk(card: Card, action: BulkAction) {
  edits.value = applyBulk(card.column, card.listed, edits.value, action)
}

/** The bulk answer every value of a scaled card has, if they share one. */
function scaleChoice(card: Card): BulkAction | '' {
  const actions = new Set(card.listed.map((v) => answerOf(card.column, v)?.action))
  const only = [...actions][0]
  return actions.size === 1 && (only === 'skip' || only === 'leave_blank') ? only : ''
}

function scaleOptions(card: Card) {
  const options: { value: BulkAction | ''; label: string }[] = []
  if (!card.column.required) options.push({ value: 'leave_blank', label: t('import-values-leave-blank') })
  options.push({ value: 'skip', label: t('import-values-skip') })
  return options
}

function setScaleChoice(card: Card, action: BulkAction | '') {
  if (!action) return
  const next = { ...edits.value }
  for (const v of card.listed) next[idOf(card.column, v)] = { action }
  edits.value = next
}

function download(card: Card) {
  const csv = valuesCsv([t('import-values-col-value'), t('import-values-col-rows')], card.listed)
  const link = document.createElement('a')
  link.href = URL.createObjectURL(new Blob([csv], { type: 'text/csv;charset=utf-8' }))
  const stem = card.column.header || card.column.field
  link.download = card.column.context
    ? `${stem}-${card.column.context}-values.csv`
    : `${stem}-values.csv`
  document.body.appendChild(link)
  link.click()
  document.body.removeChild(link)
  URL.revokeObjectURL(link.href)
}

function toggleMatched(col: ImportValueColumn) {
  showMatched.value = { ...showMatched.value, [cardId(col)]: !showMatched.value[cardId(col)] }
}

/** Settled columns as cards: when nothing asks to be read, the step still
 *  shows what every value became. */
const settledCards = computed(() => (shown.value.length === 0 ? settled.value : []))

const settledText = computed(() =>
  t('import-values-all-matched', {
    headers: [...new Set(settled.value.map((c) => c.header))].join(', '),
  }),
)

// ---- next problem ----

const root = ref<HTMLElement | null>(null)
let lastProblem: string | null = null

async function nextProblem() {
  const id = nextProblemId(order.value, problems.value, lastProblem)
  if (!id) return
  lastProblem = id
  const card = shown.value.find((c) => c.listed.some((v) => idOf(c.column, v) === id))
  if (card) {
    const index = filtered(card).findIndex((v) => idOf(card.column, v) === id)
    if (index < 0) setFilter(card.column, 'all')
    if (index < 0 || index >= PAGE) expanded.value = { ...expanded.value, [cardId(card.column)]: true }
    await nextTick()
  }
  const row = [...(root.value?.querySelectorAll<HTMLElement>('[data-value-id]') ?? [])].find(
    (el) => el.dataset.valueId === id,
  )
  const trigger = row?.querySelector<HTMLElement>('[data-value-picker]')
  row?.scrollIntoView?.({ block: 'center', behavior: 'smooth' })
  trigger?.focus({ preventScroll: true })
}

// ---- action bar ----

function checkAgain() {
  tried.value = true
  if (unfinished.value.length > 0) return
  emit('check', answersToSend(props.view, edits.value))
}

const linkClass =
  'text-accent hover:underline focus:outline-none focus-visible:ring-1 focus-visible:ring-accent rounded'
const domId = (col: ImportValueColumn, v: ImportValue) =>
  `import-value-${col.index}-${encodeURIComponent(v.key).replace(/%/g, '_')}`
</script>

<template>
  <section ref="root" class="flex flex-col gap-6" data-test="import-values">
    <!-- Strip: live counts and the next value to read. Opaque, so cards
         scrolled under it don't show through. -->
    <div
      data-test="import-values-strip"
      class="sticky top-0 z-10 bg-app border-b border-default py-2 sm:py-3 flex flex-wrap items-center gap-x-3 gap-y-2"
    >
      <p class="text-sm text-secondary flex flex-wrap items-center gap-x-2" aria-live="polite">
        <span>{{ $t('import-values-strip-checked', { count: view.rows_checked }) }}</span>
        <span aria-hidden="true">·</span>
        <span data-test="import-values-needing">{{ $t('import-values-strip-columns', { count: needing }) }}</span>
        <span aria-hidden="true">·</span>
        <span
          data-test="import-values-skipped"
          :class="skipped > 0 ? 'text-status-error' : ''"
        >{{ skipped > 0
          ? $t('import-values-strip-skipped', { count: skipped })
          : $t('import-values-strip-none-skipped') }}</span>
      </p>
      <Button
        v-if="problems.length > 0"
        class="ml-auto"
        variant="ghost"
        size="sm"
        data-test="import-values-next"
        @click="nextProblem"
      >
        {{ $t('import-map-next-problem') }}
      </Button>
    </div>

    <template v-for="card in shown" :key="cardId(card.column)">
      <!-- Too many values to list: counts and one answer for the rest. -->
      <SectionCard
        v-if="card.scaled"
        content-padding="p-4"
        data-test="import-values-scale"
        :data-field="card.column.field"
      >
        <template #title>{{ title(card.column) }}</template>
        <div class="flex flex-col gap-3">
          <p class="text-sm text-primary">
            {{ $t('import-values-scale', { total: card.column.distinct, rest: card.listed.length }) }}
          </p>
          <SegmentedControl
            class="self-start"
            :model-value="scaleChoice(card)"
            :options="scaleOptions(card)"
            size="sm"
            :aria-label="$t('import-values-scale-label')"
            @update:model-value="setScaleChoice(card, $event)"
          />
          <div>
            <Button variant="secondary" size="sm" icon="download" @click="download(card)">
              {{ $t('import-values-scale-download') }}
            </Button>
          </div>
        </div>
      </SectionCard>

      <SectionCard
        v-else
        content-padding=""
        data-test="import-values-card"
        :data-field="card.column.field"
      >
        <template #title>{{ title(card.column) }}</template>
        <div class="flex flex-col">
          <div v-if="card.listed.length >= FILTER_MIN" class="px-3 pt-3 flex">
            <SegmentedControl
              :model-value="filterOf(card.column)"
              :options="filterOptions(card)"
              size="sm"
              :aria-label="$t('import-values-filter-label')"
              @update:model-value="setFilter(card.column, $event)"
            />
          </div>

          <!-- Desktop: a semantic table. -->
          <table v-if="!isMobile && visible(card).length > 0" class="w-full table-fixed text-sm">
            <thead>
              <tr class="text-left text-xs uppercase tracking-wide text-tertiary">
                <th scope="col" class="px-3 py-2 font-medium">{{ $t('import-values-col-value') }}</th>
                <th scope="col" class="px-3 py-2 w-20 font-medium text-right">{{ $t('import-values-col-rows') }}</th>
                <th scope="col" class="px-3 py-2 w-[240px] font-medium">{{ $t('import-values-col-becomes') }}</th>
                <th scope="col" class="px-3 py-2 w-[200px] font-medium"><span class="sr-only">{{ $t('import-values-col-status') }}</span></th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="v in visible(card)"
                :key="v.key"
                :data-value-id="idOf(card.column, v)"
                data-test="import-value-row"
                class="border-t border-default"
              >
                <th scope="row" class="px-3 py-2 align-top text-left font-normal">
                  <span
                    class="block truncate"
                    :class="v.blank ? 'text-tertiary italic' : 'font-mono text-primary'"
                    :title="v.text"
                  >{{ valueText(v) }}</span>
                </th>
                <td class="px-3 py-2 align-top text-right text-tertiary tabular-nums">{{ fmt(v.rows.length) }}</td>
                <td class="px-3 py-2 align-top">
                  <div class="flex flex-col gap-1.5">
                    <SearchableDropdown
                      :model-value="answerKey(answerOf(card.column, v))"
                      :options="pickerOptions(card.column, v)"
                      size="sm"
                      :aria-label="$t('import-values-picker-label', { value: valueText(v) })"
                      :described-by="domId(card.column, v)"
                      data-value-picker
                      @update:model-value="setAnswer(card.column, v, $event)"
                    />
                    <FormInput
                      v-if="answerOf(card.column, v)?.action === 'replace'"
                      :model-value="replacementOf(card.column, v)"
                      :inputmode="card.column.value_type === 'number' ? 'decimal' : undefined"
                      size="sm"
                      :placeholder="$t(card.column.value_type === 'date' ? 'import-values-replace-date' : 'import-values-replace-number')"
                      :aria-label="$t('import-values-replace-label', { value: valueText(v) })"
                      :error="tried && !replacementOf(card.column, v).trim() ? $t('import-values-replace-needed') : undefined"
                      data-test="import-value-replace"
                      @update:model-value="setReplacement(card.column, v, String($event ?? ''))"
                    />
                  </div>
                </td>
                <td :id="domId(card.column, v)" class="px-3 py-2 align-top">
                  <div class="flex flex-col gap-1 text-xs">
                    <div class="flex flex-wrap items-center gap-2 min-h-[1.5rem]">
                      <StatusPill
                        v-if="pillOf(card.column, v) === 'no_match'"
                        data-test="pill-no-match"
                        :label="$t('import-values-pill-no-match')"
                        tone="critical"
                      />
                      <StatusPill
                        v-else-if="pillOf(card.column, v) === 'guessed'"
                        data-test="pill-guessed"
                        :label="$t('import-map-pill-guessed')"
                        tone="caution"
                      />
                      <Tooltip v-else-if="pillOf(card.column, v) === 'saved'" :text="savedText(v)">
                        <span
                          tabindex="0"
                          class="inline-flex rounded focus:outline-none focus-visible:ring-1 focus-visible:ring-accent"
                          data-test="pill-saved"
                        >
                          <StatusPill :label="$t('import-map-pill-saved')" tone="neutral" :dot="false" />
                          <span class="sr-only">{{ savedText(v) }}</span>
                        </span>
                      </Tooltip>
                      <span
                        v-if="consequenceText(card.column, v)"
                        data-test="import-value-consequence"
                        :class="consequence(card.column, v, edits) === 'skipped' ? 'text-status-error' : 'text-tertiary'"
                      >{{ consequenceText(card.column, v) }}</span>
                    </div>
                    <span v-if="noteOf(v)" class="text-tertiary">{{ noteOf(v) }}</span>
                  </div>
                </td>
              </tr>
            </tbody>
          </table>

          <!-- Phone: one item per value. -->
          <ul v-else-if="visible(card).length > 0" class="flex flex-col">
            <li
              v-for="v in visible(card)"
              :key="v.key"
              :data-value-id="idOf(card.column, v)"
              data-test="import-value-row"
              class="border-t border-default first:border-t-0 p-3 flex flex-col gap-2"
            >
              <div class="flex flex-wrap items-center gap-x-2 gap-y-1 min-w-0">
                <span
                  class="text-sm truncate"
                  :class="v.blank ? 'text-tertiary italic' : 'font-mono font-medium text-primary'"
                >{{ valueText(v) }}</span>
                <span class="text-xs text-tertiary">{{ $t('import-values-row-count', { count: v.rows.length }) }}</span>
                <StatusPill
                  v-if="pillOf(card.column, v) === 'no_match'"
                  data-test="pill-no-match"
                  :label="$t('import-values-pill-no-match')"
                  tone="critical"
                />
                <StatusPill
                  v-else-if="pillOf(card.column, v) === 'guessed'"
                  data-test="pill-guessed"
                  :label="$t('import-map-pill-guessed')"
                  tone="caution"
                />
                <StatusPill
                  v-else-if="pillOf(card.column, v) === 'saved'"
                  data-test="pill-saved"
                  :label="$t('import-map-pill-saved')"
                  tone="neutral"
                  :dot="false"
                />
              </div>
              <SearchableDropdown
                :model-value="answerKey(answerOf(card.column, v))"
                :options="pickerOptions(card.column, v)"
                size="md"
                :aria-label="$t('import-values-picker-label', { value: valueText(v) })"
                :described-by="domId(card.column, v)"
                data-value-picker
                @update:model-value="setAnswer(card.column, v, $event)"
              />
              <FormInput
                v-if="answerOf(card.column, v)?.action === 'replace'"
                :model-value="replacementOf(card.column, v)"
                :inputmode="card.column.value_type === 'number' ? 'decimal' : undefined"
                size="md"
                :placeholder="$t(card.column.value_type === 'date' ? 'import-values-replace-date' : 'import-values-replace-number')"
                :aria-label="$t('import-values-replace-label', { value: valueText(v) })"
                :error="tried && !replacementOf(card.column, v).trim() ? $t('import-values-replace-needed') : undefined"
                data-test="import-value-replace"
                @update:model-value="setReplacement(card.column, v, String($event ?? ''))"
              />
              <div :id="domId(card.column, v)" class="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs">
                <span
                  v-if="consequenceText(card.column, v)"
                  :class="consequence(card.column, v, edits) === 'skipped' ? 'text-status-error' : 'text-tertiary'"
                >{{ consequenceText(card.column, v) }}</span>
                <span v-if="noteOf(v)" class="text-tertiary">{{ noteOf(v) }}</span>
                <span v-if="pillOf(card.column, v) === 'saved'" class="text-tertiary">{{ savedText(v) }}</span>
              </div>
            </li>
          </ul>

          <p v-else class="px-3 py-6 text-sm text-tertiary text-center">
            {{ $t('import-values-empty-filter') }}
          </p>

          <div
            v-if="hiddenCount(card) > 0"
            class="px-3 py-2 border-t border-default text-sm"
          >
            <button
              type="button"
              :class="linkClass"
              data-test="import-values-more"
              @click="expanded = { ...expanded, [cardId(card.column)]: true }"
            >
              {{ $t('import-values-show-more', { count: hiddenCount(card) }) }}
            </button>
          </div>

          <!-- Matched without a question: a count, the list behind Show. -->
          <div
            v-if="matchedCount(card.column, edits) > 0"
            class="px-3 py-2 border-t border-default text-sm text-secondary flex flex-wrap items-center gap-x-2"
            data-test="import-values-matched"
          >
            <span>{{ $t('import-values-matched', { count: matchedCount(card.column, edits) }) }}</span>
            <template v-if="matchedValues(card.column, edits).length > 0">
              <span aria-hidden="true" class="text-tertiary">·</span>
              <button
                type="button"
                :class="linkClass"
                :aria-expanded="!!showMatched[cardId(card.column)]"
                @click="toggleMatched(card.column)"
              >
                {{ showMatched[cardId(card.column)] ? $t('import-values-hide') : $t('import-values-show') }}
              </button>
            </template>
          </div>
          <ul
            v-if="showMatched[cardId(card.column)]"
            class="px-3 pb-3 flex flex-col gap-1 text-sm"
            data-test="import-values-matched-list"
          >
            <li
              v-for="v in matchedValues(card.column, edits)"
              :key="v.key"
              class="flex flex-wrap items-center gap-x-3 gap-y-1"
            >
              <span class="font-mono text-primary">{{ valueText(v) }}</span>
              <span class="text-xs text-tertiary">{{ $t('import-values-row-count', { count: v.rows.length }) }}</span>
              <span class="text-secondary">→ {{ answerLabel(card.column, v) }}</span>
              <StatusPill v-if="v.match === 'saved'" :label="$t('import-map-pill-saved')" tone="neutral" :dot="false" />
            </li>
          </ul>

          <!-- Bulk answers. -->
          <div
            v-if="bulkFor(card).length > 0"
            class="px-3 py-2 border-t border-default flex flex-wrap items-center gap-2"
            data-test="import-values-bulk"
          >
            <Button
              v-for="b in bulkFor(card)"
              :key="b.action"
              variant="secondary"
              size="sm"
              :data-bulk="b.action"
              @click="bulk(card, b.action)"
            >
              {{ bulkLabel(b.action, b.count) }}
            </Button>
          </div>
        </div>
      </SectionCard>
    </template>

    <!-- Nothing to read: what every value became, column by column. -->
    <SectionCard
      v-for="col in settledCards"
      :key="cardId(col)"
      content-padding=""
      data-test="import-values-settled-card"
    >
      <template #title>{{ title(col) }}</template>
      <div class="px-3 py-2 text-sm text-secondary flex flex-wrap items-center gap-x-2">
        <span>{{ $t('import-values-matched', { count: matchedCount(col, edits) }) }}</span>
        <template v-if="matchedValues(col, edits).length > 0">
          <span aria-hidden="true" class="text-tertiary">·</span>
          <button type="button" :class="linkClass" @click="toggleMatched(col)">
            {{ showMatched[cardId(col)] ? $t('import-values-hide') : $t('import-values-show') }}
          </button>
        </template>
      </div>
      <ul v-if="showMatched[cardId(col)]" class="px-3 pb-3 flex flex-col gap-1 text-sm">
        <li v-for="v in matchedValues(col, edits)" :key="v.key" class="flex flex-wrap items-center gap-x-3 gap-y-1">
          <span class="font-mono text-primary">{{ valueText(v) }}</span>
          <span class="text-xs text-tertiary">{{ $t('import-values-row-count', { count: v.rows.length }) }}</span>
          <span class="text-secondary">→ {{ answerLabel(col, v) }}</span>
          <StatusPill v-if="v.match === 'saved'" :label="$t('import-map-pill-saved')" tone="neutral" :dot="false" />
        </li>
      </ul>
    </SectionCard>

    <p
      v-if="shown.length > 0 && settled.length > 0"
      class="text-sm text-secondary"
      data-test="import-values-settled"
    >
      {{ settledText }}
    </p>

    <!-- Action bar. -->
    <div class="sticky bottom-0 bg-app border-t border-default py-3 flex flex-wrap items-center gap-3">
      <Button variant="ghost" :disabled="checking" data-test="import-values-back" @click="emit('back')">
        {{ $t('import-values-back') }}
      </Button>
      <span v-if="tried && unfinished.length > 0" class="ml-auto text-xs text-status-error">
        {{ $t('import-values-replace-missing', { count: unfinished.length }) }}
      </span>
      <Button
        :class="tried && unfinished.length > 0 ? '' : 'ml-auto'"
        :loading="checking"
        :disabled="checking"
        data-test="import-values-check"
        @click="checkAgain"
      >
        {{ checking ? $t('import-map-checking') : $t('import-values-check-again') }}
      </Button>
    </div>
  </section>
</template>
