<script setup lang="ts">
/**
 * The hub's Recent imports card: the newest jobs that reached Map, each
 * with when and who, the file and type, its state (Draft, Imported,
 * Expired, or Removed for documentation whose pages went to Trash), what it
 * did or where it stopped, and Resume or View. On a
 * phone each job is a two-line card and the counts line carries the state.
 *
 * With `workspaceImports` (the owner, self-hosted), workspace imports join
 * the list by date: Resume while one is on its way, View once it ran.
 */
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import { useQuery } from '@pinia/colada'

import LinkButton from '@/components/common/LinkButton.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import StatusPill from '@/components/common/StatusPill.vue'
import { formatCompactDate } from '@nosdesk/core/utils/dateUtils'
import {
  importJobsQueryKey,
  importService,
  type ImportJobListItem,
} from '@nosdesk/core/services/importService'

import {
  workspaceImportService,
  workspaceImportsQueryKey,
  type WorkspaceImportJob,
} from '@nosdesk/core/services/workspaceImportService'

import {
  countsLine as workspaceCounts,
  detailTarget,
  rowAction as workspaceAction,
  statePill as workspacePill,
  wizardTarget,
} from './workspaceImport/workspaceImportModel'
import {
  RECENT_LIMIT,
  countsLine,
  detailPath,
  fileLabel,
  resumeTarget,
  rowAction,
  statePill,
} from './historyModel'

const props = defineProps<{
  /** List workspace imports too. */
  workspaceImports?: boolean
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const query = useQuery({
  key: () => importJobsQueryKey(RECENT_LIMIT),
  query: () => importService.list(RECENT_LIMIT),
})

const workspaceQuery = useQuery({
  key: () => [...workspaceImportsQueryKey],
  query: () => workspaceImportService.list(),
  enabled: () => !!props.workspaceImports,
})

type Row =
  | { kind: 'file'; id: string; at: string; item: ImportJobListItem }
  | { kind: 'workspace'; id: string; at: string; job: WorkspaceImportJob }

/** Both kinds, newest first. A workspace list that fails leaves the rest. */
const rows = computed<Row[] | null>(() => {
  const files = query.data.value
  if (!files) return null
  const out: Row[] = files.map((item) => ({ kind: 'file', id: item.id, at: item.created_at, item }))
  if (props.workspaceImports) {
    for (const job of workspaceQuery.data.value ?? []) {
      out.push({ kind: 'workspace', id: job.id, at: job.created_at, job })
    }
    out.sort((a, b) => Date.parse(b.at) - Date.parse(a.at))
  }
  return out.slice(0, RECENT_LIMIT)
})

const counts = (item: ImportJobListItem) => countsLine(t, item)
const archiveLabel = (job: WorkspaceImportJob) => job.archive.name || job.report?.source?.workspace_slug || ''
</script>

<template>
  <SectionCard content-padding="" data-test="hub-recent-imports">
    <template #title>{{ $t('import-history-title') }}</template>
    <p v-if="query.error.value" class="px-3 py-3 text-sm text-secondary">
      {{ $t('import-history-load-failed') }}
    </p>
    <p
      v-else-if="rows && rows.length === 0"
      class="px-3 py-3 text-sm text-secondary"
      data-test="recent-imports-empty"
    >
      {{ $t('import-history-empty') }}
    </p>
    <ul v-else-if="rows" class="flex flex-col divide-y divide-default">
      <template v-for="row in rows" :key="`${row.kind}:${row.id}`">
      <li
        v-if="row.kind === 'workspace'"
        class="grid grid-cols-[minmax(0,1fr)_auto] md:grid-cols-[8rem_minmax(0,1.4fr)_6rem_minmax(0,1.2fr)_7rem] items-center gap-x-3 gap-y-1 px-3 py-2.5 text-sm"
        :data-state="row.job.state"
        data-test="recent-workspace-import"
      >
        <span class="col-span-2 md:col-span-1 min-w-0 text-xs md:text-sm text-tertiary md:text-primary truncate">
          {{ formatCompactDate(row.job.created_at) }}
        </span>
        <span class="min-w-0 truncate" :title="archiveLabel(row.job)">
          <span class="font-mono text-xs text-primary">{{ archiveLabel(row.job) }}</span>
          <span class="text-xs text-tertiary"> · {{ $t('workspace-import-type') }}</span>
        </span>
        <span class="hidden md:block">
          <StatusPill :label="$t(workspacePill(row.job).key)" :tone="workspacePill(row.job).tone" />
        </span>
        <span class="order-last md:order-none col-span-2 md:col-span-1 min-w-0 text-xs-plus text-secondary truncate">
          {{ workspaceCounts(t, row.job) }}
        </span>
        <span class="justify-self-end">
          <LinkButton
            variant="secondary"
            size="sm"
            :to="workspaceAction(row.job) === 'resume' ? wizardTarget(row.job.id) : detailTarget(row.job.id)"
            data-test="recent-workspace-import-action"
          >
            {{ workspaceAction(row.job) === 'resume' ? $t('import-history-resume') : $t('import-history-view') }}
          </LinkButton>
        </span>
      </li>
      <li
        v-else
        class="grid grid-cols-[minmax(0,1fr)_auto] md:grid-cols-[8rem_minmax(0,1.4fr)_6rem_minmax(0,1.2fr)_7rem] items-center gap-x-3 gap-y-1 px-3 py-2.5 text-sm"
        :data-state="row.item.state ?? undefined"
        data-test="recent-import"
      >
        <span class="col-span-2 md:col-span-1 min-w-0 text-xs md:text-sm text-tertiary md:text-primary truncate">
          {{ formatCompactDate(row.item.created_at) }}
          <span v-if="row.item.created_by_name" class="text-xs text-tertiary">· {{ row.item.created_by_name }}</span>
        </span>
        <span class="min-w-0 truncate" :title="fileLabel(row.item)">
          <span class="font-mono text-xs text-primary">{{ fileLabel(row.item) }}</span>
          <span class="text-xs text-tertiary"> · {{ $t('import-matches-nouns', { type: row.item.job_type }) }}</span>
        </span>
        <span class="hidden md:block">
          <StatusPill
            v-if="statePill(row.item)"
            :label="$t(statePill(row.item)!.key)"
            :tone="statePill(row.item)!.tone"
            data-test="recent-import-state"
          />
        </span>
        <span class="order-last md:order-none col-span-2 md:col-span-1 min-w-0 text-xs-plus text-secondary truncate" data-test="recent-import-counts">
          {{ counts(row.item) }}
        </span>
        <span class="justify-self-end">
          <LinkButton
            v-if="rowAction(row.item) === 'resume'"
            variant="secondary"
            size="sm"
            :to="resumeTarget(row.item.id, row.item.job_type)"
            data-test="recent-import-resume"
          >
            {{ $t('import-history-resume') }}
          </LinkButton>
          <LinkButton
            v-else-if="rowAction(row.item) === 'view'"
            variant="secondary"
            size="sm"
            :to="detailPath(row.item.id)"
            data-test="recent-import-view"
          >
            {{ $t('import-history-view') }}
          </LinkButton>
          <span v-else class="text-xs text-tertiary whitespace-nowrap">{{ $t('import-history-cannot-resume') }}</span>
        </span>
      </li>
      </template>
    </ul>
  </SectionCard>
</template>
