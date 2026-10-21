<script setup lang="ts">
/**
 * Done, and a past import's page: the outcome, the counts, the staff who
 * can't sign in yet with Send invites, the next steps as a checklist (the
 * report is the to-do list), the report itself, Retry files when some
 * didn't restore, and Reset, the undo, behind a typed confirmation.
 *
 * Ticks on the checklist are kept in this browser, per import.
 */
import { computed, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { useRouter } from 'vue-router'

import Button from '@/components/common/Button.vue'
import Callout from '@/components/common/Callout.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import ErrorBanner from '@/components/common/ErrorBanner.vue'
import FormInput from '@/components/common/FormInput.vue'
import Icon from '@/components/common/Icon.vue'
import LinkButton from '@/components/common/LinkButton.vue'
import Modal from '@/components/Modal.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import ImportSignInLine from '@/components/import/ImportSignInLine.vue'
import { formatCompactDate } from '@nosdesk/core/utils/dateUtils'
import {
  workspaceImportService,
  type WorkspaceImportJob,
} from '@nosdesk/core/services/workspaceImportService'
import type { InvitesResult, SignInStatus } from '@nosdesk/core/services/importService'

import WorkspaceImportTiles from './WorkspaceImportTiles.vue'
import { archiveName, checklist, filesLine, filesPartial, resetUntil, tiles, typedMatches } from './workspaceImportModel'

const props = defineProps<{
  job: WorkspaceImportJob
  workspace: string
  retrying?: boolean
  /** On a past import's page, not the wizard. */
  detail?: boolean
}>()

const password = defineModel<string>('password', { default: '' })

const emit = defineEmits<{
  (e: 'retryFiles'): void
  (e: 'updated', job: WorkspaceImportJob): void
  (e: 'again'): void
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const router = useRouter()

const name = computed(() => archiveName(props.job))
const plan = computed(() => props.job.report?.plan)
const counts = computed(() => tiles(plan.value))
const files = computed(() => filesLine(props.job))
const partial = computed(() => filesPartial(props.job))
const failedEmpty = computed(() => props.job.state === 'failed' && !props.job.applied_at)
const wasReset = computed(() => props.job.state === 'reset')
const merged = computed(() => plan.value?.counts?.merged_into_runner ?? 0)

const when = (iso: string | null | undefined) => (iso ? formatCompactDate(iso) : '')
const duration = computed(() => {
  const from = props.job.confirmed_at ? Date.parse(props.job.confirmed_at) : NaN
  const to = props.job.finished_at ? Date.parse(props.job.finished_at) : NaN
  if (!Number.isFinite(from) || !Number.isFinite(to) || to < from) return ''
  const s = Math.round((to - from) / 1000)
  return s < 60 ? t('workspace-import-seconds', { s }) : t('workspace-import-minutes', { m: Math.floor(s / 60), s: s % 60 })
})

// ---- sign-in ----

const signIn = ref<SignInStatus | null>(null)
watch(
  () => props.job.sign_in,
  (s) => {
    signIn.value = s && s.pending > 0 ? { ...s, method: 'invite' } : null
  },
  { immediate: true },
)

async function sendInvites(): Promise<InvitesResult> {
  const result = await workspaceImportService.invite(props.job.id)
  return { sent: result.sent, failed: result.failed, sign_in: result.sign_in ? { ...result.sign_in, method: 'invite' } : null }
}

function onSignIn(status: SignInStatus | null) {
  signIn.value = status && status.pending > 0 ? status : null
}

// ---- checklist ----

const items = computed(() => checklist(props.job.report))
const storageKey = computed(() => `nosdesk.workspace-import.${props.job.id}.done`)
const ticked = ref<Set<string>>(new Set())
watch(
  storageKey,
  (key) => {
    try {
      const raw = localStorage.getItem(key)
      ticked.value = new Set(raw ? (JSON.parse(raw) as string[]) : [])
    } catch {
      ticked.value = new Set()
    }
  },
  { immediate: true },
)
function tick(id: string, on: boolean) {
  const next = new Set(ticked.value)
  if (on) next.add(id)
  else next.delete(id)
  ticked.value = next
  try {
    localStorage.setItem(storageKey.value, JSON.stringify([...next]))
  } catch {
    // Ticks are a convenience; without storage they last the visit.
  }
}
const doneCount = computed(() => items.value.filter((i) => ticked.value.has(i.id)).length)

// ---- report ----

const showReport = ref(false)
const reportJson = computed(() => JSON.stringify(props.job.report ?? {}, null, 2))
function downloadReport() {
  const blob = new Blob([reportJson.value], { type: 'application/json' })
  const link = document.createElement('a')
  link.href = URL.createObjectURL(blob)
  link.download = `workspace-import-${props.job.id}.json`
  document.body.appendChild(link)
  link.click()
  document.body.removeChild(link)
  URL.revokeObjectURL(link.href)
}

// ---- reset ----

const showReset = ref(false)
const typed = ref('')
const resetting = ref(false)
const resetError = ref('')
const canReset = computed(() => !props.job.applied_at || typedMatches(typed.value, props.workspace))
const until = computed(() => when(resetUntil(props.job)))

function openReset() {
  typed.value = ''
  resetError.value = ''
  showReset.value = true
}

async function reset() {
  if (!canReset.value || resetting.value) return
  resetting.value = true
  resetError.value = ''
  try {
    const updated = await workspaceImportService.reset(props.job.id)
    showReset.value = false
    emit('updated', updated)
  } catch (e) {
    showReset.value = false
    const body = (e as { response?: { data?: { error?: string; code?: string } } })?.response?.data
    resetError.value =
      body?.code === 'workspace_import.reset_window_closed'
        ? t('workspace-import-reset-closed')
        : body?.error || t('workspace-import-reset-failed')
    try {
      emit('updated', await workspaceImportService.get(props.job.id))
    } catch {
      // The page keeps the job as it was.
    }
  } finally {
    resetting.value = false
  }
}

function openWorkspace() {
  void router.push({ name: 'home' })
}
</script>

<template>
  <section class="flex flex-col gap-6 min-w-0" data-test="workspace-done" :data-state="job.state">
    <!-- After a reset -->
    <template v-if="wasReset">
      <Callout severity="info" data-test="workspace-done-reset">
        <template #header>
          <span class="font-medium text-primary">{{ $t('workspace-import-reset-done-title') }}</span>
        </template>
        <div class="flex flex-col gap-1.5 px-4 py-3">
          <p class="text-sm text-secondary">
            {{ job.applied_at
              ? $t('workspace-import-reset-done-detail', { workspace, date: when(job.applied_at), reset: when(job.reset_at) })
              : $t('workspace-import-reset-discarded', { date: when(job.reset_at) }) }}
          </p>
          <p class="text-xs text-tertiary">{{ $t('workspace-import-reset-done-again') }}</p>
        </div>
      </Callout>
      <div class="flex flex-wrap gap-2">
        <Button v-if="!detail" icon="move" data-test="workspace-import-again" @click="emit('again')">{{ $t('workspace-import-again') }}</Button>
        <LinkButton v-else :to="{ name: 'admin-workspace-import' }" icon="move">{{ $t('workspace-import-again') }}</LinkButton>
        <LinkButton variant="secondary" :to="{ name: 'admin-data-import' }">{{ $t('import-history-title') }}</LinkButton>
      </div>
    </template>

    <!-- The import failed and wrote nothing -->
    <template v-else-if="failedEmpty">
      <Callout severity="error" data-test="workspace-done-failed">
        <template #header>
          <span class="font-medium text-primary">{{ $t('workspace-import-failed-title') }}</span>
        </template>
        <div class="flex flex-col gap-1.5 px-4 py-3">
          <p class="text-sm text-secondary">{{ $t('workspace-import-failed-detail') }}</p>
          <p v-if="job.error?.message" class="text-xs text-tertiary break-words">{{ job.error.message }}</p>
        </div>
      </Callout>
    </template>

    <template v-else>
      <Callout v-if="partial" severity="warning" data-test="workspace-done-partial">
        <template #header>
          <span class="font-medium text-primary">{{ $t('workspace-import-partial-title', { count: files.missing }) }}</span>
        </template>
        <div class="flex flex-col gap-2 px-4 py-3">
          <p class="text-sm text-secondary">
            {{ $t('workspace-import-partial-detail', { name, workspace, restored: files.restored, total: files.total }) }}
          </p>
          <div v-if="job.actions.retry_files" class="flex flex-wrap items-end gap-2">
            <div v-if="job.archive.sealed" class="w-full max-w-xs">
              <FormInput
                v-model="password"
                type="password"
                size="sm"
                autocomplete="off"
                :label="$t('workspace-import-password-label')"
                data-test="workspace-retry-password"
              />
            </div>
            <Button
              variant="secondary"
              size="sm"
              :loading="retrying"
              :disabled="job.archive.sealed && !password"
              data-test="workspace-retry-files"
              @click="emit('retryFiles')"
            >
              {{ $t('workspace-import-retry-files') }}
            </Button>
          </div>
          <p v-else class="text-xs text-tertiary">{{ $t('workspace-import-retry-unavailable') }}</p>
        </div>
      </Callout>
      <Callout v-else severity="success" data-test="workspace-done-complete">
        <template #header>
          <span class="font-medium text-primary">{{ $t('workspace-import-done-title') }}</span>
        </template>
        <div class="flex flex-col gap-1.5 px-4 py-3">
          <p class="text-sm text-secondary">
            {{ $t('workspace-import-done-detail', { name, workspace, tickets: counts.tickets, pages: counts.pages, assets: counts.assets, people: counts.people }) }}
            <template v-if="duration"> {{ $t('workspace-import-done-took', { time: when(job.finished_at), duration }) }}</template>
          </p>
          <p class="text-xs text-tertiary">{{ $t('workspace-import-done-reloaded') }}</p>
          <p v-if="files.skipped" class="text-xs text-tertiary" data-test="workspace-files-skipped">
            {{ $t('workspace-import-files-skipped', { count: files.skipped }) }}
          </p>
        </div>
      </Callout>

      <WorkspaceImportTiles
        :tiles="[
          { label: $t('workspace-import-tile-tickets'), value: counts.tickets, sub: $t('workspace-import-n-comments', { count: counts.comments }) },
          { label: $t('workspace-import-tile-pages'), value: counts.pages, sub: $t('workspace-import-n-collections', { count: counts.collections }) },
          { label: $t('workspace-import-tile-files'), value: files.restored, sub: partial ? $t('workspace-import-tile-files-missing', { count: files.missing }) : files.skipped ? $t('workspace-import-tile-files-skipped', { count: files.skipped }) : $t('workspace-import-tile-files-all') },
          { label: $t('workspace-import-tile-people'), value: counts.people, sub: merged ? $t('workspace-import-tile-people-merged') : '' },
        ]"
      />

      <div class="flex flex-wrap gap-2">
        <Button v-if="!detail" data-test="workspace-open" @click="openWorkspace">{{ $t('workspace-import-open-workspace', { workspace }) }}</Button>
        <Button variant="secondary" data-test="workspace-report" @click="showReport = true">{{ $t('workspace-import-report') }}</Button>
        <LinkButton v-if="!detail" variant="secondary" :to="{ name: 'admin-data-import' }">{{ $t('import-history-title') }}</LinkButton>
      </div>

      <ImportSignInLine v-if="signIn" :job-id="job.id" :status="signIn" :send-invites="sendInvites" @updated="onSignIn" />
      <p v-if="signIn && !signIn.email_ready" class="-mt-4 px-1 text-xs text-tertiary" data-test="workspace-signin-cli">
        {{ $t('workspace-import-invites-cli') }}
      </p>

      <SectionCard v-if="!job.pruned_at" content-padding="" data-test="workspace-checklist">
        <template #title>
          {{ $t('workspace-import-todo-heading') }}
          <span class="font-medium text-tertiary"> · {{ $t('workspace-import-todo-count', { done: doneCount, total: items.length }) }}</span>
        </template>
        <ul class="flex flex-col divide-y divide-default">
          <li
            v-for="item in items"
            :key="item.id"
            class="flex flex-wrap sm:flex-nowrap items-start gap-x-3 gap-y-2 px-3 py-2.5"
            :data-item="item.id"
          >
            <input
              :id="`workspace-todo-${item.id}`"
              type="checkbox"
              class="mt-0.5 w-4 h-4 shrink-0 accent-[var(--color-accent)]"
              :checked="ticked.has(item.id)"
              @change="tick(item.id, ($event.target as HTMLInputElement).checked)"
            />
            <label :for="`workspace-todo-${item.id}`" class="flex flex-col gap-0.5 min-w-0 flex-1 basis-[calc(100%-2rem)] sm:basis-auto">
              <span class="text-sm font-medium break-words" :class="ticked.has(item.id) ? 'line-through text-tertiary font-normal' : 'text-primary'">
                {{ $t(item.title, item.args) }}
              </span>
              <span class="text-sm text-secondary break-words">{{ $t(item.detail, item.args) }}</span>
            </label>
            <LinkButton variant="secondary" size="sm" class="ml-7 sm:ml-0 shrink-0" :to="item.to">{{ $t(item.go) }}</LinkButton>
          </li>
        </ul>
        <p class="px-3 py-2 border-t border-default bg-surface-alt text-xs text-tertiary">{{ $t('workspace-import-todo-foot') }}</p>
      </SectionCard>
    </template>

    <ErrorBanner v-if="resetError" :message="resetError" data-test="workspace-reset-error" />

    <!-- Reset -->
    <div v-if="job.actions.reset" class="flex flex-wrap items-center gap-x-3 gap-y-1" data-test="workspace-reset">
      <Button variant="ghost-danger" data-test="workspace-reset-button" @click="openReset">
        {{ job.applied_at ? $t('workspace-import-reset-button') : $t('workspace-import-discard-button') }}
      </Button>
      <span class="text-xs text-tertiary flex-1 min-w-[12rem]">
        {{ job.applied_at ? $t('workspace-import-reset-rule', { date: until }) : $t('workspace-import-discard-rule') }}
      </span>
    </div>
    <p v-else-if="job.applied_at && job.pruned_at && !wasReset" class="text-sm text-tertiary" data-test="workspace-reset-closed">
      {{ $t('workspace-import-reset-closed') }}
    </p>

    <ConfirmModal
      :show="showReset"
      variant="danger"
      :title="job.applied_at ? $t('workspace-import-reset-title', { workspace }) : $t('workspace-import-discard-title')"
      :description="job.applied_at ? $t('workspace-import-reset-subtitle', { date: when(job.applied_at) }) : undefined"
      message=""
      :confirm-label="job.applied_at ? $t('workspace-import-reset-confirm') : $t('workspace-import-discard-confirm')"
      :confirm-disabled="!canReset"
      :loading="resetting"
      @close="showReset = false"
      @confirm="reset"
    >
      <template #body>
        <ul v-if="job.applied_at" class="flex flex-col gap-2 text-sm text-secondary" data-test="workspace-reset-stakes">
          <li class="flex gap-2.5"><Icon name="warning" size="sm" class="shrink-0 mt-0.5 text-status-error" />
            <span>{{ $t('workspace-import-reset-removes', { tickets: counts.tickets, pages: counts.pages, assets: counts.assets, people: job.people_created }) }}</span></li>
          <li class="flex gap-2.5"><Icon name="warning" size="sm" class="shrink-0 mt-0.5 text-status-error" />
            <span>{{ $t('workspace-import-reset-since') }}</span></li>
          <li class="flex gap-2.5"><Icon name="info" size="sm" class="shrink-0 mt-0.5 text-tertiary" />
            <span>{{ $t('workspace-import-reset-keeps') }}</span></li>
          <li class="flex gap-2.5"><Icon name="info" size="sm" class="shrink-0 mt-0.5 text-tertiary" />
            <span>{{ $t('workspace-import-reset-archive') }}</span></li>
          <li class="flex gap-2.5"><Icon name="info" size="sm" class="shrink-0 mt-0.5 text-tertiary" />
            <span>{{ $t('workspace-import-reset-final') }}</span></li>
        </ul>
        <p v-else class="text-sm text-secondary">{{ $t('workspace-import-discard-detail') }}</p>
        <div v-if="job.applied_at" class="flex flex-col gap-1.5">
          <label for="workspace-reset-name" class="text-xs font-medium text-secondary">
            {{ $t('workspace-import-type-to-confirm', { workspace }) }}
          </label>
          <input
            id="workspace-reset-name"
            v-model="typed"
            type="text"
            autocomplete="off"
            autocapitalize="off"
            spellcheck="false"
            :placeholder="workspace"
            class="w-full px-3 py-2 text-sm rounded-lg border border-strong bg-surface text-primary focus:outline-none focus:border-accent focus:ring-2 focus:ring-accent/20"
            data-test="workspace-reset-name"
            @keydown.enter="reset"
          />
        </div>
      </template>
    </ConfirmModal>

    <Modal :show="showReport" :title="$t('workspace-import-report-title')" :description="$t('workspace-import-report-subtitle')" size="lg" @close="showReport = false">
      <pre class="max-h-[50vh] overflow-auto rounded-lg border border-default bg-surface-alt p-3 font-mono text-xs text-primary whitespace-pre" data-test="workspace-report-json">{{ reportJson }}</pre>
      <template #footer>
        <div class="modal-actions">
          <Button variant="ghost" @click="showReport = false">{{ $t('workspace-import-report-close') }}</Button>
          <Button variant="secondary" icon="download" @click="downloadReport">{{ $t('workspace-import-report-download') }}</Button>
        </div>
      </template>
    </Modal>
  </section>
</template>
