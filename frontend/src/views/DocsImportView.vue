<script setup lang="ts">
/**
 * Documentation import: Start, Pages, Review, Done.
 *
 *  - Start  : drop one .zip or some .md files; the upload streams with its
 *             progress, the server reads the archive and answers with the
 *             job in `parsed` and the tree. A refused file stays on Start
 *             with the reason.
 *  - Pages  : the tree as Nosdesk will show it, ticks, "Where it goes".
 *             Another profile or a collection layout reads the tree again
 *             (`check: false`); Check stores the options and runs the dry
 *             run in the background (`checking`, then `dry_run_done`, or back
 *             on Pages with the reason).
 *  - Review : the plan (`docsImportService.allPages`), the duplicates
 *             choice, the preview; Import starts the commit in the
 *             background. A plan that changed since the check is refused in
 *             place and shown.
 *  - Done   : follows the commit (`committing`) to `done`.
 *
 * Nothing in the workspace changes before Import. The job id rides in
 * `?job=` so a reload or a link resumes where the job is, including a check
 * or a commit still running.
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { useRoute, useRouter } from 'vue-router'
import { useQuery, useQueryCache } from '@pinia/colada'

import BackButton from '@/components/common/BackButton.vue'
import ErrorBanner from '@/components/common/ErrorBanner.vue'
import Icon from '@/components/common/Icon.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import Spinner from '@/components/common/Spinner.vue'
import DocsDoneStep from '@/components/import/docsImport/DocsDoneStep.vue'
import DocsPagesStep from '@/components/import/docsImport/DocsPagesStep.vue'
import DocsReviewStep from '@/components/import/docsImport/DocsReviewStep.vue'
import type { CollectionChoice } from '@/components/import/docsImport/DocsWhereItGoes.vue'
import {
  changedRows,
  collectionAfterImport,
  destinationText,
  type DestinationWords,
} from '@/components/import/docsImport/docsReviewModel'
import {
  buildTree,
  collectionGroups,
  defaultMode,
  layoutOf,
  optionsPayload,
  patternsProblem,
  splitPatterns,
  tickPatterns,
  type PagesChoices,
} from '@/components/import/docsImport/docsTreeModel'
import { detailPath, resumeTarget } from '@/components/import/historyModel'
import { errorBody, refusalText } from '@/components/import/refusals'
import { importService } from '@nosdesk/core/services/importService'
import {
  docsImportService,
  docsPagesQueryKey,
  docsSummary,
  DOCS_JOB_TYPE,
  type DocsDuplicates,
  type DocsImportJob,
  type DocsPageRow,
} from '@nosdesk/core/services/docsImportService'
import { useSyncDocsStore, isActivePage } from '@nosdesk/core/sync/stores/documentation'
import { formatFileSize } from '@nosdesk/core/utils/formatFileSize'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const route = useRoute()
const router = useRouter()
const queryCache = useQueryCache()
const docs = useSyncDocsStore()

type Step = 'start' | 'pages' | 'review' | 'done'
const STEPS: Step[] = ['start', 'pages', 'review', 'done']
const STEP_KEYS: Record<Step, string> = {
  start: 'import-step-start',
  pages: 'docs-import-step-pages',
  review: 'import-step-review',
  done: 'import-step-done',
}

/** The upload's cap, and the pages one import takes. */
const MAX_BYTES = 200 * 1024 * 1024

const step = ref<Step>('start')
const job = ref<DocsImportJob | null>(null)
const busy = ref<'upload' | 'reread' | 'check' | 'commit' | 'resume' | 'discard' | null>(null)
const errorMessage = ref('')
/** The plan changed under a commit: what changed. */
const notice = ref('')
const downloading = ref(false)

const fileInput = ref<HTMLInputElement | null>(null)
const dropZone = ref<HTMLButtonElement | null>(null)
const isDragOver = ref(false)
const uploading = ref<{ name: string; size: number; loaded: number; reading: boolean } | null>(null)
const uploadError = ref<{ message: string; detail: string } | null>(null)

const stepIndex = computed(() => STEPS.indexOf(step.value))

const linkClass =
  'text-accent hover:underline focus:outline-none focus-visible:ring-1 focus-visible:ring-accent rounded'

// ---- polling ----

let alive = true
onBeforeUnmount(() => {
  alive = false
})
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

/** Follow a background run: read the job until it leaves `status`. */
async function follow(id: string, status: 'checking' | 'committing', every: number): Promise<DocsImportJob | null> {
  for (let i = 0; i < 1800 && alive; i++) {
    await sleep(every)
    if (!alive || job.value?.id !== id) return null
    let found: DocsImportJob
    try {
      found = await docsImportService.get(id)
    } catch {
      continue
    }
    if (job.value?.id !== id) return null
    job.value = found
    if (found.status !== status) return found
  }
  return null
}

// ---- route ----

function setJobQuery(id: string | null) {
  const query = { ...route.query }
  if (id) query.job = id
  else delete query.job
  void router.replace({ query })
}

function stepForStatus(status: DocsImportJob['status']): Step {
  switch (status) {
    case 'parsed':
    case 'checking':
      return 'pages'
    case 'dry_run_done':
      return 'review'
    case 'committing':
    case 'done':
      return 'done'
    default:
      return 'start'
  }
}

function apiError(e: unknown, fallbackKey: string, extra: Record<string, string | number> = {}): string {
  return refusalText(t, errorBody(e), extra) ?? t(fallbackKey)
}

// ---- the tree and the choices ----

const tree = computed(() => buildTree(job.value?.source?.tree.pages ?? []))

const emptyChoices = (): PagesChoices => ({
  profile: null,
  mode: 'new',
  collectionName: '',
  collectionId: null,
  underPageId: null,
  status: 'drafts',
  webImages: 'download',
  duplicates: 'skip',
  typed: [],
})

const choices = ref<PagesChoices>(emptyChoices())
const off = ref<Set<string>>(new Set())
const expanded = ref<Set<string>>(new Set())
const duplicates = ref<DocsDuplicates>('skip')
/** Set while choices are loaded from a job, so loading doesn't read the
 *  tree again. */
let loading = false

const collections = computed<CollectionChoice[]>(() => {
  const pages = new Map<number, number>()
  for (const p of docs.allPages) {
    if (p.collection_id != null && isActivePage(p)) pages.set(p.collection_id, (pages.get(p.collection_id) ?? 0) + 1)
  }
  return [...docs.allCollections]
    .filter((c) => !c.is_system)
    .sort((a, b) => a.name.localeCompare(b.name))
    .map((c) => ({ id: c.id, name: c.name, pages: pages.get(c.id) ?? 0 }))
})

/** An existing collection named like the import: Pages selects it and
 *  says why. */
const suggested = ref<CollectionChoice | null>(null)

const underPages = computed(() => {
  const cid = choices.value.collectionId
  if (cid == null) return []
  const rows = docs.allPages.filter((p) => p.collection_id === cid && isActivePage(p))
  const byParent = new Map<number | null, typeof rows>()
  for (const p of rows) {
    const parent = p.parent_id != null && rows.some((r) => r.id === p.parent_id) ? p.parent_id : null
    byParent.set(parent, [...(byParent.get(parent) ?? []), p])
  }
  const out: { id: number; title: string; depth: number }[] = []
  const walk = (parent: number | null, depth: number) => {
    const kids = (byParent.get(parent) ?? []).sort(
      (a, b) => (a.display_order ?? 0) - (b.display_order ?? 0) || a.title.localeCompare(b.title),
    )
    for (const p of kids) {
      out.push({ id: p.id, title: p.title || t('docs-import-untitled'), depth })
      if (depth < 4) walk(p.id, depth + 1)
    }
  }
  walk(null, 0)
  return out
})

const existingTarget = computed(() => {
  const c = collections.value.find((x) => x.id === choices.value.collectionId)
  if (!c) return null
  const under = underPages.value.find((p) => p.id === choices.value.underPageId)
  return { name: c.name, under: under?.title ?? null }
})

const groups = computed(() => {
  const view = job.value?.source?.tree
  if (!view) return []
  return collectionGroups(tree.value, view, choices.value, existingTarget.value)
})

/** Load the step's state from a job: its options, ticks and patterns. */
function loadChoices(found: DocsImportJob, keep: Partial<PagesChoices> = {}) {
  loading = true
  const view = found.source?.tree
  const o = found.options ?? {}
  const split = view ? splitPatterns(buildTree(view.pages), o.leave_out ?? []) : { off: new Set<string>(), typed: [] }
  const next: PagesChoices = {
    profile: o.profile ?? null,
    mode: o.collections ?? (view ? defaultMode(view.profile) : 'new'),
    collectionName: o.collection_name ?? '',
    collectionId: o.collection_id ?? null,
    underPageId: o.under_page_id ?? null,
    status: o.status ?? 'drafts',
    webImages: o.web_images ?? 'download',
    duplicates: o.duplicates ?? 'skip',
    typed: split.typed,
    ...keep,
  }
  suggested.value = null
  // A fresh import whose name matches an existing collection goes into it.
  if (!o.collections && !keep.mode && next.mode === 'new' && view?.collections.length === 1) {
    const name = view.collections[0].toLowerCase()
    const match = collections.value.find((c) => c.name.toLowerCase() === name)
    if (match) {
      next.mode = 'existing'
      next.collectionId = match.id
      suggested.value = match
    }
  }
  choices.value = next
  off.value = split.off
  duplicates.value = next.duplicates
  expanded.value = new Set(buildTree(view?.pages ?? []).roots.filter((n) => n.children.length > 0).map((n) => n.page.path))
  void nextTick(() => {
    loading = false
  })
}

// Another profile, or a collection layout, reads the tree again.
watch(
  () => [choices.value.profile, layoutOf(choices.value.mode)] as const,
  (now, before) => {
    if (loading || !job.value || step.value !== 'pages') return
    if (now[0] === before[0] && now[1] === before[1]) return
    void reread()
  },
)

async function reread() {
  if (!job.value) return
  busy.value = 'reread'
  errorMessage.value = ''
  const id = job.value.id
  try {
    const payload = optionsPayload(choices.value, [], 'reread')
    const found = await docsImportService.putOptions(id, payload, false)
    if (job.value?.id !== id) return
    // The ticks stay by path; the pages that are gone drop out.
    const keepOff = off.value
    const keep = { ...choices.value }
    job.value = found
    loadChoices(found, keep)
    off.value = new Set([...keepOff].filter((p) => buildTree(found.source?.tree.pages ?? []).byPath.has(p)))
  } catch (e) {
    errorMessage.value = apiError(e, 'docs-import-error-generic')
  } finally {
    busy.value = null
  }
}

// ---- resume ----

async function resume(id: string) {
  busy.value = 'resume'
  try {
    const found = await docsImportService.get(id)
    if ((found.job_type as string) !== DOCS_JOB_TYPE) {
      void router.replace(resumeTarget(id, found.job_type))
      return
    }
    if (found.status === 'done' && found.pruned_at) {
      void router.replace(detailPath(found.id))
      return
    }
    if (found.state === 'expired') {
      job.value = null
      step.value = 'start'
      errorMessage.value = t('import-history-expired-resume')
      setJobQuery(null)
      return
    }
    if (!found.source) throw new Error('no tree')
    job.value = found
    loadChoices(found)
    step.value = stepForStatus(found.status)
    if (found.error_message && (found.status === 'parsed' || found.status === 'dry_run_done')) {
      errorMessage.value = found.error_message
    }
    busy.value = null
    if (found.status === 'checking') await waitForCheck(found.id)
    else if (found.status === 'committing') await waitForCommit(found.id)
  } catch {
    job.value = null
    step.value = 'start'
    errorMessage.value = t('import-resume-failed')
    setJobQuery(null)
  } finally {
    if (busy.value === 'resume') busy.value = null
  }
}

onMounted(() => {
  const id = route.query.job
  if (typeof id === 'string' && id) void resume(id)
})

// ---- start ----

function chooseFiles() {
  if (busy.value) return
  fileInput.value?.click()
}

function pickFiles(event: Event) {
  const input = event.target as HTMLInputElement
  const files = [...(input.files ?? [])]
  input.value = ''
  if (files.length > 0) void acceptFiles(files)
}

const isZip = (f: File) => /\.zip$/i.test(f.name) || f.type === 'application/zip' || f.type === 'application/x-zip-compressed'
const isMarkdown = (f: File) => /\.(md|markdown|mdx)$/i.test(f.name)

async function acceptFiles(files: File[]) {
  if (busy.value) return
  errorMessage.value = ''
  const size = files.reduce((n, f) => n + f.size, 0)
  const name = files.length === 1 ? files[0].name : t('docs-import-files-count', { count: files.length })
  const detail = `${name} · ${formatFileSize(size)}`
  const oneZip = files.length === 1 && isZip(files[0])
  if (!oneZip && !files.every(isMarkdown)) {
    uploadError.value = { message: t('docs-import-refused-mixed-files'), detail }
    return
  }
  if (size > MAX_BYTES) {
    uploadError.value = {
      message: t('docs-import-refused-too-large', { name, size: formatFileSize(size), max: formatFileSize(MAX_BYTES) }),
      detail,
    }
    return
  }
  uploadError.value = null
  busy.value = 'upload'
  uploading.value = { name, size, loaded: 0, reading: false }
  try {
    const found = await docsImportService.upload(files, (loaded, total) => {
      if (!uploading.value) return
      uploading.value = { ...uploading.value, loaded, size: total || size, reading: loaded >= (total || size) }
    })
    const kept = keptChoices.value
    keptChoices.value = null
    job.value = found
    loadChoices(found, kept ?? {})
    setJobQuery(found.id)
    step.value = 'pages'
    queryCache.invalidateQueries({ key: ['import-jobs'] })
    // Kept choices that lay the tree out differently read it again.
    if (kept?.mode && found.source && layoutOf(kept.mode) !== layoutOf(defaultMode(found.source.tree.profile))) {
      busy.value = null
      await reread()
    }
  } catch (e) {
    uploadError.value = {
      message: apiError(e, 'docs-import-error-generic', { name, size: formatFileSize(size) }),
      detail,
    }
  } finally {
    if (busy.value === 'upload') busy.value = null
    uploading.value = null
  }
}

function onDragOver(event: DragEvent) {
  event.preventDefault()
  if (event.dataTransfer) event.dataTransfer.dropEffect = busy.value ? 'none' : 'copy'
  isDragOver.value = !busy.value
}

function onDrop(event: DragEvent) {
  event.preventDefault()
  isDragOver.value = false
  const files = [...(event.dataTransfer?.files ?? [])]
  if (files.length > 0) void acceptFiles(files)
}

const uploadPct = computed(() =>
  uploading.value && uploading.value.size > 0 ? Math.min(100, Math.round((uploading.value.loaded / uploading.value.size) * 100)) : 0,
)

// ---- pages ----

/** "Change file" keeps where the pages go for the next file. */
const keptChoices = ref<Partial<PagesChoices> | null>(null)

async function changeFile() {
  if (!job.value || busy.value) return
  busy.value = 'discard'
  errorMessage.value = ''
  try {
    await importService.remove(job.value.id)
  } catch (e) {
    if ((e as { response?: { status?: number } })?.response?.status !== 404) {
      errorMessage.value = apiError(e, 'docs-import-error-generic')
      busy.value = null
      return
    }
  }
  const c = choices.value
  keptChoices.value = {
    status: c.status,
    webImages: c.webImages,
    duplicates: c.duplicates,
    typed: c.typed,
    ...(c.mode === 'existing' ? { mode: c.mode, collectionId: c.collectionId, underPageId: c.underPageId } : {}),
  }
  busy.value = null
  resetToStart()
  queryCache.invalidateQueries({ key: ['import-jobs'] })
}

function resetToStart() {
  job.value = null
  setJobQuery(null)
  uploadError.value = null
  errorMessage.value = ''
  notice.value = ''
  changed.value = new Set()
  step.value = 'start'
}

async function check() {
  if (!job.value || busy.value) return
  const patterns = [...choices.value.typed, ...tickPatterns(tree.value, off.value)]
  const problem = patternsProblem(patterns)
  if (problem) {
    errorMessage.value = t(problem === 'too_many' ? 'docs-import-too-many-patterns' : 'docs-import-pattern-too-long')
    return
  }
  if (choices.value.mode === 'existing' && choices.value.collectionId == null) {
    errorMessage.value = t('docs-import-choose-collection')
    return
  }
  busy.value = 'check'
  errorMessage.value = ''
  notice.value = ''
  changed.value = new Set()
  const id = job.value.id
  try {
    const payload = optionsPayload(choices.value, tickPatterns(tree.value, off.value), 'check')
    job.value = await docsImportService.putOptions(id, payload, true)
    duplicates.value = choices.value.duplicates
    await waitForCheck(id)
  } catch (e) {
    errorMessage.value = apiError(e, 'docs-import-error-generic')
  } finally {
    if (busy.value === 'check') busy.value = null
  }
}

async function waitForCheck(id: string) {
  busy.value = 'check'
  step.value = 'pages'
  const found = job.value?.status === 'checking' ? await follow(id, 'checking', 1500) : job.value
  if (busy.value === 'check') busy.value = null
  if (!found || job.value?.id !== id) return
  if (found.status === 'dry_run_done') {
    duplicates.value = found.options?.duplicates ?? 'skip'
    step.value = 'review'
    window.scrollTo({ top: 0 })
  } else if (found.status === 'parsed') {
    errorMessage.value = found.error_message ?? t('docs-import-check-failed')
  }
}

// ---- review ----

const summary = computed(() => docsSummary(job.value))

const rowsQuery = useQuery({
  key: () => docsPagesQueryKey(job.value?.id ?? '', job.value?.updated_at ?? ''),
  query: () => docsImportService.allPages(job.value!.id),
  enabled: () =>
    !!job.value && step.value === 'review' && job.value.status === 'dry_run_done' && !job.value.pruned_at,
})
const rows = ref<DocsPageRow[] | null>(null)
watch(
  () => rowsQuery.data.value,
  (data) => {
    if (data) rows.value = data
  },
  { immediate: true },
)
watch(
  () => job.value?.id,
  () => {
    rows.value = null
  },
)

/** Rows whose plan moved under a refused commit. */
const changed = ref<Set<number>>(new Set())

const destWords = computed<DestinationWords>(() => {
  const s = summary.value
  const d = s?.destination
  if (!d) return { mode: 'new', name: '' }
  if (d.mode === 'existing') {
    const under = d.under_page_id != null ? docs.allPages.find((p) => p.id === d.under_page_id)?.title : null
    return { mode: 'existing', name: d.collection_name ?? '', under }
  }
  if (d.new_collections.length === 1) return { mode: 'new', name: d.new_collections[0] }
  return { mode: 'many', name: '', count: d.new_collections.length }
})
const dest = computed(() => destinationText(t, destWords.value))

function goBackToPages() {
  if (busy.value) return
  errorMessage.value = ''
  notice.value = ''
  step.value = 'pages'
}

async function commit() {
  if (!job.value || busy.value) return
  busy.value = 'commit'
  errorMessage.value = ''
  notice.value = ''
  const id = job.value.id
  const before = rows.value ?? []
  try {
    job.value = await docsImportService.commit(id, duplicates.value)
    step.value = 'done'
    changed.value = new Set()
    queryCache.invalidateQueries({ key: ['import-jobs'] })
    busy.value = null
    await waitForCommit(id)
  } catch (e) {
    const data = (e as { response?: { data?: { code?: string; error?: string; job?: DocsImportJob } } })?.response?.data
    if (data?.code === 'import_plan_changed' && data.job) {
      job.value = data.job
      duplicates.value = data.job.options?.duplicates ?? duplicates.value
      try {
        const after = await docsImportService.allPages(id)
        queryCache.setQueryData(docsPagesQueryKey(id, data.job.updated_at), after)
        rows.value = after
        changed.value = changedRows(before, after)
        const more = after.filter((r) => r.duplicate).length - before.filter((r) => r.duplicate).length
        notice.value =
          more > 0
            ? t('docs-import-plan-changed-more', { count: more })
            : (data.job.error_message ?? data.error ?? t('docs-import-plan-changed'))
      } catch {
        notice.value = data.job.error_message ?? t('docs-import-plan-changed')
      }
    } else if (data?.code === 'import_commit_running') {
      step.value = 'done'
      busy.value = null
      job.value = await docsImportService.get(id).catch(() => job.value)
      await waitForCommit(id)
    } else {
      errorMessage.value = apiError(e, 'docs-import-error-generic')
    }
  } finally {
    if (busy.value === 'commit') busy.value = null
  }
}

async function waitForCommit(id: string) {
  step.value = 'done'
  const found = job.value?.status === 'committing' ? await follow(id, 'committing', 2000) : job.value
  if (!found || job.value?.id !== id) return
  queryCache.invalidateQueries({ key: ['import-jobs'] })
  if (found.status === 'dry_run_done') {
    // The commit failed and wrote nothing; the job is back on Review.
    step.value = 'review'
    errorMessage.value = found.error_message ?? t('docs-import-commit-failed')
  } else if (found.status === 'done') {
    queryCache.invalidateQueries({ key: ['import-job', id] })
  }
}

async function downloadReport() {
  if (!job.value || downloading.value) return
  downloading.value = true
  errorMessage.value = ''
  try {
    const { blob, filename } = await docsImportService.report(job.value.id)
    const link = document.createElement('a')
    link.href = URL.createObjectURL(blob)
    link.download = filename
    document.body.appendChild(link)
    link.click()
    document.body.removeChild(link)
    URL.revokeObjectURL(link.href)
  } catch (e) {
    errorMessage.value = apiError(e, 'docs-import-report-failed')
  } finally {
    downloading.value = false
  }
}

// ---- done ----

/** The collection the pages went into, when there is one. */
const doneCollection = computed(() => {
  const w = destWords.value
  return w.mode === 'many' ? null : w.name || null
})

function openCollection() {
  const target = collectionAfterImport(summary.value, docs.allCollections)
  if (target) void router.push({ name: 'collection-view', params: { slug: target.slug } })
  else void router.push({ name: 'documentation' })
}

async function importAnother() {
  keptChoices.value = null
  choices.value = emptyChoices()
  resetToStart()
  await nextTick()
  dropZone.value?.focus()
}

// ---- stepper ----

function canGoTo(target: Step): boolean {
  return target === 'pages' && step.value === 'review' && !busy.value
}

function goTo(target: Step) {
  if (canGoTo(target)) goBackToPages()
}
</script>

<template>
  <div class="flex-1">
    <div class="pt-4 px-4 sm:px-6 flex justify-between items-center">
      <BackButton fallbackRoute="/admin/data-import" :label="$t('csv-import-back')" />
    </div>

    <div class="flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-4xl" data-test="docs-import">
      <div class="flex flex-col gap-2">
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ $t('docs-import-title') }}</h1>
        <p class="text-sm sm:text-base text-secondary">{{ $t('docs-import-subtitle') }}</p>
      </div>

      <nav :aria-label="$t('import-stepper-label')" class="flex flex-col gap-2">
        <ol class="flex items-center gap-2 sm:gap-3 text-sm">
          <li v-for="(s, i) in STEPS" :key="s" class="flex items-center gap-2">
            <component
              :is="canGoTo(s) ? 'button' : 'span'"
              :type="canGoTo(s) ? 'button' : undefined"
              class="flex items-center gap-2"
              :aria-current="step === s ? 'step' : undefined"
              :data-step="s"
              @click="goTo(s)"
            >
              <span
                class="w-7 h-7 rounded-full flex items-center justify-center font-medium border"
                :class="step === s
                  ? 'bg-accent text-on-accent border-accent'
                  : i < stepIndex
                    ? 'bg-status-success text-on-accent border-status-success'
                    : 'bg-surface-alt text-secondary border-default'"
              >
                <Icon v-if="i < stepIndex" name="check" size="sm" />
                <template v-else>{{ i + 1 }}</template>
              </span>
              <span
                class="hidden sm:inline"
                :class="[step === s ? 'text-primary font-medium' : 'text-secondary', canGoTo(s) ? 'hover:underline' : '']"
              >
                {{ $t(STEP_KEYS[s]) }}
              </span>
            </component>
            <span v-if="i < STEPS.length - 1" class="w-3 sm:w-6 h-px bg-default" />
          </li>
        </ol>
        <p class="sm:hidden text-xs text-secondary">
          {{ $t('import-step-progress', { current: stepIndex + 1, total: STEPS.length, name: $t(STEP_KEYS[step]) }) }}
        </p>
        <p v-if="step !== 'done'" class="flex items-start gap-2 text-xs-plus text-tertiary" data-test="docs-nothing-changes">
          <Icon name="lock" size="xs" class="mt-[3px] shrink-0" />
          <span>{{ $t('docs-import-nothing-changes') }}</span>
        </p>
      </nav>

      <ErrorBanner v-if="notice" :message="notice" data-test="docs-plan-changed" />
      <ErrorBanner v-if="errorMessage" :message="errorMessage" data-test="docs-import-error" />

      <p v-if="busy === 'resume'" class="flex items-center gap-2 text-sm text-secondary">
        <Spinner size="sm" />
        {{ $t('import-resuming') }}
      </p>

      <!-- Start ---------------------------------------------------------- -->
      <section v-else-if="step === 'start'" class="flex flex-col gap-6" data-test="docs-start">
        <SectionCard content-padding="p-4">
          <template #title>{{ $t('docs-import-start-heading') }}</template>
          <div class="flex flex-col gap-4">
            <ErrorBanner
              v-if="uploadError"
              :message="uploadError.message"
              :detail="uploadError.detail"
              role="alert"
              data-test="docs-upload-error"
            />

            <div
              v-if="uploading"
              class="flex flex-col items-center justify-center gap-3 border-2 border-dashed border-default rounded-xl px-4 py-10 min-h-[176px] text-center bg-surface-alt/60"
              aria-busy="true"
              aria-live="polite"
              data-test="docs-uploading"
            >
              <template v-if="!uploading.reading">
                <div class="w-full max-w-xs h-1.5 rounded-full bg-surface-hover overflow-hidden">
                  <span class="block h-full bg-accent rounded-full transition-[width] duration-300" :style="{ width: `${uploadPct}%` }" />
                </div>
                <div class="flex flex-col items-center gap-1 min-w-0 max-w-full">
                  <p class="text-sm text-primary font-medium break-all">{{ $t('docs-import-uploading', { name: uploading.name }) }}</p>
                  <p class="text-xs text-tertiary">
                    {{ $t('docs-import-uploading-progress', { loaded: formatFileSize(uploading.loaded), total: formatFileSize(uploading.size) }) }}
                  </p>
                </div>
              </template>
              <template v-else>
                <Spinner size="md" />
                <div class="flex flex-col items-center gap-1 min-w-0 max-w-full">
                  <p class="text-sm text-primary font-medium">{{ $t('docs-import-reading') }}</p>
                  <p class="text-xs text-tertiary break-all">{{ uploading.name }} · {{ formatFileSize(uploading.size) }}</p>
                </div>
              </template>
            </div>

            <button
              v-else
              ref="dropZone"
              type="button"
              class="w-full flex flex-col items-center justify-center gap-3 border-2 border-dashed rounded-xl px-3 sm:px-4 py-7 sm:py-10 min-h-[176px] text-center cursor-pointer transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
              :class="isDragOver ? 'border-accent bg-accent/10' : 'border-default bg-surface-alt/60 hover:border-strong hover:bg-surface-alt'"
              data-test="docs-drop"
              @click="chooseFiles"
              @dragover="onDragOver"
              @dragleave="isDragOver = false"
              @drop="onDrop"
            >
              <span
                class="w-12 h-12 rounded-full flex items-center justify-center"
                :class="isDragOver ? 'bg-accent/20 text-accent' : 'bg-surface text-tertiary'"
              >
                <Icon name="archive" size="md" />
              </span>
              <span class="flex flex-col items-center gap-1">
                <span class="text-sm text-primary font-medium">
                  {{ isDragOver ? $t('docs-import-drop-release') : $t('docs-import-drop-idle') }}
                </span>
                <span class="text-xs text-tertiary">{{ $t('docs-import-drop-hint') }}</span>
              </span>
            </button>
            <input
              ref="fileInput"
              type="file"
              multiple
              accept=".zip,application/zip,.md,.markdown,.mdx,text/markdown"
              class="hidden"
              tabindex="-1"
              aria-hidden="true"
              data-test="docs-file"
              @change="pickFiles"
            />

            <ul class="list-disc pl-[18px] flex flex-col gap-1 text-xs text-tertiary">
              <li>{{ $t('docs-import-start-sources') }}</li>
              <li>{{ $t('docs-import-start-images') }}</li>
              <li>{{ $t('docs-import-start-attachments') }}</li>
            </ul>
          </div>
        </SectionCard>

        <SectionCard content-padding="p-4">
          <template #title>{{ $t('docs-import-comes-across-heading') }}</template>
          <div class="flex flex-col gap-2 text-sm text-secondary">
            <p>{{ $t('docs-import-comes-across-1') }}</p>
            <p>{{ $t('docs-import-comes-across-2') }}</p>
          </div>
        </SectionCard>

        <p class="text-sm">
          <RouterLink :class="linkClass" :to="{ name: 'admin-data-import' }" data-test="docs-recent-link">
            {{ $t('import-history-title') }}
          </RouterLink>
        </p>
      </section>

      <!-- Pages ---------------------------------------------------------- -->
      <DocsPagesStep
        v-else-if="step === 'pages' && job?.source"
        :key="job.id"
        v-model:choices="choices"
        v-model:off="off"
        v-model:expanded="expanded"
        :filename="job.filename"
        :source="job.source"
        :tree="tree"
        :groups="groups"
        :collections="collections"
        :under-pages="underPages"
        :suggested="suggested"
        :checking="busy === 'check'"
        :rereading="busy === 'reread'"
        :discarding="busy === 'discard'"
        @check="check"
        @change-file="changeFile"
      />

      <!-- Review --------------------------------------------------------- -->
      <DocsReviewStep
        v-else-if="step === 'review' && job && summary"
        :key="`${job.id}:review`"
        v-model:duplicates="duplicates"
        :job="job"
        :summary="summary"
        :rows="rows"
        :rows-failed="!!rowsQuery.error.value && !rows"
        :changed="changed"
        :dest="dest"
        :committing="busy === 'commit'"
        :downloading="downloading"
        @back="goBackToPages"
        @commit="commit"
        @report="downloadReport"
      />

      <!-- Done ----------------------------------------------------------- -->
      <template v-else-if="step === 'done' && job && summary">
        <DocsDoneStep
          :job="job"
          :summary="summary"
          :collection="doneCollection"
          :downloading="downloading"
          @open="openCollection"
          @report="downloadReport"
          @another="importAnother"
          @recent="router.push({ name: 'admin-data-import' })"
        />
        <p v-if="job.status === 'done'" class="text-sm">
          <RouterLink :class="linkClass" :to="{ name: 'admin-data-import' }" data-test="docs-done-recent">
            {{ $t('import-history-title') }}
          </RouterLink>
        </p>
      </template>
    </div>
  </div>
</template>
