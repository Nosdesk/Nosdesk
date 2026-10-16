<script setup lang="ts">
/**
 * Saved matches: what each spelling a file used became in the workspace,
 * per import type, as Match values saved it when someone pressed Import.
 * One table with a section per field (a sub-section per manufacturer or
 * kind where the field is answered in one), search above ten entries,
 * per-entry and Delete all. Below it, the header memory of the type's
 * latest import, read only. Editing an entry is delete, then answer again
 * on the next import. The rules live in `matchesModel.ts`.
 *
 * Reached from the hub card, the Match values footer (`?from=values&job=`,
 * so Back returns there) and the Saved match pill.
 */
import { computed, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { useRoute, useRouter } from 'vue-router'
import { useQuery, useQueryCache } from '@pinia/colada'

import Button from '@/components/common/Button.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import Icon from '@/components/common/Icon.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import SegmentedControl from '@/components/common/SegmentedControl.vue'
import { assetStatusLabel } from '@/utils/assetStatusMeta'
import { formatCompactDate } from '@nosdesk/core/utils/dateUtils'
import { useToastStore } from '@nosdesk/core/stores/toast'
import {
  importAnswersQueryKey,
  importService,
  type ImportJobType,
  type SavedColumn,
  type SavedMatch,
  type SavedMatches,
} from '@nosdesk/core/services/importService'

import { detailPath } from '@/components/import/historyModel'
import { SEARCH_FROM, firstName, isDiscard, sections } from '@/components/import/matchesModel'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const route = useRoute()
const router = useRouter()
const toast = useToastStore()
const queryCache = useQueryCache()

const TYPES: ImportJobType[] = ['assets', 'users', 'tickets']

function useAnswers(type: ImportJobType) {
  return useQuery({
    key: () => importAnswersQueryKey(type),
    query: () => importService.answers(type),
  })
}
const queries = {
  assets: useAnswers('assets'),
  users: useAnswers('users'),
  tickets: useAnswers('tickets'),
}
const countOf = (type: ImportJobType) => queries[type].data.value?.entries.length ?? 0

const queryType = (): ImportJobType => {
  const q = route.query.type
  return TYPES.find((t) => t === q) ?? 'assets'
}
const type = ref<ImportJobType>(queryType())
watch(type, (value) => {
  search.value = ''
  router.replace({ query: { ...route.query, type: value } })
})

const typeOptions = computed(() =>
  TYPES.filter((ty) => ty !== 'tickets' || countOf('tickets') > 0 || type.value === 'tickets').map(
    (ty) => ({ value: ty, label: t('import-matches-type-option', { type: ty, count: countOf(ty) }) }),
  ),
)

const current = computed(() => queries[type.value])
const view = computed<SavedMatches | undefined>(() => current.value.data.value)
const entries = computed<SavedMatch[]>(() => view.value?.entries ?? [])
const noun = computed(() => t('import-matches-noun', { type: type.value }))
const nouns = computed(() => t('import-matches-nouns', { type: type.value }))

// ---- labels ----

function fieldLabel(e: { label_key?: string; field_label?: string; field?: string }): string {
  return e.label_key ? t(e.label_key) : (e.field_label ?? e.field ?? '')
}

function contextLabel(field: string, label: string | null): string {
  if (label) return label
  return field === 'model' ? t('import-matches-no-manufacturer') : ''
}

/** What the entry becomes, as the row says it. */
function outcomeText(e: SavedMatch): string {
  switch (e.outcome) {
    case 'link':
      if (type.value === 'assets' && e.field === 'status' && e.target) {
        return assetStatusLabel(t, e.target)
      }
      return e.label ?? e.target ?? ''
    case 'kept_as_text':
      return t('import-matches-kept-as-text')
    case 'left_as_is':
      return t('import-matches-left-as-is')
    case 'left_off':
      return t('import-matches-left-off')
    case 'left_unassigned':
      return t('import-matches-left-unassigned')
    default:
      return t('import-matches-left-blank')
  }
}

function savedByText(e: SavedMatch): string {
  const name = e.saved.by && e.saved.by_name ? firstName(e.saved.by_name) : t('import-matches-former-member')
  return t('import-matches-saved-by', { name, date: formatCompactDate(e.saved.at) })
}

function describe(e: SavedMatch): string {
  return [fieldLabel(e), contextLabel(e.field, e.context_label ?? null), outcomeText(e)].join(' ')
}

// ---- search ----

const search = ref('')
const searchable = computed(() => entries.value.length > SEARCH_FROM)
const listed = computed(() => sections(entries.value, searchable.value ? search.value : '', describe))
const filtering = computed(() => searchable.value && search.value.trim() !== '')

function sectionCount(shown: number, total: number): string {
  return filtering.value ? t('import-matches-section-filtered', { shown, total }) : String(total)
}

// ---- back ----

const fromValues = computed(() => route.query.from === 'values' && typeof route.query.job === 'string')
const backLabel = computed(() =>
  fromValues.value ? t('import-matches-back-values') : t('import-matches-back-hub'),
)
function back() {
  if (fromValues.value) {
    router.push({ path: '/admin/data-import/csv', query: { job: route.query.job as string, step: 'values' } })
  } else {
    router.push('/admin/data-import')
  }
}

/** The import that saved a match: its own page. */
function importLink(jobId: string) {
  return detailPath(jobId)
}

// ---- delete ----

const deleting = ref<string | null>(null)

function dropFromCache(jobType: ImportJobType, keep: (e: SavedMatch) => boolean) {
  const key = importAnswersQueryKey(jobType)
  const data = queryCache.getQueryData<SavedMatches>(key)
  if (data) queryCache.setQueryData(key, { ...data, entries: data.entries.filter(keep) })
}

async function deleteOne(e: SavedMatch) {
  if (deleting.value) return
  deleting.value = e.id
  const jobType = type.value
  try {
    await importService.deleteAnswer(e.id)
    dropFromCache(jobType, (x) => x.id !== e.id)
    toast.success(t('import-matches-deleted-one', { noun: noun.value, value: e.text }))
  } catch {
    toast.error(t('import-matches-delete-failed'))
  } finally {
    deleting.value = null
    queryCache.invalidateQueries({ key: importAnswersQueryKey(jobType) })
  }
}

const confirmAll = ref(false)
const deletingAll = ref(false)

async function deleteAll() {
  const jobType = type.value
  deletingAll.value = true
  try {
    await importService.deleteAnswers(jobType)
    dropFromCache(jobType, () => false)
    confirmAll.value = false
    toast.success(t('import-matches-deleted-all', { noun: noun.value }))
  } catch {
    toast.error(t('import-matches-delete-failed'))
  } finally {
    deletingAll.value = false
    queryCache.invalidateQueries({ key: importAnswersQueryKey(jobType) })
  }
}

// ---- columns ----

const showColumns = ref(false)
const columns = computed(() => view.value?.columns ?? null)

function columnsWho(): string {
  const c = columns.value
  if (!c) return ''
  const date = formatCompactDate(c.at)
  return c.by && c.by_name
    ? t('import-matches-who', { name: firstName(c.by_name), date })
    : t('import-matches-who-unknown', { date })
}

const columnsSummary = computed(() => {
  const c = columns.value
  if (!c) return ''
  const mapped = c.rows.filter((r) => r.field).length
  const off = c.rows.filter((r) => r.ignored)
  const parts = [
    t('import-matches-columns-from', { count: c.rows.length, who: columnsWho(), mapped }),
  ]
  if (off.length > 0) {
    parts.push(
      t('import-matches-columns-off', {
        count: off.length,
        list: off.map((r) => `'${r.header}'`).join(', '),
      }),
    )
  }
  parts.push(t('import-matches-columns-replaces', { noun: noun.value }))
  return parts.join(' ')
})

function columnTarget(r: SavedColumn): string {
  if (r.field) return fieldLabel(r)
  return r.ignored ? t('import-map-ignore') : t('import-matches-no-field')
}

const linkClass = 'text-accent hover:underline focus:outline-none focus-visible:ring-2 focus-visible:ring-accent rounded'
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-4xl">
      <div>
        <button
          type="button"
          class="inline-flex items-center gap-1 h-8 px-1 rounded text-sm text-secondary hover:text-primary focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          data-test="matches-back"
          @click="back"
        >
          <Icon name="chevronLeft" size="sm" />
          {{ backLabel }}
        </button>
      </div>

      <div class="flex flex-col gap-2">
        <h1 class="text-2xl font-bold text-primary">{{ $t('import-matches-title') }}</h1>
        <p class="text-secondary">{{ $t('import-matches-subtitle') }}</p>
      </div>

      <div class="flex flex-wrap items-center gap-3">
        <SegmentedControl
          v-model="type"
          :options="typeOptions"
          :aria-label="$t('import-matches-type-label')"
          size="sm"
        />
      </div>

      <p v-if="current.error.value" class="text-sm text-status-error flex flex-wrap items-center gap-2">
        {{ $t('import-matches-load-failed') }}
        <button type="button" :class="linkClass" @click="current.refetch()">{{ $t('import-matches-retry') }}</button>
      </p>

      <!-- Values -->
      <SectionCard content-padding="" data-test="matches-values">
        <template #title>{{ $t('import-matches-values-title', { count: entries.length }) }}</template>

        <p v-if="view && entries.length === 0" class="px-3 py-4 text-sm text-secondary" data-test="matches-empty">
          {{ $t('import-matches-empty') }}
        </p>

        <template v-else-if="entries.length > 0">
          <div
            v-if="searchable"
            class="flex flex-wrap items-center gap-x-3 gap-y-2 px-3 py-2 border-b border-default"
          >
            <div class="relative flex-1 min-w-[10rem] max-w-64 max-md:max-w-none max-md:basis-full">
              <span class="absolute inset-y-0 left-0 flex items-center pl-2 pointer-events-none text-tertiary">
                <Icon name="search" size="sm" />
              </span>
              <input
                v-model="search"
                type="search"
                :placeholder="$t('import-matches-search')"
                :aria-label="$t('import-matches-search-label')"
                class="block w-full py-1 pl-7 pr-2 text-sm rounded-lg bg-surface-alt border border-default text-primary placeholder-tertiary hover:border-strong focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent"
                data-test="matches-search"
              />
            </div>
            <span class="text-xs text-tertiary" data-test="matches-count">
              {{
                filtering
                  ? $t('import-matches-count-filtered', { shown: listed.shown, total: listed.total })
                  : $t('import-matches-count', { count: listed.total })
              }}
            </span>
          </div>

          <table class="mx-tbl w-full text-sm">
            <colgroup>
              <col class="c-src" />
              <col class="c-arr" />
              <col />
              <col class="c-who" />
              <col class="c-act" />
            </colgroup>
            <thead>
              <tr>
                <th scope="col" class="src">{{ $t('import-matches-col-file') }}</th>
                <th scope="col" class="arr"><span class="sr-only">{{ $t('import-matches-becomes') }}</span></th>
                <th scope="col">{{ $t('import-matches-col-workspace') }}</th>
                <th scope="col" class="who">{{ $t('import-matches-col-saved-by') }}</th>
                <th scope="col" class="act"><span class="sr-only">{{ $t('import-matches-actions') }}</span></th>
              </tr>
            </thead>
            <tbody v-for="section in listed.sections" :key="section.field" data-test="matches-section">
              <tr class="mx-sec">
                <th colspan="5" scope="rowgroup">
                  {{ fieldLabel(section.subs[0]?.entries[0] ?? { field: section.field }) }}
                  <span class="n" data-test="matches-section-count">{{ sectionCount(section.shown, section.total) }}</span>
                </th>
              </tr>
              <template v-for="sub in section.subs" :key="sub.context">
                <tr v-if="!section.flat" class="mx-sub" data-test="matches-sub">
                  <th colspan="5" scope="rowgroup">{{ contextLabel(section.field, sub.label) }}</th>
                </tr>
                <tr
                  v-for="e in sub.entries"
                  :key="e.id"
                  class="mx-row"
                  :class="{ in: !section.flat }"
                  data-test="matches-row"
                >
                  <td class="src" :title="e.text">{{ e.text }}</td>
                  <td class="arr" aria-hidden="true">→</td>
                  <td class="dst" :class="{ discard: isDiscard(e) }" :title="outcomeText(e)">
                    {{ outcomeText(e) }}<span v-if="e.created" class="note">{{ $t('import-matches-created') }}</span>
                  </td>
                  <td class="who" :title="savedByText(e)">
                    <RouterLink
                      v-if="e.saved.job_id"
                      :to="importLink(e.saved.job_id)"
                      :title="$t('import-matches-open-import')"
                      class="hover:underline focus:outline-none focus-visible:ring-2 focus-visible:ring-accent rounded"
                    >{{ savedByText(e) }}</RouterLink>
                    <span v-else>{{ savedByText(e) }}</span>
                  </td>
                  <td class="act">
                    <Button
                      variant="ghost"
                      size="xs"
                      :title="$t('import-matches-delete-hint')"
                      :loading="deleting === e.id"
                      :disabled="deleting !== null"
                      data-test="matches-delete"
                      @click="deleteOne(e)"
                    >
                      {{ $t('import-matches-delete') }}
                    </Button>
                  </td>
                </tr>
              </template>
            </tbody>
          </table>

          <p v-if="listed.shown === 0" class="px-3 py-2 text-sm text-secondary" data-test="matches-no-results">
            {{ $t('import-matches-no-results') }}
          </p>

          <div class="flex flex-wrap items-center gap-2 px-3 py-2 border-t border-default bg-surface-alt text-xs text-tertiary">
            <Button variant="ghost" size="sm" data-test="matches-delete-all" @click="confirmAll = true">
              {{ $t('import-matches-delete-all', { count: entries.length, nouns }) }}
            </Button>
            <span>{{ $t('import-matches-delete-all-hint', { noun }) }}</span>
          </div>
        </template>
      </SectionCard>

      <!-- Columns: the header memory, read only. -->
      <SectionCard content-padding="" data-test="matches-columns">
        <template #title>
          {{ columns ? $t('import-matches-columns-title', { count: columns.rows.length }) : $t('import-matches-columns-title-empty') }}
        </template>
        <p v-if="view && !columns" class="px-3 py-4 text-sm text-secondary">
          {{ $t('import-matches-columns-empty', { noun }) }}
        </p>
        <template v-else-if="columns">
          <p class="px-3 py-2 text-sm text-secondary" data-test="matches-columns-summary">{{ columnsSummary }}</p>
          <table v-if="showColumns" class="mx-tbl cols-3 w-full text-sm">
            <colgroup>
              <col class="c-src" />
              <col class="c-arr" />
              <col />
            </colgroup>
            <thead>
              <tr>
                <th scope="col" class="src">{{ $t('import-matches-col-file') }}</th>
                <th scope="col" class="arr"><span class="sr-only">{{ $t('import-matches-becomes') }}</span></th>
                <th scope="col">{{ $t('import-matches-col-workspace') }}</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="(r, i) in columns.rows" :key="i" class="mx-row">
                <td class="src" :title="r.header">{{ r.header }}</td>
                <td class="arr" aria-hidden="true">→</td>
                <td class="dst" :class="{ off: r.ignored, none: !r.field && !r.ignored }">{{ columnTarget(r) }}</td>
              </tr>
            </tbody>
          </table>
          <div class="px-3 py-2 border-t border-default text-sm">
            <button type="button" :class="linkClass" data-test="matches-columns-toggle" @click="showColumns = !showColumns">
              {{ showColumns ? $t('import-matches-hide-columns') : $t('import-matches-show-columns') }}
            </button>
          </div>
        </template>
      </SectionCard>
    </div>

    <ConfirmModal
      :show="confirmAll"
      :title="$t('import-matches-delete-all-title', { count: entries.length, nouns })"
      :message="$t('import-matches-delete-all-body', { noun })"
      :confirm-label="$t('import-matches-delete-all-confirm')"
      variant="danger"
      :loading="deletingAll"
      @confirm="deleteAll"
      @close="confirmAll = false"
    />
  </div>
</template>

<style scoped>
/* One reference table: sections as heading rows, the file side tinted. */
.mx-tbl {
  table-layout: fixed;
  border-collapse: collapse;
}
.mx-tbl col.c-src {
  width: 36%;
}
.mx-tbl.cols-3 col.c-src {
  width: 40%;
}
.mx-tbl col.c-arr {
  width: 2rem;
}
.mx-tbl col.c-who {
  width: 7.5rem;
}
.mx-tbl col.c-act {
  width: 4.75rem;
}
.mx-tbl thead th {
  background: var(--color-surface-alt);
  border-bottom: 1px solid var(--color-default);
  padding: 0.4375rem 0.75rem;
  text-align: left;
  font-size: 12px;
  font-weight: 500;
  color: var(--color-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.mx-tbl thead th.src {
  background: var(--color-surface-hover);
}
.mx-tbl td {
  border-top: 1px solid var(--color-default);
  padding: 0.4375rem 0.75rem;
  vertical-align: middle;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.mx-tbl td.src {
  font-family: var(--font-mono);
  font-size: 12.5px;
  color: var(--color-primary);
  background: var(--color-subtle);
}
.mx-tbl td.arr,
.mx-tbl th.arr {
  color: var(--color-tertiary);
  text-align: center;
  padding-left: 0;
  padding-right: 0;
}
.mx-tbl td.dst {
  font-weight: 500;
  color: var(--color-primary);
}
.mx-tbl td.dst.discard {
  font-weight: 400;
  color: var(--color-secondary);
}
.mx-tbl td.dst.off {
  color: var(--color-status-warning);
}
.mx-tbl td.dst.none {
  font-weight: 400;
  color: var(--color-tertiary);
}
.mx-tbl td.dst .note {
  margin-left: 0.375rem;
  font-size: 12px;
  font-weight: 400;
  color: var(--color-tertiary);
}
.mx-tbl td.who {
  font-size: 12px;
  color: var(--color-tertiary);
}
.mx-tbl td.act {
  text-align: right;
  padding-right: 0.5rem;
}
.mx-tbl td.act :deep(button) {
  opacity: 0.55;
}
.mx-tbl tr.mx-row:hover td.act :deep(button),
.mx-tbl tr.mx-row:focus-within td.act :deep(button) {
  opacity: 1;
}
@media (hover: none) {
  .mx-tbl td.act :deep(button) {
    opacity: 1;
  }
}
.mx-tbl tr.mx-row:hover td {
  background: var(--color-surface-alt);
}
.mx-tbl tr.mx-sec th {
  padding: 1rem 0.75rem 0.375rem;
  border-top: 1px solid var(--color-default);
  background: var(--color-surface);
  text-align: left;
  font-size: 13px;
  font-weight: 600;
  color: var(--color-primary);
}
.mx-tbl tbody:first-of-type tr.mx-sec th {
  border-top: 0;
}
.mx-tbl tr.mx-sec th .n {
  margin-left: 0.375rem;
  font-weight: 400;
  color: var(--color-tertiary);
}
.mx-tbl tr.mx-sub th {
  padding: 0.5rem 0.75rem 0.25rem 1.5rem;
  border-top: 1px solid var(--color-default);
  background: var(--color-surface);
  text-align: left;
  font-size: 12px;
  font-weight: 500;
  color: var(--color-secondary);
}
.mx-tbl tr.mx-sub th::before {
  content: '';
  display: inline-block;
  width: 0.5rem;
  height: 0.5rem;
  margin: 0 0.5rem 0 -1rem;
  border-left: 1px solid var(--color-strong);
  border-bottom: 1px solid var(--color-strong);
  vertical-align: 1px;
}
.mx-tbl tr.mx-row.in td.src {
  padding-left: 1.5rem;
}

/* Phone: each row is two lines, the match, then Saved by and Delete; the
   header stays one band. */
@media (max-width: 767px) {
  .mx-tbl colgroup {
    display: none;
  }
  .mx-tbl,
  .mx-tbl thead,
  .mx-tbl tbody,
  .mx-tbl tr,
  .mx-tbl th,
  .mx-tbl td {
    display: block;
  }
  .mx-tbl thead tr,
  .mx-tbl tr.mx-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 1.25rem minmax(0, 1fr);
    column-gap: 0.25rem;
    align-items: center;
  }
  .mx-tbl thead tr {
    align-items: stretch;
    column-gap: 0;
    background: var(--color-surface-alt);
  }
  .mx-tbl thead th,
  .mx-tbl thead th.src {
    background: var(--color-surface-alt);
  }
  .mx-tbl thead th.who,
  .mx-tbl thead th.act {
    display: none;
  }
  .mx-tbl tr.mx-row {
    padding: 0.5rem 0.75rem;
    border-top: 1px solid var(--color-default);
  }
  .mx-tbl tr.mx-row td {
    border-top: 0;
    padding: 0;
    background: none;
  }
  .mx-tbl tr.mx-row:hover td {
    background: none;
  }
  .mx-tbl tr.mx-row:hover {
    background: var(--color-surface-alt);
  }
  .mx-tbl td.src,
  .mx-tbl tr.mx-row.in td.src {
    padding-left: 0;
  }
  .mx-tbl td.who {
    grid-column: 1 / 3;
    padding-top: 0.125rem;
  }
  .mx-tbl td.act {
    grid-column: 3;
    grid-row: 2;
    text-align: right;
    padding-right: 0;
  }
  .mx-tbl td.act :deep(button) {
    opacity: 1;
  }
}
</style>
