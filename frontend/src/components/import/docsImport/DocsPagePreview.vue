<script setup lang="ts">
/**
 * Review's preview of one page: its notes, then the converted page in a
 * read-only view over the editor's own schema, with images as named
 * placeholders and labels where the conversion changed something. Beside
 * the list on wide screens, in a sheet with Previous and Next on phones.
 */
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { useQuery } from '@pinia/colada'

import ErrorBanner from '@/components/common/ErrorBanner.vue'
import Icon from '@/components/common/Icon.vue'
import Spinner from '@/components/common/Spinner.vue'
import StatusPill from '@/components/common/StatusPill.vue'
import type { StatusPillTone } from '@/components/common/statusPillTone'
import { schema } from '@/components/editor/schema'
import { formatFileSize } from '@nosdesk/core/utils/formatFileSize'
import {
  docsImportService,
  docsPreviewQueryKey,
  type DocsPageRow,
  type DocsPreviewImage,
} from '@nosdesk/core/services/docsImportService'

import { IMAGE_PROBLEM_KEY, type NoteLine, type RowOutcome } from './docsReviewModel'
import { previewPath } from './docsTreeModel'
import { mountPreview, type PreviewHandle, type PreviewLabels } from './docsPreviewView'

const props = withDefaults(
  defineProps<{
    jobId: string
    updatedAt: string
    row: DocsPageRow | null
    outcome?: RowOutcome | null
    notes?: NoteLine[]
    /** Pages the import writes besides this one, for an error page's line. */
    others?: number
    /** In the sheet, whose header carries the title: the path and outcome
     *  lead the body instead. */
    headless?: boolean
    /** The folder the source sits in, left off the path shown. */
    root?: string | null
  }>(),
  { outcome: null, notes: () => [], others: 0, headless: false, root: null },
)

const emit = defineEmits<{
  /** A link to another page in the import was followed. */
  open: [index: number]
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const index = computed(() => props.row?.index ?? null)

const query = useQuery({
  key: () => docsPreviewQueryKey(props.jobId, props.updatedAt, index.value ?? -1),
  query: () => docsImportService.preview(props.jobId, index.value!),
  enabled: () => index.value != null,
})

const mountRef = ref<HTMLElement | null>(null)
const schemaError = ref('')
let handle: PreviewHandle | null = null

const labels: PreviewLabels = {
  get callout() {
    return t('docs-import-preview-was-callout')
  },
  get checklist() {
    return t('docs-import-preview-was-checklist')
  },
  get tableMoved() {
    return t('docs-import-preview-table-moved')
  },
  image(info: DocsPreviewImage | undefined, alt: string | null) {
    if (info?.problem) {
      return t('docs-import-preview-image-problem', {
        problem: t(IMAGE_PROBLEM_KEY[info.problem] ?? 'docs-import-image-unreadable'),
        alt: alt ?? '',
      })
    }
    const bits: string[] = []
    if (info?.kind === 'web') bits.push(t('docs-import-preview-image-web'))
    if (info?.size) bits.push(formatFileSize(info.size))
    if (alt) bits.push(alt)
    return bits.join(' · ')
  },
  imageAria(info: DocsPreviewImage | undefined, name: string) {
    return info?.problem
      ? t('docs-import-preview-image-aria-problem', { name })
      : t('docs-import-preview-image-aria', { name })
  },
}

function teardown() {
  handle?.destroy()
  handle = null
}

watch(
  [() => query.data.value, mountRef],
  ([data, mount]) => {
    teardown()
    schemaError.value = ''
    if (!data || !mount || data.index !== index.value || props.row?.error) return
    try {
      handle = mountPreview({
        mount,
        schema,
        json: data.doc,
        images: data.images,
        notes: props.row?.notes ?? {},
        labels,
        ariaLabel: t('docs-import-preview-aria', { title: data.title }),
        onPageLink: (i) => emit('open', i),
      })
    } catch (e) {
      schemaError.value = t('docs-import-preview-schema-failed', {
        reason: e instanceof Error ? e.message : String(e),
      })
    }
  },
  { flush: 'post' },
)

onBeforeUnmount(teardown)

const OUTCOME: Record<RowOutcome, { key: string; tone: StatusPillTone }> = {
  new: { key: 'docs-import-outcome-new', tone: 'positive' },
  copy: { key: 'docs-import-outcome-copy', tone: 'info' },
  skipped: { key: 'docs-import-outcome-skipped', tone: 'neutral' },
  error: { key: 'docs-import-outcome-error', tone: 'critical' },
}
</script>

<template>
  <div class="bg-surface flex flex-col overflow-hidden min-w-0 docs-preview" :class="headless ? '' : 'border border-default rounded-xl'" data-test="docs-preview">
    <div v-if="!headless" class="flex items-center gap-2 px-3 py-2 border-b border-default bg-surface-alt min-w-0">
      <div class="flex-1 min-w-0 flex flex-col">
        <b class="text-sm font-semibold text-primary truncate">{{ row ? row.title : $t('docs-import-preview-title') }}</b>
        <span class="text-2xs text-tertiary truncate" :class="row ? 'font-mono' : ''" :title="row?.path" data-test="docs-preview-path">
          {{ row ? previewPath(row.path, root) : $t('docs-import-preview-select') }}
        </span>
      </div>
      <StatusPill v-if="row && outcome" :label="$t(OUTCOME[outcome].key)" :tone="OUTCOME[outcome].tone" />
    </div>
    <div v-else-if="row" class="flex items-center gap-2 px-3 py-2 border-b border-default min-w-0">
      <span class="flex-1 min-w-0 text-2xs font-mono text-tertiary truncate" :title="row.path" data-test="docs-preview-path">{{ previewPath(row.path, root) }}</span>
      <StatusPill v-if="outcome" :label="$t(OUTCOME[outcome].key)" :tone="OUTCOME[outcome].tone" />
    </div>

    <div
      v-if="!row"
      class="px-4 py-10 flex flex-col items-center gap-2 text-center text-sm text-tertiary"
    >
      <Icon name="eye" size="md" class="opacity-60" />
      <span>{{ $t('docs-import-preview-empty') }}</span>
    </div>

    <template v-else>
      <div v-if="notes.length > 0" class="px-3 py-2 border-b border-default flex flex-col gap-1 text-xs text-secondary" data-test="docs-preview-notes">
        <span
          v-for="(n, i) in notes"
          :key="i"
          :class="n.tone === 'error' ? 'text-status-error' : n.tone === 'warning' ? 'text-status-warning' : ''"
        >{{ n.text }}</span>
      </div>

      <div class="docs-preview__scroll min-w-0">
        <div v-if="row.error" class="px-4 py-6 flex flex-col gap-2">
          <ErrorBanner :message="row.error" />
          <p class="text-sm text-secondary">{{ $t('docs-import-preview-error-line', { count: others }) }}</p>
        </div>
        <template v-else>
          <div class="flex items-center gap-1.5 px-3 py-1.5 text-2xs text-tertiary border-b border-default">
            <Icon name="lock" size="xs" />
            <span>{{ $t('docs-import-preview-read-only') }}</span>
          </div>
          <ErrorBanner v-if="schemaError" class="m-3" :message="schemaError" data-test="docs-preview-schema-error" />
          <ErrorBanner v-else-if="query.error.value" class="m-3" :message="$t('docs-import-preview-failed')" />
          <p v-else-if="!query.data.value" class="flex items-center gap-2 px-4 py-6 text-sm text-secondary">
            <Spinner size="sm" />
            {{ $t('docs-import-preview-loading') }}
          </p>
          <div v-show="!schemaError && query.data.value" ref="mountRef" class="docs-preview__editor" data-test="docs-preview-mount" />
        </template>
      </div>
    </template>
  </div>
</template>

<style scoped>
/* The editor's reading styles live with CollaborativeEditor, which this
   page doesn't load; the preview carries the ones a converted page uses. */
.docs-preview__editor :deep(.ProseMirror) {
  outline: none;
  padding: 16px;
  min-height: 120px;
  white-space: pre-wrap;
  word-wrap: break-word;
  color: var(--color-primary);
  font-size: max(1rem, 16px);
  line-height: 1.5;
}
.docs-preview__editor :deep(.ProseMirror > :first-child) { margin-top: 0; }
.docs-preview__editor :deep(.ProseMirror p) { margin: 0.5rem 0; line-height: 1.6; }
.docs-preview__editor :deep(.ProseMirror h1) { font-size: 2rem; font-weight: 700; margin: 1rem 0; border-bottom: 1px solid var(--color-default); padding-bottom: 0.5rem; line-height: 1.2; }
.docs-preview__editor :deep(.ProseMirror h2) { font-size: 1.5rem; font-weight: 700; margin: 1.5rem 0 1rem; line-height: 1.3; }
.docs-preview__editor :deep(.ProseMirror h3) { font-size: 1.25rem; font-weight: 600; margin: 1.5rem 0 1rem; line-height: 1.4; }
.docs-preview__editor :deep(.ProseMirror :is(h4, h5, h6)) { font-size: 1.0625rem; font-weight: 600; margin: 1.25rem 0 0.75rem; }
.docs-preview__editor :deep(.ProseMirror blockquote) { border-left: 3px solid var(--color-strong); padding: 0.5rem 1rem; margin: 1rem 0; color: var(--color-secondary); background: var(--color-surface-alt); border-radius: 0.375rem; }
.docs-preview__editor :deep(.ProseMirror pre) { background: var(--color-app); padding: 0.75rem; border-radius: 0.5rem; margin: 1rem 0; border: 1px solid var(--color-subtle); font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace; font-size: 0.875rem; overflow-x: auto; }
.docs-preview__editor :deep(.ProseMirror pre code) { background: transparent; padding: 0; display: block; white-space: pre-wrap; word-break: break-word; }
.docs-preview__editor :deep(.ProseMirror code) { background: var(--color-surface-alt); padding: 0.125rem 0.375rem; border-radius: 0.25rem; font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace; font-size: 0.875em; }
.docs-preview__editor :deep(.ProseMirror :is(ul, ol)) { padding-left: 1.5rem; margin: 1rem 0; }
.docs-preview__editor :deep(.ProseMirror ul) { list-style-type: disc; }
.docs-preview__editor :deep(.ProseMirror ol) { list-style-type: decimal; }
.docs-preview__editor :deep(.ProseMirror li) { margin-bottom: 0.5rem; line-height: 1.6; }
.docs-preview__editor :deep(.ProseMirror li p) { margin: 0.25rem 0; }
.docs-preview__editor :deep(.ProseMirror a) { color: var(--color-accent); text-decoration: underline; }
.docs-preview__editor :deep(.ProseMirror strong) { font-weight: 600; }
.docs-preview__editor :deep(.ProseMirror hr) { border: 0; border-top: 1px solid var(--color-default); margin: 1.5rem 0; }
.docs-preview__editor :deep(.embedded-document-block) { padding: 0.5rem 0.75rem; margin: 0.5rem 0; border: 1px solid var(--color-default); border-radius: 0.5rem; background: var(--color-surface-alt); font-size: 0.875rem; }

/* Where the conversion changed the page. */
.docs-preview__editor :deep(.docs-preview-note) { position: relative; margin-top: 1.25rem; }
.docs-preview__editor :deep(.docs-preview-note::after) {
  content: attr(data-note);
  position: absolute;
  top: -9px;
  right: 8px;
  font-size: 0.625rem;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--color-accent);
  background: var(--color-surface);
  border: 1px solid var(--color-accent);
  border-radius: 9999px;
  padding: 1px 7px;
  line-height: 1.4;
  white-space: nowrap;
  pointer-events: none;
}
.docs-preview__editor :deep(.docs-preview-note.is-warn::after) { color: var(--color-status-warning); border-color: var(--color-status-warning); }

/* An image that isn't stored yet. */
.docs-preview__editor :deep(.docs-preview-img) {
  display: inline-flex;
  align-items: center;
  gap: 10px;
  max-width: 100%;
  padding: 10px 12px;
  margin: 0.25rem 0;
  border: 1px dashed var(--color-strong);
  border-radius: 8px;
  background: var(--color-surface-alt);
  color: var(--color-secondary);
  font-size: 0.8125rem;
  vertical-align: top;
  line-height: 1.3;
  white-space: normal;
}
.docs-preview__editor :deep(.docs-preview-img svg) { width: 20px; height: 20px; flex-shrink: 0; color: var(--color-tertiary); }
.docs-preview__editor :deep(.docs-preview-img__text) { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
.docs-preview__editor :deep(.docs-preview-img__name) { font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace; font-size: 0.75rem; color: var(--color-primary); overflow-wrap: anywhere; }
.docs-preview__editor :deep(.docs-preview-img__meta) { font-size: 0.6875rem; color: var(--color-tertiary); }
.docs-preview__editor :deep(.docs-preview-img.is-problem) { border-color: color-mix(in srgb, var(--color-status-error) 30%, transparent); background: var(--color-status-error-muted); }
.docs-preview__editor :deep(.docs-preview-img.is-problem svg),
.docs-preview__editor :deep(.docs-preview-img.is-problem .docs-preview-img__meta) { color: var(--color-status-error); }
</style>
