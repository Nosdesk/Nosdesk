<script setup lang="ts">
/**
 * Done for a documentation import. The commit runs in the background, so
 * Done first follows it ("Importing 36 pages", with the pages written so
 * far; the admin can leave and come back), then says what it did, with the
 * collection to open, Publish all, the conversion report, and Remove
 * imported pages.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import Callout from '@/components/common/Callout.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import Spinner from '@/components/common/Spinner.vue'
import type { DocsImportJob, DocsSummary } from '@nosdesk/core/services/docsImportService'

import DocsPublishAction from './DocsPublishAction.vue'
import DocsRemoveBlock from './DocsRemoveBlock.vue'
import { noteText, summaryCounts } from './docsReviewModel'
import { destinationOf } from './docsRemoveModel'

const props = defineProps<{
  job: DocsImportJob
  summary: DocsSummary
  /** "IT Handbook", or null when the pages went into several collections. */
  collection: string | null
  downloading?: boolean
}>()

const emit = defineEmits<{
  open: []
  report: []
  another: []
  recent: []
  /** The job after Remove imported pages, or as it is now after a refusal. */
  updated: [job: DocsImportJob]
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const fmt = (n: number) => n.toLocaleString()

const counts = computed(() => summaryCounts(props.summary))
const importing = computed(() => props.job.status === 'committing')
const total = computed(() => counts.value.imported)
const written = computed(() => Math.min(props.job.records_committed ?? 0, total.value))
const pct = computed(() => (total.value > 0 ? Math.round((written.value / total.value) * 100) : 0))

const dest = computed(() => ({ ...destinationOf(props.summary), name: props.collection ?? destinationOf(props.summary).name }))
const removed = computed(() => !!props.job.removed_at || !!props.job.remove_result)
/** Publish all ran here: the drafts line no longer applies. */
const published = ref(false)

const where = computed(() =>
  props.collection ?? t('docs-import-done-many', { count: props.summary.destination.new_collections.length }),
)

const doneLine = computed(() => {
  const s = props.summary
  const parts = [
    t('docs-import-done-summary', {
      count: props.job.records_committed ?? counts.value.imported,
      where: where.value,
      status: s.status,
    }),
  ]
  if (s.images > 0) parts.push(t('docs-import-done-images', { count: s.images }))
  if (counts.value.skipped > 0) parts.push(t('docs-import-done-skipped', { count: counts.value.skipped }))
  if (counts.value.errors > 0) parts.push(t('docs-import-done-errors', { count: counts.value.errors }))
  return parts.join(' ')
})

const tiles = computed(() => [
  { id: 'new', label: t('docs-import-tile-new'), value: props.job.records_committed ?? counts.value.imported, sub: '' },
  {
    id: 'images',
    label: t('docs-import-tile-images'),
    value: props.summary.images,
    sub: props.summary.image_problems > 0 ? t('docs-import-tile-images-left-out', { count: props.summary.image_problems }) : '',
  },
  { id: 'links', label: t('docs-import-tile-links'), value: props.summary.links_rewritten, sub: '' },
  {
    id: 'skipped',
    label: t('docs-import-tile-skipped'),
    value: counts.value.skipped,
    sub: counts.value.skipped > 0 ? t('docs-import-tile-skipped-already', { count: counts.value.skipped }) : '',
  },
])

const noteLines = computed(() =>
  props.summary.notes.map((n) => ({
    key: n.code,
    text: noteText(t, n.code, n.count),
    pages: n.pages,
  })),
)
</script>

<template>
  <section v-if="importing" class="flex flex-col gap-6" data-test="docs-done-importing">
    <SectionCard content-padding="p-4">
      <template #title>{{ $t('docs-import-importing-title', { count: total }) }}</template>
      <div class="flex flex-col gap-3" aria-live="polite">
        <div class="flex items-center gap-3">
          <Spinner size="sm" />
          <span class="text-sm text-primary" data-test="docs-done-progress">
            {{
              written < total
                ? $t('docs-import-importing-progress', { written, total })
                : $t('docs-import-importing-finishing')
            }}
          </span>
        </div>
        <div
          class="w-full max-w-xs h-1.5 rounded-full bg-surface-hover overflow-hidden"
          role="progressbar"
          :aria-valuenow="written"
          aria-valuemin="0"
          :aria-valuemax="total"
          :aria-label="$t('docs-import-importing-title', { count: total })"
        >
          <span class="block h-full bg-accent rounded-full transition-[width] duration-500" :style="{ width: `${pct}%` }" />
        </div>
        <p class="text-xs text-tertiary">{{ $t('docs-import-importing-leave') }}</p>
      </div>
    </SectionCard>
    <div class="sticky bottom-0 z-10 bg-app border-t border-default py-3 flex flex-wrap items-center gap-3">
      <Button variant="ghost" data-test="docs-done-recent-bar" @click="emit('recent')">{{ $t('import-history-title') }}</Button>
      <Button class="ml-auto" loading>{{ $t('docs-import-review-importing') }}</Button>
    </div>
  </section>

  <section v-else class="flex flex-col gap-6" data-test="docs-done">
    <Callout severity="success">
      <template #header>
        <span class="font-medium text-primary">{{ $t('docs-import-done-heading') }}</span>
      </template>
      <div class="px-4 py-3 flex flex-col gap-1.5 text-sm">
        <p class="text-primary" data-test="docs-done-headline">{{ doneLine }}</p>
        <p v-if="summary.status === 'drafts' && !removed && !published" class="text-secondary">{{ $t('docs-import-done-drafts') }}</p>
        <p class="text-xs text-tertiary">{{ $t('docs-import-done-revision', { file: job.filename }) }}</p>
      </div>
    </Callout>

    <div class="grid grid-cols-2 sm:grid-cols-4 gap-3" data-test="docs-done-tiles">
      <div
        v-for="tile in tiles"
        :key="tile.id"
        class="bg-surface border border-default rounded-lg px-4 py-3.5 flex flex-col gap-1 min-w-0"
      >
        <span class="text-xs uppercase tracking-wide text-tertiary">{{ tile.label }}</span>
        <span class="text-xl sm:text-2xl font-semibold text-primary leading-tight">{{ fmt(tile.value) }}</span>
        <span v-if="tile.sub" class="text-xs text-tertiary">{{ tile.sub }}</span>
      </div>
    </div>

    <div class="flex flex-wrap items-center gap-2" data-test="docs-done-actions">
      <Button icon="openExternal" data-test="docs-done-open" @click="emit('open')">
        {{ collection ? $t('docs-import-done-open', { name: collection }) : $t('docs-import-done-open-docs') }}
      </Button>
      <DocsPublishAction :job="job" :summary="summary" :collection="collection" @published="published = true" />
      <Button
        v-if="!job.pruned_at"
        variant="secondary"
        icon="download"
        :loading="downloading"
        data-test="docs-done-report"
        @click="emit('report')"
      >
        {{ $t('docs-import-report') }}
      </Button>
      <Button variant="secondary" data-test="docs-done-another" @click="emit('another')">
        {{ $t('docs-import-done-another') }}
      </Button>
    </div>

    <SectionCard v-if="noteLines.length > 0" content-padding="">
      <template #title>{{ $t('docs-import-conversion-done-title') }}</template>
      <ul class="flex flex-col divide-y divide-default">
        <li v-for="line in noteLines" :key="line.key" class="px-3 py-2 text-sm text-primary">
          {{ line.text }}
          <span class="text-tertiary">· {{ $t('docs-import-line-pages', { count: line.pages }) }}</span>
        </li>
      </ul>
      <p class="px-3 py-2 border-t border-default bg-surface-alt text-xs text-tertiary">
        {{ $t('docs-import-conversion-done-footer') }}
      </p>
    </SectionCard>

    <DocsRemoveBlock :job="job" :dest="dest" @updated="emit('updated', $event)" />
  </section>
</template>
