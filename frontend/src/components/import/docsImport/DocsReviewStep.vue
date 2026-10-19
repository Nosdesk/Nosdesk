<script setup lang="ts">
/**
 * Review for a documentation import: the plan the check made, which is
 * what Import will do. The headline and tiles count it; pages already in
 * Nosdesk get their own choice (Skip them or Import as copies, a commit
 * option, so flipping it recomputes everything here without a new check);
 * each change the conversion made is a line with a count and Show pages;
 * then every page, filtered and 25 at a time, with a read-only preview of
 * the selected one beside the list (or in a sheet on smaller screens).
 */
import { computed, nextTick, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import Callout from '@/components/common/Callout.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import Icon from '@/components/common/Icon.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import SegmentedControl from '@/components/common/SegmentedControl.vue'
import Spinner from '@/components/common/Spinner.vue'
import StatusPill from '@/components/common/StatusPill.vue'
import type { StatusPillTone } from '@/components/common/statusPillTone'
import Modal from '@/components/Modal.vue'
import { useMobileDetection } from '@/composables/useMobileDetection'
import type {
  DocsDuplicates,
  DocsImportJob,
  DocsNoteCode,
  DocsPageRow,
  DocsSummary,
} from '@nosdesk/core/services/docsImportService'

import DocsPagePreview from './DocsPagePreview.vue'
import {
  confirmText,
  crumbs,
  filterCounts,
  filterRows,
  headline,
  imageProblems,
  isSection,
  noteText,
  outcomeOf,
  PAGE_SIZE,
  paginate,
  planCounts,
  rowNotes,
  type ListChip,
  type ListFilter,
  type RowOutcome,
} from './docsReviewModel'

const props = defineProps<{
  job: DocsImportJob
  summary: DocsSummary
  rows: DocsPageRow[] | null
  rowsFailed?: boolean
  /** Rows whose plan moved since the check (a refused commit). */
  changed: ReadonlySet<number>
  dest: string
  committing?: boolean
  downloading?: boolean
}>()

const duplicates = defineModel<DocsDuplicates>('duplicates', { required: true })

const emit = defineEmits<{
  back: []
  commit: []
  report: []
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const fmt = (n: number) => n.toLocaleString()

const { isMobile: narrow } = useMobileDetection('lg')

const allRows = computed(() => props.rows ?? [])
const byIndex = computed(() => new Map(allRows.value.map((r) => [r.index, r])))
const counts = computed(() => planCounts(allRows.value, duplicates.value))
const imported = computed(() => allRows.value.filter((r) => outcomeOf(r, duplicates.value) !== 'skipped'))

const imagesStored = computed(() => {
  const keepLinks = props.summary.web_images_choice === 'links'
  return imported.value.reduce(
    (n, r) => n + (r.images ?? []).filter((i) => !i.problem && !(keepLinks && i.kind === 'web')).length,
    0,
  )
})
const imageTrouble = computed(() => imported.value.reduce((n, r) => n + imageProblems(r).length, 0))
const webImages = computed(() =>
  imported.value.reduce((n, r) => n + (r.images ?? []).filter((i) => i.kind === 'web' && !i.problem).length, 0),
)
const linksRewritten = computed(() => imported.value.reduce((n, r) => n + (r.links.rewritten ?? 0), 0))
const keptAsText = computed(() =>
  imported.value.reduce((n, r) => n + (r.links.unresolved?.length ?? 0) + (r.links.attachments?.length ?? 0), 0),
)
const sections = computed(() => imported.value.filter(isSection))

const headlineText = computed(() => headline(t, counts.value, props.dest, props.summary.status))

// ---- list state ----

const filter = ref<ListFilter>('all')
const chip = ref<ListChip | null>(null)
const page = ref(1)
const selected = ref<number | null>(null)
const sheetOpen = ref(false)
const listRef = ref<HTMLElement | null>(null)

const filtered = computed(() => filterRows(allRows.value, filter.value, chip.value, duplicates.value, props.changed))
const paged = computed(() => paginate(filtered.value, page.value))
const fcounts = computed(() => filterCounts(allRows.value, duplicates.value))

watch(
  [filter, chip, duplicates],
  () => {
    page.value = 1
  },
  { flush: 'sync' },
)

const filterOptions = computed(() => [
  { value: 'all' as ListFilter, label: t('docs-import-filter-all', { count: fcounts.value.all }) },
  { value: 'notes' as ListFilter, label: t('docs-import-filter-notes', { count: fcounts.value.notes }) },
  { value: 'skipped' as ListFilter, label: t('docs-import-filter-skipped', { count: fcounts.value.skipped }) },
  { value: 'errors' as ListFilter, label: t('docs-import-filter-errors', { count: fcounts.value.errors }) },
])

const chipLabel = computed(() => {
  const c = chip.value
  if (!c) return ''
  switch (c) {
    case 'sections':
      return t('docs-import-chip-sections')
    case 'images':
      return t('docs-import-chip-images')
    case 'links':
      return t('docs-import-chip-links')
    case 'changed':
      return t('docs-import-chip-changed')
    case 'errors':
      return t('docs-import-chip-errors')
    default:
      return noteText(t, c, noteTotals.value.get(c)?.count ?? 0)
  }
})

async function showChip(c: ListChip) {
  chip.value = c
  filter.value = 'all'
  page.value = 1
  await nextTick()
  listRef.value?.scrollIntoView({ block: 'start', behavior: 'smooth' })
}

function showFilter(f: ListFilter) {
  filter.value = f
  chip.value = null
}

// A refused commit marks what changed: show it.
watch(
  () => props.changed.size,
  (n) => {
    if (n > 0) void showChip('changed')
  },
)

// ---- tiles ----

const tiles = computed(() => {
  const c = counts.value
  const newSub =
    c.copies > 0
      ? t('docs-import-tile-new-copies', { count: c.copies })
      : c.errors > 0
        ? t('docs-import-tile-new-errors', { count: c.errors })
        : sections.value.length > 0
          ? t('docs-import-tile-new-sections', { count: sections.value.length })
          : ''
  return [
    {
      id: 'new',
      label: t('docs-import-tile-new'),
      value: c.imported,
      sub: newSub,
      on: filter.value === 'all' && !chip.value,
      go: () => showFilter('all'),
    },
    {
      id: 'images',
      label: t('docs-import-tile-images'),
      value: imagesStored.value,
      sub:
        imageTrouble.value > 0
          ? t('docs-import-tile-images-problems', { count: imageTrouble.value })
          : webImages.value > 0
            ? t('docs-import-tile-images-web', { count: webImages.value })
            : '',
      on: chip.value === 'images',
      go: () => void showChip('images'),
    },
    {
      id: 'links',
      label: t('docs-import-tile-links'),
      value: linksRewritten.value,
      sub: keptAsText.value > 0 ? t('docs-import-tile-links-kept', { count: keptAsText.value }) : t('docs-import-tile-links-all'),
      on: chip.value === 'links',
      go: () => void showChip('links'),
    },
    {
      id: 'skipped',
      label: t('docs-import-tile-skipped'),
      value: c.skipped,
      sub: c.skipped > 0 ? t('docs-import-tile-skipped-already', { count: c.skipped }) : '',
      on: filter.value === 'skipped',
      go: () => showFilter('skipped'),
    },
  ]
})

// ---- already in Nosdesk ----

const dupRows = computed(() => allRows.value.filter((r) => r.duplicate))
const dupNames = computed(() => {
  const names = dupRows.value.slice(0, 4).map((r) => r.title)
  const more = dupRows.value.length - names.length
  return more > 0 ? t('docs-import-already-names-more', { names: names.join(', '), count: more }) : names.join(', ')
})
const dupBy = computed(() => {
  const by = new Set(dupRows.value.map((r) => r.duplicate!.by))
  if (by.size === 1 && by.has('source')) return t('docs-import-already-by-source')
  if (by.size === 1 && by.has('uuid')) return t('docs-import-already-by-uuid')
  return t('docs-import-already-by-both')
})
const dupOptions = computed(() => [
  { value: 'skip' as DocsDuplicates, label: t('docs-import-already-skip') },
  { value: 'copy' as DocsDuplicates, label: t('docs-import-already-copy') },
])

// ---- conversion notes ----

/** Each note code over the pages the commit writes. */
const noteTotals = computed(() => {
  const out = new Map<DocsNoteCode, { count: number; pages: number }>()
  for (const row of imported.value) {
    for (const [code, n] of Object.entries(row.notes ?? {}) as [DocsNoteCode, number][]) {
      const line = out.get(code) ?? { count: 0, pages: 0 }
      line.count += n
      line.pages += 1
      out.set(code, line)
    }
  }
  return out
})

interface ConversionLine {
  key: string
  text: string
  pages?: number
  chip?: ListChip
}

const conversionLines = computed<ConversionLine[]>(() => {
  const lines: ConversionLine[] = [...noteTotals.value.entries()].map(([code, v]) => ({
    key: code,
    text: noteText(t, code, v.count),
    pages: v.pages,
    chip: code,
  }))
  const errors = counts.value.errors
  if (errors > 0) lines.unshift({ key: 'errors', text: t('docs-import-line-errors', { count: errors }), chip: 'errors' })
  if (sections.value.length > 0) {
    lines.push({
      key: 'sections',
      text: t('docs-import-line-sections', {
        count: sections.value.length,
        names: sections.value
          .slice(0, 5)
          .map((r) => r.title)
          .join(', '),
      }),
      chip: 'sections',
    })
  }
  const fm = Object.entries(props.summary.front_matter_not_imported ?? {})
  if (fm.length > 0) {
    lines.push({
      key: 'front-matter',
      text: t('docs-import-line-front-matter', {
        fields: fm
          .slice(0, 6)
          .map(([k, n]) => `${k} (${fmt(n)})`)
          .join(', '),
      }),
    })
  }
  return lines
})

// ---- rows and the preview ----

function notesOf(row: DocsPageRow) {
  return rowNotes(t, row, duplicates.value, props.changed)
}

const OUTCOME: Record<RowOutcome, { key: string; tone: StatusPillTone }> = {
  new: { key: 'docs-import-outcome-new', tone: 'positive' },
  copy: { key: 'docs-import-outcome-copy', tone: 'info' },
  skipped: { key: 'docs-import-outcome-skipped', tone: 'neutral' },
  error: { key: 'docs-import-outcome-error', tone: 'critical' },
}

const selectedRow = computed(() => (selected.value != null ? (byIndex.value.get(selected.value) ?? null) : null))
const selectedIx = computed(() => filtered.value.findIndex((r) => r.index === selected.value))

/** Focus the row as listed on screen: the table row, or the card on a
 *  phone. */
async function focusRow(index: number) {
  await nextTick()
  const els = listRef.value?.querySelectorAll<HTMLElement>(`[data-row="${index}"]`) ?? []
  for (const el of els) {
    if (el.offsetParent !== null) {
      el.focus()
      return
    }
  }
}

function select(index: number, open = false) {
  selected.value = index
  // Keep the selection on the page it is listed on.
  const at = filtered.value.findIndex((r) => r.index === index)
  if (at >= 0) page.value = Math.floor(at / PAGE_SIZE) + 1
  if (open && narrow.value) sheetOpen.value = true
  void focusRow(index)
}

function move(delta: number) {
  const next = filtered.value[selectedIx.value + delta]
  if (next) select(next.index)
}

function onRowKey(event: KeyboardEvent, row: DocsPageRow) {
  if (event.key === 'ArrowDown') {
    event.preventDefault()
    const next = filtered.value[filtered.value.findIndex((r) => r.index === row.index) + 1]
    if (next) select(next.index)
  } else if (event.key === 'ArrowUp') {
    event.preventDefault()
    const prev = filtered.value[filtered.value.findIndex((r) => r.index === row.index) - 1]
    if (prev) select(prev.index)
  } else if (event.key === 'Enter' || event.key === ' ') {
    event.preventDefault()
    select(row.index, true)
  } else if (event.key === 'Escape') {
    event.preventDefault()
    selected.value = null
  }
}

/** A link in the preview to another page in the import selects it. */
function openLinked(index: number) {
  if (!byIndex.value.has(index)) return
  if (!filtered.value.some((r) => r.index === index)) {
    filter.value = 'all'
    chip.value = null
  }
  select(index)
}

const rovingIndex = computed(() =>
  paged.value.rows.some((r) => r.index === selected.value) ? selected.value : (paged.value.rows[0]?.index ?? null),
)

// ---- commit ----

const showConfirm = ref(false)
const confirmMessage = computed(() =>
  confirmText(t, counts.value, imagesStored.value, props.dest, props.summary.status),
)
const skipReason = computed(() =>
  counts.value.skipped > 0 && counts.value.imported > 0
    ? t('docs-import-review-skip-reason', { count: counts.value.skipped })
    : '',
)

function confirm() {
  showConfirm.value = false
  emit('commit')
}
</script>

<template>
  <section class="flex flex-col gap-6" data-test="docs-review">
    <p class="text-base text-primary" aria-live="polite" data-test="docs-review-headline">{{ headlineText }}</p>

    <div class="grid grid-cols-2 sm:grid-cols-4 gap-3" data-test="docs-review-tiles">
      <button
        v-for="tile in tiles"
        :key="tile.id"
        type="button"
        class="bg-surface border rounded-lg px-4 py-3.5 flex flex-col gap-1 text-left min-w-0 transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        :class="tile.on ? 'border-accent ring-1 ring-accent' : 'border-default hover:border-strong'"
        :aria-pressed="tile.on"
        :data-test="`docs-tile-${tile.id}`"
        @click="tile.go()"
      >
        <span class="text-xs uppercase tracking-wide text-tertiary">{{ tile.label }}</span>
        <span class="text-xl sm:text-2xl font-semibold text-primary leading-tight">{{ fmt(tile.value) }}</span>
        <span v-if="tile.sub" class="text-xs text-tertiary break-words" data-test="docs-tile-sub">{{ tile.sub }}</span>
      </button>
    </div>

    <Callout v-if="dupRows.length > 0" severity="info" data-test="docs-review-already">
      <template #header>
        <span class="font-medium text-primary">{{ $t('docs-import-already-title', { count: dupRows.length }) }}</span>
        <span class="block text-xs text-secondary mt-0.5">{{ dupNames }}: {{ dupBy }}</span>
      </template>
      <div class="px-4 py-3 flex flex-wrap items-start gap-x-4 gap-y-2">
        <span class="flex-[1_1_220px] min-w-0 text-xs text-tertiary">
          {{ duplicates === 'skip' ? $t('docs-import-already-skip-help') : $t('docs-import-already-copy-help') }}
        </span>
        <SegmentedControl v-model="duplicates" :options="dupOptions" :aria-label="$t('docs-import-already-label')" />
      </div>
    </Callout>

    <SectionCard content-padding="" data-test="docs-review-conversion">
      <template #title>{{ $t('docs-import-conversion-title') }}</template>
      <ul v-if="conversionLines.length > 0" class="flex flex-col divide-y divide-default">
        <li
          v-for="line in conversionLines"
          :key="line.key"
          class="flex flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2 text-sm min-w-0"
          data-test="docs-conversion-line"
        >
          <span class="min-w-0 flex-1 text-primary">
            {{ line.text }}
            <span v-if="line.pages" class="text-tertiary">· {{ $t('docs-import-line-pages', { count: line.pages }) }}</span>
          </span>
          <Button
            v-if="line.chip"
            variant="ghost"
            size="sm"
            :data-test="`docs-show-${line.key}`"
            @click="showChip(line.chip)"
          >
            {{ $t('docs-import-show-pages') }}
          </Button>
        </li>
      </ul>
      <p v-else class="px-3 py-3 text-sm text-secondary">{{ $t('docs-import-conversion-clean') }}</p>
      <p class="px-3 py-2 border-t border-default bg-surface-alt text-xs text-tertiary">
        {{ $t('docs-import-conversion-footer') }}
      </p>
    </SectionCard>

    <div
      class="flex flex-col gap-6 min-w-0"
      :class="narrow ? '' : 'docs-review-grid--side'"
    >
      <div ref="listRef" class="scroll-mt-4 min-w-0">
        <SectionCard content-padding="" data-test="docs-review-list">
          <template #title>{{ $t('docs-import-list-title') }}</template>
          <div class="flex flex-wrap items-center gap-2 px-3 py-2 border-b border-default">
            <div class="max-w-full overflow-x-auto scrollbar-hide">
              <SegmentedControl
                :model-value="filter"
                :options="filterOptions"
                size="sm"
                :aria-label="$t('docs-import-filter-label')"
                @update:model-value="showFilter"
              />
            </div>
            <span
              v-if="chip"
              class="inline-flex items-center gap-1.5 h-[26px] pl-2.5 pr-1.5 rounded-full bg-accent/15 border border-accent text-xs text-primary max-w-full"
              data-test="docs-list-chip"
            >
              <span class="truncate">{{ chipLabel }}</span>
              <button
                type="button"
                class="w-[18px] h-[18px] rounded-full flex items-center justify-center text-accent hover:bg-surface-hover"
                :aria-label="$t('docs-import-chip-clear')"
                data-test="docs-list-chip-clear"
                @click="chip = null"
              >
                <Icon name="close" size="xs" />
              </button>
            </span>
            <span class="hidden lg:inline ml-auto text-2xs text-tertiary">{{ $t('docs-import-list-keys') }}</span>
          </div>

          <p v-if="rowsFailed" class="px-3 py-5 text-sm text-secondary">{{ $t('docs-import-list-failed') }}</p>
          <p v-else-if="!rows" class="flex items-center gap-2 px-3 py-5 text-sm text-secondary">
            <Spinner size="sm" />
            {{ $t('docs-import-list-loading') }}
          </p>
          <p v-else-if="filtered.length === 0" class="px-3 py-5 text-sm text-secondary" data-test="docs-list-empty">
            {{
              filter === 'notes'
                ? $t('docs-import-list-empty-notes')
                : filter === 'skipped'
                  ? $t('docs-import-list-empty-skipped')
                  : filter === 'errors'
                    ? $t('docs-import-list-empty-errors')
                    : $t('docs-import-list-empty')
            }}
          </p>
          <template v-else>
            <table class="hidden sm:table w-full table-fixed border-collapse text-sm" :aria-label="$t('docs-import-list-title')">
              <thead class="bg-surface-alt">
                <tr>
                  <th scope="col" class="text-left text-xs uppercase tracking-wide text-tertiary font-medium px-3 py-2" :class="narrow ? 'w-[60%]' : ''">
                    {{ $t('docs-import-col-page') }}
                  </th>
                  <th scope="col" class="text-left text-xs uppercase tracking-wide text-tertiary font-medium px-3 py-2 w-[110px]">
                    {{ $t('docs-import-col-outcome') }}
                  </th>
                  <th v-if="narrow" scope="col" class="text-left text-xs uppercase tracking-wide text-tertiary font-medium px-3 py-2">
                    {{ $t('docs-import-col-notes') }}
                  </th>
                </tr>
              </thead>
              <tbody>
                <tr
                  v-for="row in paged.rows"
                  :key="row.index"
                  :data-row="row.index"
                  :tabindex="rovingIndex === row.index ? 0 : -1"
                  :aria-selected="selected === row.index"
                  class="cursor-pointer border-t border-default align-top outline-none focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
                  :class="selected === row.index ? 'bg-accent/15' : 'hover:bg-surface-alt'"
                  data-test="docs-list-row"
                  @click="select(row.index, true)"
                  @keydown="onRowKey($event, row)"
                >
                  <td class="px-3 py-2 min-w-0">
                    <div class="font-medium text-primary break-words">{{ row.title }}</div>
                    <div class="text-xs text-tertiary truncate" :title="row.path">
                      {{ crumbs(row, byIndex) || $t('docs-import-top-level') }}
                    </div>
                  </td>
                  <td class="px-3 py-2">
                    <StatusPill
                      :label="$t(OUTCOME[outcomeOf(row, duplicates)].key)"
                      :tone="OUTCOME[outcomeOf(row, duplicates)].tone"
                      data-test="docs-list-outcome"
                    />
                  </td>
                  <td v-if="narrow" class="px-3 py-2 min-w-0">
                    <div class="flex flex-col gap-0.5 text-xs-plus text-secondary">
                      <span
                        v-for="(n, i) in notesOf(row)"
                        :key="i"
                        :class="n.tone === 'error' ? 'text-status-error' : n.tone === 'warning' ? 'text-status-warning' : ''"
                      >{{ n.text }}</span>
                      <span v-if="notesOf(row).length === 0" class="text-tertiary">{{ $t('docs-import-no-notes') }}</span>
                    </div>
                  </td>
                </tr>
              </tbody>
            </table>

            <ul class="sm:hidden flex flex-col gap-2 p-3">
              <li
                v-for="row in paged.rows"
                :key="row.index"
                :data-row="row.index"
                tabindex="0"
                class="border rounded-lg p-3 flex flex-col gap-1 text-sm bg-surface cursor-pointer outline-none focus-visible:ring-2 focus-visible:ring-accent"
                :class="selected === row.index ? 'border-accent' : 'border-default'"
                data-test="docs-list-card"
                @click="select(row.index, true)"
                @keydown="onRowKey($event, row)"
              >
                <div class="flex items-start gap-2">
                  <span class="font-medium text-primary break-words min-w-0 flex-1">{{ row.title }}</span>
                  <StatusPill
                    :label="$t(OUTCOME[outcomeOf(row, duplicates)].key)"
                    :tone="OUTCOME[outcomeOf(row, duplicates)].tone"
                  />
                </div>
                <div class="text-xs text-tertiary truncate">{{ crumbs(row, byIndex) || $t('docs-import-top-level') }}</div>
                <div v-if="notesOf(row).length > 0" class="flex flex-col gap-0.5 text-xs-plus text-secondary">
                  <span
                    v-for="(n, i) in notesOf(row)"
                    :key="i"
                    :class="n.tone === 'error' ? 'text-status-error' : n.tone === 'warning' ? 'text-status-warning' : ''"
                  >{{ n.text }}</span>
                </div>
              </li>
            </ul>

            <div
              v-if="paged.pages > 1"
              class="flex items-center gap-2 px-3 py-2 border-t border-default text-xs text-tertiary"
              data-test="docs-list-pager"
            >
              <span>{{ $t('docs-import-pager', { from: paged.from, to: paged.to, total: paged.total }) }}</span>
              <span class="ml-auto flex items-center gap-1">
                <Button variant="ghost" size="xs" :disabled="paged.page === 1" data-test="docs-pager-prev" @click="page = paged.page - 1">
                  {{ $t('docs-import-pager-prev') }}
                </Button>
                <Button variant="ghost" size="xs" :disabled="paged.page === paged.pages" data-test="docs-pager-next" @click="page = paged.page + 1">
                  {{ $t('docs-import-pager-next') }}
                </Button>
              </span>
            </div>
          </template>
        </SectionCard>
      </div>

      <div v-if="!narrow" class="docs-review-side min-w-0">
        <DocsPagePreview
          :job-id="job.id"
          :updated-at="job.updated_at"
          :row="selectedRow"
          :outcome="selectedRow ? outcomeOf(selectedRow, duplicates) : null"
          :notes="selectedRow ? notesOf(selectedRow) : []"
          :others="counts.imported - (selectedRow && outcomeOf(selectedRow, duplicates) !== 'skipped' ? 1 : 0)"
          @open="openLinked"
        />
      </div>
    </div>

    <Modal
      v-if="narrow"
      :show="sheetOpen && !!selectedRow"
      :title="selectedRow?.title ?? ''"
      size="lg"
      remove-padding
      @close="sheetOpen = false"
    >
      <DocsPagePreview
        :job-id="job.id"
        :updated-at="job.updated_at"
        :row="selectedRow"
        :outcome="selectedRow ? outcomeOf(selectedRow, duplicates) : null"
        :notes="selectedRow ? notesOf(selectedRow) : []"
        :others="counts.imported - 1"
        headless
        @open="openLinked"
      />
      <template #footer>
        <div class="modal-actions">
          <Button variant="ghost" icon="chevronLeft" :disabled="selectedIx <= 0" data-test="docs-sheet-prev" @click="move(-1)">
            {{ $t('docs-import-preview-prev') }}
          </Button>
          <Button variant="secondary" :disabled="selectedIx < 0 || selectedIx >= filtered.length - 1" data-test="docs-sheet-next" @click="move(1)">
            {{ $t('docs-import-preview-next') }}
          </Button>
        </div>
      </template>
    </Modal>

    <div class="sticky bottom-0 z-10 bg-app border-t border-default py-3 flex flex-wrap items-center gap-3" data-test="docs-review-bar">
      <Button variant="ghost" class="order-2 sm:order-none w-full sm:w-auto" :disabled="committing" data-test="docs-review-back" @click="emit('back')">
        {{ $t('docs-import-review-back') }}
      </Button>
      <Button
        variant="secondary"
        icon="download"
        class="order-1 sm:order-none w-full sm:w-auto"
        :loading="downloading"
        data-test="docs-review-report"
        @click="emit('report')"
      >
        {{ $t('docs-import-report') }}
      </Button>
      <span v-if="skipReason" class="hidden sm:inline ml-auto text-xs text-tertiary">{{ skipReason }}</span>
      <Button
        class="order-first sm:order-none w-full sm:w-auto"
        :class="skipReason ? '' : 'sm:ml-auto'"
        :disabled="counts.imported === 0 || committing || !rows"
        :loading="committing"
        data-test="docs-review-import"
        @click="showConfirm = true"
      >
        {{
          committing
            ? $t('docs-import-review-importing')
            : counts.imported === 0
              ? $t('docs-import-review-nothing')
              : $t('docs-import-review-import', { count: counts.imported })
        }}
      </Button>
    </div>

    <ConfirmModal
      :show="showConfirm"
      :title="$t('docs-import-confirm-title', { count: counts.imported })"
      :message="confirmMessage"
      :confirm-label="$t('docs-import-review-import', { count: counts.imported })"
      variant="info"
      @close="showConfirm = false"
      @confirm="confirm"
    />
  </section>
</template>

<style scoped>
/* Wide screens: the preview follows the selected row beside the list. */
@media (min-width: 1024px) {
  .docs-review-grid--side {
    display: grid;
    grid-template-columns: minmax(300px, 2fr) minmax(0, 3fr);
    align-items: start;
  }
  .docs-review-side {
    position: sticky;
    top: 1rem;
    max-height: calc(100vh - 2rem);
    overflow-y: auto;
  }
}
</style>
