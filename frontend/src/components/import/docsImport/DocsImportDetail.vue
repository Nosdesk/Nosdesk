<script setup lang="ts">
/**
 * A past documentation import: what it read, the options it ran with, its
 * counts, and the tree with each page's outcome (each written page's title
 * opens it), then Open, Publish all, the conversion report and Remove
 * imported pages. Thirty days after the import the per-page detail is
 * removed: the page then shows the counts and options, and says so.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import { useQuery } from '@pinia/colada'
import { useRouter } from 'vue-router'

import Button from '@/components/common/Button.vue'
import ErrorBanner from '@/components/common/ErrorBanner.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import { formatCompactDate } from '@nosdesk/core/utils/dateUtils'
import { formatFileSize } from '@nosdesk/core/utils/formatFileSize'
import {
  docsImportService,
  docsPagesQueryKey,
  docsSummary,
  type DocsImportJob,
} from '@nosdesk/core/services/docsImportService'
import { useSyncDocsStore } from '@nosdesk/core/sync/stores/documentation'

import { errorBody, refusalText } from '../refusals'
import DocsPageTree, { type TreeOutcome } from './DocsPageTree.vue'
import DocsPublishAction from './DocsPublishAction.vue'
import DocsRemoveBlock from './DocsRemoveBlock.vue'
import { destinationOf, removedState } from './docsRemoveModel'
import {
  collectionAfterImport,
  outcomeOf,
  rowNotes,
  summaryCounts,
} from './docsReviewModel'
import { buildTree, collectionGroups } from './docsTreeModel'

const props = defineProps<{ job: DocsImportJob }>()

const emit = defineEmits<{
  /** The job after Remove imported pages, or as it is now after a refusal. */
  updated: [job: DocsImportJob]
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const fmt = (n: number) => n.toLocaleString()
const router = useRouter()
const docs = useSyncDocsStore()

const summary = computed(() => docsSummary(props.job))
const kept = computed(() => !props.job.pruned_at)
const counts = computed(() => (summary.value ? summaryCounts(summary.value) : null))
const choice = computed(() => summary.value?.duplicates_choice ?? 'skip')

const rowsQuery = useQuery({
  key: () => docsPagesQueryKey(props.job.id, props.job.updated_at),
  query: () => docsImportService.allPages(props.job.id),
  enabled: () => kept.value && props.job.status === 'done',
})

const view = computed(() => props.job.source?.tree ?? null)
const tree = computed(() => buildTree(view.value?.pages ?? []))
const expanded = ref<Set<string>>(
  new Set(tree.value.roots.filter((n) => n.children.length > 0).map((n) => n.page.path)),
)

const groups = computed(() => {
  const s = summary.value
  if (!view.value || !s) return []
  const d = s.destination
  const existing = d.mode === 'existing' ? { name: d.collection_name ?? '', under: null } : null
  const mode = d.mode === 'existing' ? 'existing' : d.new_collections.length === 1 ? 'new' : d.mode
  return collectionGroups(tree.value, { ...view.value, collections: d.new_collections }, {
    mode,
    collectionName: d.new_collections[0] ?? '',
  }, existing)
})

/** The page each written page became, by path. */
const links = computed(() => new Map((props.job.pages_written ?? []).map((p) => [p.path, p.id])))

/** Each page's outcome by path; a page the import left out has no row.
 *  Once Remove imported pages ran, a page it wrote went to Trash or was
 *  kept. */
const outcomes = computed(() => {
  const out = new Map<string, TreeOutcome>()
  const rows = rowsQuery.data.value
  if (!rows) return out
  const byPath = new Map(rows.map((r) => [r.path, r]))
  const removed = props.job.remove_result
  const kept = new Set((removed?.kept ?? []).map((k) => k.id))
  const trashed = new Set(removed?.deleted_ids ?? [])
  for (const node of tree.value.all) {
    const row = byPath.get(node.page.path)
    let outcome: TreeOutcome = row ? outcomeOf(row, choice.value) : 'left'
    const id = links.value.get(node.page.path)
    if (id != null && kept.has(id)) outcome = 'kept'
    else if (id != null && trashed.has(id)) outcome = 'trash'
    out.set(node.page.path, outcome)
  }
  return out
})

const noteCounts = computed(() => {
  const out = new Map<string, number>()
  for (const row of rowsQuery.data.value ?? []) {
    out.set(row.path, rowNotes(t, row, choice.value).filter((n) => !n.tone).length)
  }
  return out
})

const subtitle = computed(() =>
  [
    t('admin-data-import-docs-title'),
    formatCompactDate(props.job.completed_at ?? props.job.created_at),
    props.job.created_by_name,
  ]
    .filter(Boolean)
    .join(' · '),
)

const stateLabel = computed(() => {
  const s = summary.value
  if (!s) return ''
  if (props.job.remove_result) return removedState(t, props.job, formatCompactDate)
  return t('docs-import-detail-state', {
    count: props.job.records_committed ?? counts.value?.imported ?? 0,
    status: s.status,
  })
})

const sourceLabel = computed(() => {
  const s = props.job.source
  const profile = summary.value?.profile ?? s?.tree.profile
  const bits: string[] = []
  if (profile) bits.push(t(`docs-import-profile-option-${profile.replace(/_/g, '-')}`))
  if (s) {
    bits.push(t('docs-import-detail-files', { count: s.files }))
    bits.push(formatFileSize(s.size))
  }
  return bits.join(' · ')
})

const optionsLabel = computed(() => {
  const s = summary.value
  if (!s) return ''
  const d = s.destination
  const bits: string[] = []
  if (d.mode === 'existing') bits.push(t('docs-import-detail-into-existing', { name: d.collection_name ?? '' }))
  else if (d.new_collections.length === 1) bits.push(t('docs-import-detail-into-new', { name: d.new_collections[0] }))
  else bits.push(t('docs-import-detail-into-many', { count: d.new_collections.length }))
  bits.push(t(`docs-import-status-${s.status.replace(/_/g, '-')}`))
  bits.push(s.web_images_choice === 'download' ? t('docs-import-detail-web-download') : t('docs-import-detail-web-links'))
  bits.push(s.duplicates_choice === 'skip' ? t('docs-import-detail-dup-skip') : t('docs-import-detail-dup-copy'))
  const typed = props.job.options?.leave_out ?? []
  if (typed.length > 0) bits.push(t('docs-import-detail-left-out', { patterns: typed.join(', ') }))
  return bits.join(' · ')
})

const tiles = computed(() => {
  const s = summary.value
  const c = counts.value
  if (!s || !c) return []
  return [
    { id: 'new', label: t('docs-import-tile-new'), value: props.job.records_committed ?? c.imported, sub: '' },
    { id: 'images', label: t('docs-import-tile-images'), value: s.images, sub: '' },
    { id: 'links', label: t('docs-import-tile-links'), value: s.links_rewritten, sub: '' },
    {
      id: 'skipped',
      label: t('docs-import-tile-skipped'),
      value: c.skipped,
      sub: c.skipped > 0 ? t('docs-import-tile-skipped-already', { count: c.skipped }) : '',
    },
  ]
})

const collectionTarget = computed(() => collectionAfterImport(summary.value, docs.allCollections))
const dest = computed(() => ({
  ...destinationOf(summary.value),
  ...(collectionTarget.value ? { name: collectionTarget.value.name } : {}),
}))

function open() {
  const target = collectionTarget.value
  if (target) void router.push({ name: 'collection-view', params: { slug: target.slug } })
  else void router.push({ name: 'documentation' })
}

const downloading = ref(false)
const errorMessage = ref('')

async function downloadReport() {
  if (downloading.value) return
  downloading.value = true
  errorMessage.value = ''
  try {
    const { blob, filename } = await docsImportService.report(props.job.id)
    const link = document.createElement('a')
    link.href = URL.createObjectURL(blob)
    link.download = filename
    document.body.appendChild(link)
    link.click()
    document.body.removeChild(link)
    URL.revokeObjectURL(link.href)
  } catch (e) {
    const status = (e as { response?: { status?: number } })?.response?.status
    errorMessage.value =
      status === 410 ? t('import-detail-kept-30-days') : (refusalText(t, errorBody(e)) ?? t('docs-import-report-failed'))
  } finally {
    downloading.value = false
  }
}
</script>

<template>
  <div class="flex flex-col gap-6" data-test="docs-import-detail">
    <div class="flex flex-col gap-2">
      <h1 class="text-2xl font-bold text-primary break-words" data-test="import-detail-title">{{ job.filename }}</h1>
      <p class="text-secondary">{{ subtitle }}</p>
    </div>

    <dl class="grid grid-cols-1 sm:grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-0.5 sm:gap-y-1.5 text-sm" data-test="import-detail-facts">
      <dt class="text-tertiary mt-2 sm:mt-0">{{ $t('import-detail-state') }}</dt>
      <dd class="text-primary" data-test="import-detail-state">{{ stateLabel }}</dd>
      <template v-if="sourceLabel">
        <dt class="text-tertiary mt-2 sm:mt-0">{{ $t('docs-import-detail-source') }}</dt>
        <dd class="text-primary break-words">{{ sourceLabel }}</dd>
      </template>
      <template v-if="optionsLabel">
        <dt class="text-tertiary mt-2 sm:mt-0">{{ $t('import-detail-options') }}</dt>
        <dd class="text-primary break-words">{{ optionsLabel }}</dd>
      </template>
    </dl>

    <ErrorBanner v-if="errorMessage" :message="errorMessage" />

    <div v-if="tiles.length > 0" class="grid grid-cols-2 sm:grid-cols-4 gap-3" data-test="docs-detail-tiles">
      <div v-for="tile in tiles" :key="tile.id" class="bg-surface border border-default rounded-lg px-4 py-3.5 flex flex-col gap-1 min-w-0">
        <span class="text-xs uppercase tracking-wide text-tertiary">{{ tile.label }}</span>
        <span class="text-xl sm:text-2xl font-semibold text-primary leading-tight">{{ fmt(tile.value) }}</span>
        <span v-if="tile.sub" class="text-xs text-tertiary">{{ tile.sub }}</span>
      </div>
    </div>

    <SectionCard v-if="kept && view" content-padding="" data-test="docs-detail-tree">
      <template #title>{{ $t('docs-import-detail-tree-title') }}</template>
      <DocsPageTree
        v-model:expanded="expanded"
        :tree="tree"
        :groups="groups"
        mode="detail"
        :outcomes="outcomes"
        :note-counts="noteCounts"
        :links="links"
        :label="$t('docs-import-detail-tree-title')"
      />
      <p class="px-3 py-2 border-t border-default bg-surface-alt text-xs text-tertiary">
        {{ $t('docs-import-detail-tree-footer') }}
      </p>
    </SectionCard>

    <div class="flex flex-wrap items-center gap-2" data-test="import-detail-actions">
      <Button icon="openExternal" data-test="docs-detail-open" @click="open">
        {{ collectionTarget ? $t('docs-import-done-open', { name: collectionTarget.name }) : $t('docs-import-done-open-docs') }}
      </Button>
      <DocsPublishAction v-if="summary" :job="job" :summary="summary" :collection="collectionTarget?.name ?? null" />
      <Button
        v-if="kept"
        variant="secondary"
        icon="download"
        :loading="downloading"
        data-test="import-detail-report"
        @click="downloadReport"
      >
        {{ $t('docs-import-report') }}
      </Button>
    </div>

    <p class="text-sm text-tertiary" data-test="import-detail-kept">
      {{ kept ? $t('docs-import-detail-kept') : $t('import-detail-kept-30-days') }}
    </p>

    <DocsRemoveBlock :job="job" :dest="dest" @updated="emit('updated', $event)" />
  </div>
</template>
