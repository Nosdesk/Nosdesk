<script setup lang="ts">
/**
 * A past workspace import: when and from where, its state, then Done's
 * cards (the outcome, the counts, the next steps, the report, Retry files
 * and Reset). An import still on its way opens in the wizard instead.
 *
 * Thirty days on, the report keeps its counts but not its people, and reset
 * is no longer offered; the page says so.
 */
import { computed, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { useRoute, useRouter } from 'vue-router'
import { useQuery, useQueryCache } from '@pinia/colada'

import BackButton from '@/components/common/BackButton.vue'
import ErrorBanner from '@/components/common/ErrorBanner.vue'
import Spinner from '@/components/common/Spinner.vue'
import StatusPill from '@/components/common/StatusPill.vue'
import WorkspaceImportDone from '@/components/import/workspaceImport/WorkspaceImportDone.vue'
import { useWorkspaceName } from '@/components/import/workspaceImport/useWorkspaceName'
import {
  archiveName,
  hostOf,
  isLive,
  statePill,
  wizardTarget,
} from '@/components/import/workspaceImport/workspaceImportModel'
import { formatCompactDate } from '@nosdesk/core/utils/dateUtils'
import { formatFileSize } from '@nosdesk/core/utils/formatFileSize'
import {
  workspaceImportQueryKey,
  workspaceImportService,
  workspaceImportsQueryKey,
  type WorkspaceImportJob,
} from '@nosdesk/core/services/workspaceImportService'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const route = useRoute()
const router = useRouter()
const queryCache = useQueryCache()
const workspaceName = useWorkspaceName()
const id = computed(() => String(route.params.id ?? ''))

const query = useQuery({
  key: () => workspaceImportQueryKey(id.value),
  query: () => workspaceImportService.get(id.value),
  enabled: () => !!id.value,
})
const job = computed(() => query.data.value)

watch(job, (j) => {
  if (j && isLive(j)) void router.replace(wizardTarget(j.id))
})

const pill = computed(() => (job.value ? statePill(job.value) : null))

const meta = computed(() => {
  const j = job.value
  if (!j) return ''
  const parts: string[] = [formatCompactDate(j.created_at)]
  if (j.archive.name) parts.push(j.archive.name)
  if (j.archive.size) parts.push(formatFileSize(j.archive.size))
  const source = j.report?.source
  if (source) {
    const origin = hostOf(source.origin)
    parts.push(
      t(origin ? 'workspace-import-detail-source' : 'workspace-import-detail-source-no-origin', {
        name: archiveName(j),
        mode: source.mode,
        version: source.nosdesk_version,
        origin,
      }),
    )
  }
  return parts.join(' · ')
})

const password = ref('')
const retrying = ref(false)
const errorMessage = ref('')

/** Retry files starts here and is followed in the wizard. */
async function retryFiles() {
  if (!job.value || retrying.value) return
  retrying.value = true
  errorMessage.value = ''
  try {
    await workspaceImportService.retryFiles(job.value.id, { password: password.value || undefined })
    queryCache.invalidateQueries({ key: workspaceImportsQueryKey })
    void router.replace(wizardTarget(job.value.id))
  } catch (e) {
    const body = (e as { response?: { data?: { error?: string } } })?.response?.data
    errorMessage.value = body?.error || t('workspace-import-retry-failed')
  } finally {
    retrying.value = false
  }
}

function onUpdated(updated: WorkspaceImportJob) {
  queryCache.setQueryData(workspaceImportQueryKey(updated.id), updated)
  queryCache.invalidateQueries({ key: workspaceImportsQueryKey })
}
</script>

<template>
  <div class="flex-1">
    <div class="pt-4 px-4 sm:px-6 flex justify-between items-center">
      <BackButton fallbackRoute="/admin/data-import" :label="$t('csv-import-back')" />
    </div>

    <div class="flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-4xl min-w-0" data-test="workspace-import-detail">
      <ErrorBanner v-if="query.error.value && !job" :message="$t('workspace-import-detail-load-failed')" />
      <p v-else-if="!job" class="flex items-center gap-2 text-sm text-secondary">
        <Spinner size="sm" />
        {{ $t('import-resuming') }}
      </p>
      <template v-else>
        <div class="flex flex-col gap-2">
          <div class="flex flex-wrap items-center gap-2">
            <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ $t('workspace-import-detail-title') }}</h1>
            <StatusPill v-if="pill" :label="$t(pill.key)" :tone="pill.tone" data-test="workspace-detail-state" />
          </div>
          <p class="text-sm text-secondary break-words" data-test="workspace-detail-meta">{{ meta }}</p>
          <p v-if="job.pruned_at" class="text-xs text-tertiary">{{ $t('workspace-import-detail-pruned') }}</p>
        </div>

        <ErrorBanner v-if="errorMessage" :message="errorMessage" />
        <WorkspaceImportDone
          v-model:password="password"
          :job="job"
          :workspace="workspaceName"
          :retrying="retrying"
          detail
          @retry-files="retryFiles"
          @updated="onUpdated"
        />
      </template>
    </div>
  </div>
</template>
