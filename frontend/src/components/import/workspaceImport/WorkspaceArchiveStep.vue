<script setup lang="ts">
/**
 * The Archive step: the export file (and its password), then the checks
 * that run before anything is written. Each check is a row that fills in as
 * it runs; a refusal is a failed row with its reason and the one action that
 * fixes it.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import FormInput from '@/components/common/FormInput.vue'
import Icon from '@/components/common/Icon.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import Spinner from '@/components/common/Spinner.vue'
import { formatFileSize } from '@nosdesk/core/utils/formatFileSize'
import type {
  WorkspaceImportJob,
  WorkspaceImportRefusal,
} from '@nosdesk/core/services/workspaceImportService'

import { COUNT_KEYS, archiveChecks, tableWord, type CheckLine } from './workspaceImportModel'

const props = defineProps<{
  job: WorkspaceImportJob | null
  file: { name: string; size: number } | null
  uploading: boolean
  uploaded: { loaded: number; total: number }
  checking: boolean
  discarding: boolean
  refused: WorkspaceImportRefusal | null
  maxBytes: number
  serverVersion: string
  workspace: string
}>()

const password = defineModel<string>('password', { required: true })

const emit = defineEmits<{
  (e: 'upload', file: File): void
  (e: 'check', options: { regenerateUuids?: boolean }): void
  (e: 'chooseAnother'): void
  (e: 'follow', id: string): void
  (e: 'cancel'): void
  (e: 'next'): void
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const fileInput = ref<HTMLInputElement | null>(null)
const isDragOver = ref(false)

const lines = computed(() =>
  archiveChecks({
    job: props.job,
    file: props.file,
    uploading: props.uploading,
    refused: props.refused,
    maxBytes: props.maxBytes,
    serverVersion: props.serverVersion,
    workspace: props.workspace,
  }),
)

const anyFailed = computed(() => lines.value.some((l) => l.state === 'bad'))
const workspaceFailed = computed(() => lines.value[0].state === 'bad')
const ready = computed(() => props.job?.state === 'previewed')
const checking = computed(() => props.checking || props.job?.state === 'checking')

/** The file the page holds: the job's archive, else the one chosen. */
const shown = computed(() => {
  if (props.job) return { name: props.job.archive.name ?? t('workspace-import-archive-unnamed'), size: props.job.archive.size, sealed: props.job.archive.sealed }
  if (props.file && props.refused) return { name: props.file.name, size: props.file.size, sealed: false }
  return null
})

const uploadPct = computed(() =>
  props.uploaded.total > 0 ? Math.min(100, Math.round((props.uploaded.loaded / props.uploaded.total) * 100)) : 0,
)

/** Words for a check line, its arguments shown as people read them. */
function words(key: string | undefined, line: CheckLine): string {
  if (!key) return ''
  const args: Record<string, string | number> = { ...line.args }
  for (const k of ['size', 'max'] as const) {
    if (typeof args[k] === 'number') args[k] = formatFileSize(args[k] as number)
  }
  if (typeof args.date === 'string' && args.date) {
    const d = new Date(args.date)
    if (!Number.isNaN(d.getTime())) args.date = d.toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })
  }
  return t(key, args)
}

function foundWord(f: { what: string; rows: number }): string {
  if (f.what === 'files') return t('workspace-import-found-files', { count: f.rows })
  const key = COUNT_KEYS[f.what]
  return key ? t(key, { count: f.rows }) : t('workspace-import-found-rows', { what: tableWord(f.what), count: f.rows })
}

function choose() {
  if (props.uploading || workspaceFailed.value) return
  fileInput.value?.click()
}

function picked(event: Event) {
  const input = event.target as HTMLInputElement
  const chosen = input.files?.[0]
  input.value = ''
  if (chosen) emit('upload', chosen)
}

function onDragOver(event: DragEvent) {
  event.preventDefault()
  if (event.dataTransfer) event.dataTransfer.dropEffect = props.uploading ? 'none' : 'copy'
  isDragOver.value = !props.uploading
}

function onDrop(event: DragEvent) {
  event.preventDefault()
  isDragOver.value = false
  const chosen = event.dataTransfer?.files?.[0]
  if (chosen && !props.uploading && !workspaceFailed.value) emit('upload', chosen)
}

const ICON = { ok: 'checkCircle', bad: 'xCircle', todo: 'circleDot' } as const
const ICON_CLASS = { ok: 'text-status-success', bad: 'text-status-error', wait: 'text-tertiary', todo: 'text-tertiary opacity-50' }
</script>

<template>
  <section class="flex flex-col gap-6" data-test="workspace-archive">
    <SectionCard content-padding="p-4">
      <template #title>{{ $t('workspace-import-archive-heading') }}</template>
      <div class="flex flex-col gap-4">
        <!-- Uploading -->
        <div
          v-if="uploading"
          class="flex flex-col gap-3 border-2 border-dashed border-default rounded-xl px-4 py-4 bg-surface-alt/60"
          aria-busy="true"
          aria-live="polite"
          data-test="workspace-uploading"
        >
          <div class="flex items-center gap-3 min-w-0">
            <Icon name="document" class="text-tertiary shrink-0" />
            <div class="flex flex-col min-w-0 flex-1">
              <span class="font-mono text-xs text-primary truncate">{{ file?.name }}</span>
              <span class="text-xs text-tertiary">
                {{ $t('workspace-import-uploading', { size: formatFileSize(file?.size ?? 0), pct: uploadPct }) }}
              </span>
            </div>
          </div>
          <div class="w-full h-1.5 rounded-full bg-surface-hover overflow-hidden">
            <span class="block h-full bg-accent rounded-full transition-[width] duration-300" :style="{ width: `${uploadPct}%` }" />
          </div>
        </div>

        <!-- The archive the page holds -->
        <div
          v-else-if="shown"
          class="flex items-center gap-3 min-w-0 border border-default rounded-xl px-4 py-3 bg-surface"
          data-test="workspace-archive-file"
        >
          <Icon name="document" class="text-tertiary shrink-0" />
          <div class="flex flex-col min-w-0 flex-1">
            <span class="font-mono text-xs text-primary truncate">{{ shown.name }}</span>
            <span class="flex items-center gap-1 text-xs text-tertiary">
              {{ formatFileSize(shown.size) }}
              <template v-if="shown.sealed">
                · <Icon name="lock" size="xs" /> {{ $t('workspace-import-archive-sealed') }}
              </template>
            </span>
          </div>
          <Button
            v-if="!checking && !ready"
            variant="ghost"
            size="sm"
            :loading="discarding"
            data-test="workspace-choose-another"
            @click="emit('chooseAnother')"
          >
            {{ $t('workspace-import-choose-another') }}
          </Button>
        </div>

        <!-- Pick a file -->
        <template v-else>
          <button
            type="button"
            class="w-full flex flex-col items-center justify-center gap-3 border-2 border-dashed rounded-xl px-3 sm:px-4 py-7 sm:py-10 min-h-[176px] text-center transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            :class="[
              isDragOver ? 'border-accent bg-accent/10' : 'border-default bg-surface-alt/60 hover:border-strong hover:bg-surface-alt',
              workspaceFailed ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer',
            ]"
            :disabled="workspaceFailed"
            data-test="workspace-drop"
            @click="choose"
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
              <span class="text-sm text-primary font-medium">{{ $t('workspace-import-drop-idle') }}</span>
              <span class="text-xs text-tertiary">{{ $t('workspace-import-drop-hint', { max: formatFileSize(maxBytes) }) }}</span>
            </span>
          </button>
          <input
            ref="fileInput"
            type="file"
            accept=".nosdesk,application/octet-stream"
            class="hidden"
            tabindex="-1"
            aria-hidden="true"
            data-test="workspace-file"
            @change="picked"
          />
          <div class="max-w-sm">
            <FormInput
              v-model="password"
              type="password"
              size="sm"
              autocomplete="off"
              :label="$t('workspace-import-password-label')"
              :placeholder="$t('workspace-import-password-placeholder')"
              :description="$t('workspace-import-password-hint')"
              data-test="workspace-password"
            />
          </div>
        </template>
      </div>
    </SectionCard>

    <SectionCard content-padding="">
      <template #title>
        {{ $t('workspace-import-checks-heading') }}
        <span class="font-medium text-tertiary"> · {{ $t('workspace-import-checks-before') }}</span>
      </template>
      <ul class="flex flex-col divide-y divide-default" aria-live="polite" data-test="workspace-checks">
        <li
          v-for="line in lines"
          :key="line.row"
          class="flex items-start gap-3 px-3 py-2.5"
          :data-check="line.row"
          :data-state="line.state"
        >
          <span class="w-5 h-5 shrink-0 mt-px flex items-center justify-center" :class="ICON_CLASS[line.state]">
            <Spinner v-if="line.state === 'wait'" size="sm" />
            <Icon v-else :name="ICON[line.state]" size="sm" />
          </span>
          <div class="flex flex-col gap-1 min-w-0 flex-1">
            <p class="text-sm font-medium text-primary break-words">{{ words(line.title, line) }}</p>
            <p v-if="line.detail" class="text-sm text-secondary break-words">{{ words(line.detail, line) }}</p>
            <p v-if="line.refusal?.message" class="text-xs text-tertiary break-words" data-test="workspace-check-message">
              {{ line.refusal.message }}
            </p>
            <div
              v-if="line.refusal?.found?.length"
              class="grid grid-cols-[repeat(auto-fill,minmax(9rem,1fr))] gap-x-3 gap-y-1 text-sm text-primary mt-1"
              data-test="workspace-check-found"
            >
              <span v-for="f in line.refusal.found" :key="f.what">{{ foundWord(f) }}</span>
            </div>
            <p v-if="line.refusal?.found" class="text-sm text-secondary mt-1">{{ $t('workspace-import-check-not-empty-settings') }}</p>

            <!-- The fix -->
            <div v-if="line.refusal" class="flex flex-wrap items-end gap-2 mt-2" data-test="workspace-check-fix" :data-fix="line.refusal.fix">
              <template v-if="line.refusal.fix === 'password'">
                <div class="w-full max-w-xs">
                  <FormInput
                    v-model="password"
                    type="password"
                    size="sm"
                    autocomplete="off"
                    :label="$t('workspace-import-password-label')"
                    data-test="workspace-fix-password"
                    @keydown.enter="password && emit('check', {})"
                  />
                </div>
                <Button
                  variant="secondary"
                  size="sm"
                  :disabled="!password"
                  :loading="checking"
                  data-test="workspace-fix-check"
                  @click="emit('check', {})"
                >
                  {{ $t('workspace-import-fix-check') }}
                </Button>
              </template>
              <Button
                v-else-if="line.refusal.fix === 'check_again'"
                variant="secondary"
                size="sm"
                :loading="checking"
                data-test="workspace-fix-check-again"
                @click="emit('check', {})"
              >
                {{ $t('workspace-import-fix-check-again') }}
              </Button>
              <Button
                v-else-if="line.refusal.fix === 'new_ids'"
                variant="secondary"
                size="sm"
                :loading="checking"
                data-test="workspace-fix-new-ids"
                @click="emit('check', { regenerateUuids: true })"
              >
                {{ $t('workspace-import-fix-new-ids') }}
              </Button>
              <Button
                v-else-if="line.refusal.fix === 'follow'"
                variant="secondary"
                size="sm"
                data-test="workspace-fix-follow"
                @click="emit('follow', String(line.refusal.args.job))"
              >
                {{ $t('workspace-import-fix-follow') }}
              </Button>
              <Button
                v-else-if="line.refusal.fix === 'choose_another'"
                variant="secondary"
                size="sm"
                :loading="discarding"
                data-test="workspace-fix-choose-another"
                @click="emit('chooseAnother')"
              >
                {{ $t('workspace-import-choose-another') }}
              </Button>
              <span v-else-if="line.row === 'workspace' && line.refusal.found" class="text-sm text-tertiary">
                {{ $t('workspace-import-fix-not-empty') }}
              </span>
            </div>
          </div>
        </li>
      </ul>
      <p class="px-3 py-2 border-t border-default bg-surface-alt text-xs text-tertiary">
        {{ $t('workspace-import-checks-again') }}
      </p>
    </SectionCard>

    <div
      class="sticky bottom-0 z-10 -mx-4 sm:mx-0 px-4 sm:px-0 py-3 bg-app border-t border-default flex flex-wrap items-center gap-3"
      data-test="workspace-archive-actions"
    >
      <Button variant="ghost" data-test="workspace-cancel" @click="emit('cancel')">{{ $t('common-cancel') }}</Button>
      <span v-if="ready || !workspaceFailed" class="basis-full sm:basis-auto order-last sm:order-none text-xs text-tertiary">
        {{ ready ? $t('workspace-import-archive-next-note') : $t('workspace-import-archive-wait-note') }}
      </span>
      <Button
        class="ml-auto"
        :disabled="!ready || anyFailed"
        data-test="workspace-to-preview"
        @click="emit('next')"
      >
        {{ $t('workspace-import-continue-to-preview') }}
      </Button>
    </div>
  </section>
</template>
