<script setup lang="ts">
/**
 * The hub's Recent imports card: the newest jobs that reached Map, each
 * with when and who, the file and type, its state (Draft, Imported,
 * Expired), what it did or where it stopped, and Resume or View. On a
 * phone each job is a two-line card and the counts line carries the state.
 */
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
  RECENT_LIMIT,
  STATE_KEY,
  STATE_TONE,
  countsLine,
  detailPath,
  fileLabel,
  resumeTarget,
  rowAction,
} from './historyModel'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const query = useQuery({
  key: () => importJobsQueryKey(RECENT_LIMIT),
  query: () => importService.list(RECENT_LIMIT),
})

const counts = (item: ImportJobListItem) => countsLine(t, item)
</script>

<template>
  <SectionCard content-padding="" data-test="hub-recent-imports">
    <template #title>{{ $t('import-history-title') }}</template>
    <p v-if="query.error.value" class="px-3 py-3 text-sm text-secondary">
      {{ $t('import-history-load-failed') }}
    </p>
    <p
      v-else-if="query.data.value && query.data.value.length === 0"
      class="px-3 py-3 text-sm text-secondary"
      data-test="recent-imports-empty"
    >
      {{ $t('import-history-empty') }}
    </p>
    <ul v-else-if="query.data.value" class="flex flex-col divide-y divide-default">
      <li
        v-for="item in query.data.value"
        :key="item.id"
        class="grid grid-cols-[minmax(0,1fr)_auto] md:grid-cols-[8rem_minmax(0,1.4fr)_6rem_minmax(0,1.2fr)_7rem] items-center gap-x-3 gap-y-1 px-3 py-2.5 text-sm"
        :data-state="item.state ?? undefined"
        data-test="recent-import"
      >
        <span class="col-span-2 md:col-span-1 min-w-0 text-xs md:text-sm text-tertiary md:text-primary truncate">
          {{ formatCompactDate(item.created_at) }}
          <span v-if="item.created_by_name" class="text-xs text-tertiary">· {{ item.created_by_name }}</span>
        </span>
        <span class="min-w-0 truncate" :title="fileLabel(item)">
          <span class="font-mono text-xs text-primary">{{ fileLabel(item) }}</span>
          <span class="text-xs text-tertiary"> · {{ $t('import-matches-nouns', { type: item.job_type }) }}</span>
        </span>
        <span class="hidden md:block">
          <StatusPill
            v-if="item.state"
            :label="$t(STATE_KEY[item.state])"
            :tone="STATE_TONE[item.state]"
            data-test="recent-import-state"
          />
        </span>
        <span class="order-last md:order-none col-span-2 md:col-span-1 min-w-0 text-xs-plus text-secondary truncate" data-test="recent-import-counts">
          {{ counts(item) }}
        </span>
        <span class="justify-self-end">
          <LinkButton
            v-if="rowAction(item) === 'resume'"
            variant="secondary"
            size="sm"
            :to="resumeTarget(item.id)"
            data-test="recent-import-resume"
          >
            {{ $t('import-history-resume') }}
          </LinkButton>
          <LinkButton
            v-else-if="rowAction(item) === 'view'"
            variant="secondary"
            size="sm"
            :to="detailPath(item.id)"
            data-test="recent-import-view"
          >
            {{ $t('import-history-view') }}
          </LinkButton>
          <span v-else class="text-xs text-tertiary whitespace-nowrap">{{ $t('import-history-cannot-resume') }}</span>
        </span>
      </li>
    </ul>
  </SectionCard>
</template>
