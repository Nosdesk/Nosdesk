<script setup lang="ts">
/**
 * Import a Nosdesk export: Archive, Preview, Import.
 *
 *  - Archive : choose the export (and its password); the upload streams with
 *              its progress, then the server checks it in the background
 *              (`checking`). The three checks fill in as they run; a refusal
 *              fails its row with the reason and the one action that fixes
 *              it, and the job waits in `uploaded`.
 *  - Preview : the plan's report: what comes across, the people, how they
 *              sign in, what this workspace gives up, what arrives switched
 *              off, what doesn't move, what needs attention. Import asks for
 *              the workspace's name, then confirms.
 *  - Import  : follows the job through its stages to Done, with the next
 *              steps, the invitations, Retry files and Reset.
 *
 * Nothing in the workspace changes before Import. The job rides in `?job=`,
 * so a reload or a link resumes it, a check or an import still running
 * included; with no `?job=`, a live import resumes.
 *
 * Owner only, self-hosted only: on Nosdesk Cloud, Nosdesk support runs a
 * move on request, and the page says how to reach them.
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { useRoute, useRouter } from 'vue-router'
import { useQueryCache } from '@pinia/colada'

import BackButton from '@/components/common/BackButton.vue'
import ErrorBanner from '@/components/common/ErrorBanner.vue'
import Icon from '@/components/common/Icon.vue'
import Spinner from '@/components/common/Spinner.vue'
import WorkspaceArchiveStep from '@/components/import/workspaceImport/WorkspaceArchiveStep.vue'
import WorkspaceImportDone from '@/components/import/workspaceImport/WorkspaceImportDone.vue'
import WorkspaceImportProgress from '@/components/import/workspaceImport/WorkspaceImportProgress.vue'
import WorkspacePreviewStep from '@/components/import/workspaceImport/WorkspacePreviewStep.vue'
import {
  DEFAULT_MAX_BYTES,
  STEPS,
  isLive,
  isRunning,
  stepFor,
  type WizardStep,
} from '@/components/import/workspaceImport/workspaceImportModel'
import { errorBody } from '@/components/import/refusals'
import { useAuthStore } from '@/stores/auth'
import { useWorkspaceName } from '@/components/import/workspaceImport/useWorkspaceName'
import SupportAddress from '@/components/SupportAddress.vue'
import { getServerVersion, isHostedDeploymentRef } from '@nosdesk/core/services/instanceConfig'
import {
  workspaceImportService,
  workspaceImportQueryKey,
  workspaceImportsQueryKey,
  type WorkspaceImportJob,
  type WorkspaceImportRefusal,
} from '@nosdesk/core/services/workspaceImportService'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const route = useRoute()
const router = useRouter()
const queryCache = useQueryCache()
const auth = useAuthStore()
const workspaceName = useWorkspaceName()

const STEP_KEYS: Record<WizardStep, string> = {
  archive: 'workspace-import-step-archive',
  preview: 'workspace-import-step-preview',
  import: 'workspace-import-step-import',
}

const hosted = computed(() => isHostedDeploymentRef.value)
const allowed = computed(() => auth.isOwner && !hosted.value)
const serverVersion = computed(() => getServerVersion())

const job = ref<WorkspaceImportJob | null>(null)
const step = ref<WizardStep>('archive')
const busy = ref<'upload' | 'check' | 'confirm' | 'resume' | 'retry' | 'discard' | null>(null)
const errorMessage = ref('')
/** A refusal a request came back with before any job held it. */
const refused = ref<WorkspaceImportRefusal | null>(null)
const file = ref<{ name: string; size: number } | null>(null)
const uploaded = ref({ loaded: 0, total: 0 })
/** The archive's password, kept for confirm and retry files; never stored. */
const password = ref('')

const stepIndex = computed(() => STEPS.indexOf(step.value))
const locked = computed(() => !!job.value && ['importing', 'imported', 'failed', 'reset'].includes(job.value.state))

// ---- polling ----

let alive = true
onBeforeUnmount(() => {
  alive = false
})
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

/** Read the job until its background run ends. */
async function follow(id: string): Promise<WorkspaceImportJob | null> {
  for (let i = 0; i < 3600 && alive; i++) {
    await sleep(job.value?.state === 'importing' ? 2000 : 1500)
    if (!alive || job.value?.id !== id) return null
    let found: WorkspaceImportJob
    try {
      found = await workspaceImportService.get(id)
    } catch {
      continue
    }
    if (job.value?.id !== id) return null
    job.value = found
    if (!isRunning(found)) return found
  }
  return null
}

async function settle(found: WorkspaceImportJob) {
  queryCache.invalidateQueries({ key: workspaceImportsQueryKey })
  queryCache.invalidateQueries({ key: workspaceImportQueryKey(found.id) })
  const before = step.value
  step.value = stepFor(found)
  if (found.state === 'previewed' && found.error) {
    // The import read the archive again and stopped; nothing was written.
    errorMessage.value = found.error.message
  }
  if (step.value !== before) window.scrollTo({ top: 0 })
}

async function track(found: WorkspaceImportJob) {
  job.value = found
  setJobQuery(found.id)
  step.value = stepFor(found)
  if (isRunning(found)) {
    const done = await follow(found.id)
    if (done) await settle(done)
  } else {
    await settle(found)
  }
}

// ---- route ----

function setJobQuery(id: string | null) {
  if ((route.query.job ?? null) === id) return
  const query = { ...route.query }
  if (id) query.job = id
  else delete query.job
  void router.replace({ query })
}

async function resume() {
  if (!allowed.value) return
  busy.value = 'resume'
  try {
    const id = typeof route.query.job === 'string' ? route.query.job : ''
    const found = id ? await workspaceImportService.get(id) : await workspaceImportService.latest()
    busy.value = null
    // With no job named, only a live import resumes; a finished one stays on
    // its page and Recent imports.
    if (found && (id || isLive(found))) await track(found)
  } catch {
    job.value = null
    step.value = 'archive'
    errorMessage.value = t('workspace-import-resume-failed')
    setJobQuery(null)
  } finally {
    if (busy.value === 'resume') busy.value = null
  }
}

onMounted(() => {
  void resume()
})

watch(allowed, (now, before) => {
  if (now && !before) void resume()
})

// ---- archive ----

const maxBytes = DEFAULT_MAX_BYTES

function codeOf(e: unknown): WorkspaceImportRefusal | null {
  const body = errorBody(e) as { code?: string; error?: string; args?: Record<string, unknown> } | undefined
  if (!body?.code) return null
  return { code: body.code, message: body.error ?? '', args: body.args }
}

async function upload(chosen: File) {
  if (busy.value) return
  errorMessage.value = ''
  refused.value = null
  file.value = { name: chosen.name, size: chosen.size }
  if (chosen.size > maxBytes) {
    refused.value = { code: 'upload_too_large', message: '', args: { max: maxBytes } }
    return
  }
  busy.value = 'upload'
  uploaded.value = { loaded: 0, total: chosen.size }
  try {
    const found = await workspaceImportService.upload(chosen, { password: password.value || undefined }, (loaded, total) => {
      uploaded.value = { loaded, total: total || chosen.size }
    })
    queryCache.invalidateQueries({ key: workspaceImportsQueryKey })
    busy.value = null
    await track(found)
  } catch (e) {
    const r = codeOf(e)
    if (r) refused.value = r
    else errorMessage.value = t('workspace-import-upload-failed')
  } finally {
    if (busy.value === 'upload') busy.value = null
  }
}

/** Check the archive again: with its password, or giving every row a new
 *  id. */
async function check(options: { regenerateUuids?: boolean } = {}) {
  if (!job.value || busy.value) return
  busy.value = 'check'
  errorMessage.value = ''
  const id = job.value.id
  try {
    const found = await workspaceImportService.check(id, {
      password: password.value || undefined,
      regenerate_uuids: options.regenerateUuids,
    })
    busy.value = null
    await track(found)
  } catch (e) {
    const r = codeOf(e)
    if (r && r.code !== 'workspace_import.wrong_state') refused.value = r
    else errorMessage.value = r?.message || t('workspace-import-check-request-failed')
  } finally {
    if (busy.value === 'check') busy.value = null
  }
}

/** Put the archive aside and start again from the file picker. A job that
 *  imported nothing is discarded. */
async function chooseAnother() {
  if (busy.value) return
  const current = job.value
  if (current && current.actions.reset && !current.applied_at) {
    busy.value = 'discard'
    try {
      await workspaceImportService.reset(current.id)
    } catch {
      // A discard that failed leaves a parked upload; the prune removes it.
    } finally {
      busy.value = null
    }
    queryCache.invalidateQueries({ key: workspaceImportsQueryKey })
  }
  job.value = null
  refused.value = null
  file.value = null
  errorMessage.value = ''
  password.value = ''
  step.value = 'archive'
  setJobQuery(null)
}

async function followLive(id: string) {
  refused.value = null
  file.value = null
  busy.value = 'resume'
  try {
    const found = await workspaceImportService.get(id)
    busy.value = null
    await track(found)
  } catch {
    errorMessage.value = t('workspace-import-resume-failed')
  } finally {
    if (busy.value === 'resume') busy.value = null
  }
}

/** Leave for the hub. A parked upload stays as a draft in Recent imports,
 *  to resume or discard later. */
function cancel() {
  void router.push({ name: 'admin-data-import' })
}

// ---- preview ----

async function confirm() {
  if (!job.value || busy.value) return
  busy.value = 'confirm'
  errorMessage.value = ''
  const id = job.value.id
  try {
    const found = await workspaceImportService.confirm(id, { password: password.value || undefined })
    busy.value = null
    window.scrollTo({ top: 0 })
    await track(found)
  } catch (e) {
    const r = codeOf(e)
    errorMessage.value = r?.message || t('workspace-import-confirm-failed')
  } finally {
    if (busy.value === 'confirm') busy.value = null
  }
}

// ---- import ----

async function retryFiles() {
  if (!job.value || busy.value) return
  busy.value = 'retry'
  errorMessage.value = ''
  try {
    const found = await workspaceImportService.retryFiles(job.value.id, { password: password.value || undefined })
    busy.value = null
    await track(found)
  } catch (e) {
    errorMessage.value = codeOf(e)?.message || t('workspace-import-retry-failed')
  } finally {
    if (busy.value === 'retry') busy.value = null
  }
}

function onUpdated(updated: WorkspaceImportJob) {
  job.value = updated
  queryCache.invalidateQueries({ key: workspaceImportsQueryKey })
  queryCache.invalidateQueries({ key: workspaceImportQueryKey(updated.id) })
}

function importAgain() {
  job.value = null
  refused.value = null
  file.value = null
  password.value = ''
  errorMessage.value = ''
  step.value = 'archive'
  setJobQuery(null)
}

// ---- stepper ----

function canGoTo(target: WizardStep): boolean {
  if (busy.value || locked.value) return false
  const i = STEPS.indexOf(target)
  if (i >= stepIndex.value) return false
  return target === 'archive' || (target === 'preview' && job.value?.state === 'previewed')
}

function goTo(target: WizardStep) {
  if (!canGoTo(target)) return
  errorMessage.value = ''
  step.value = target
}

function toPreview() {
  if (job.value?.state === 'previewed') {
    errorMessage.value = ''
    step.value = 'preview'
    window.scrollTo({ top: 0 })
  }
}
</script>

<template>
  <div class="flex-1 flex flex-col">
    <div class="pt-4 px-4 sm:px-6 flex justify-between items-center">
      <BackButton fallbackRoute="/admin/data-import" :label="$t('csv-import-back')" />
    </div>

    <div class="flex-1 flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-4xl min-w-0" data-test="workspace-import">
      <div class="flex flex-col gap-2">
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ $t('workspace-import-title') }}</h1>
        <p class="text-sm sm:text-base text-secondary">
          {{ $t('workspace-import-subtitle', { workspace: workspaceName }) }}
        </p>
      </div>

      <!-- Cloud: Nosdesk support runs moves in. -->
      <div
        v-if="hosted"
        class="rounded-xl border border-default bg-surface px-4 py-4 flex flex-col gap-3"
        data-test="workspace-import-cloud"
      >
        <p class="text-sm text-secondary">{{ $t('workspace-import-cloud-elsewhere') }}</p>
        <SupportAddress />
      </div>

      <p
        v-else-if="!auth.isOwner"
        class="rounded-xl border border-default bg-surface px-4 py-4 text-sm text-secondary"
        data-test="workspace-import-owner-only"
      >
        {{ $t('workspace-import-owner-only') }}
      </p>

      <template v-else>
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
          <p v-if="step !== 'import'" class="flex items-start gap-2 text-xs-plus text-tertiary" data-test="workspace-import-nothing-changes">
            <Icon name="info" size="xs" class="mt-[3px] shrink-0" />
            <span>{{ $t('workspace-import-nothing-changes') }}</span>
          </p>
        </nav>

        <ErrorBanner v-if="errorMessage" :message="errorMessage" data-test="workspace-import-error" />

        <p v-if="busy === 'resume'" class="flex items-center gap-2 text-sm text-secondary">
          <Spinner size="sm" />
          {{ $t('import-resuming') }}
        </p>

        <WorkspaceArchiveStep
          v-else-if="step === 'archive'"
          v-model:password="password"
          :job="job"
          :file="file"
          :uploading="busy === 'upload'"
          :uploaded="uploaded"
          :checking="busy === 'check'"
          :discarding="busy === 'discard'"
          :refused="refused"
          :max-bytes="maxBytes"
          :server-version="serverVersion"
          :workspace="workspaceName"
          @upload="upload"
          @check="check"
          @choose-another="chooseAnother"
          @follow="followLive"
          @cancel="cancel"
          @next="toPreview"
        />

        <WorkspacePreviewStep
          v-else-if="step === 'preview' && job"
          v-model:password="password"
          :job="job"
          :workspace="workspaceName"
          :confirming="busy === 'confirm'"
          @back="goTo('archive')"
          @confirm="confirm"
        />

        <template v-else-if="step === 'import' && job">
          <WorkspaceImportProgress v-if="job.state === 'importing'" :job="job" />
          <WorkspaceImportDone
            v-else
            v-model:password="password"
            :job="job"
            :workspace="workspaceName"
            :retrying="busy === 'retry'"
            @retry-files="retryFiles"
            @updated="onUpdated"
            @again="importAgain"
          />
        </template>
      </template>
    </div>
  </div>
</template>
