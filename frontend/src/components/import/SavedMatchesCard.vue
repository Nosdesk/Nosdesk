<script setup lang="ts">
/**
 * The hub's Saved matches card: per import type with anything saved, up to
 * three examples, how many values and columns are saved and which import
 * the columns come from, and Manage for the full page.
 */
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import { useQuery } from '@pinia/colada'

import SectionCard from '@/components/common/SectionCard.vue'
import { assetStatusLabel } from '@/utils/assetStatusMeta'
import { formatCompactDate } from '@nosdesk/core/utils/dateUtils'
import {
  importAnswersQueryKey,
  importService,
  type ImportJobType,
  type SavedMatch,
  type SavedMatches,
} from '@nosdesk/core/services/importService'

import { examples, firstName } from './matchesModel'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const TYPES: ImportJobType[] = ['assets', 'users', 'tickets']

const queries = TYPES.map((type) => ({
  type,
  query: useQuery({
    key: () => importAnswersQueryKey(type),
    query: () => importService.answers(type),
  }),
}))

interface Summary {
  type: ImportJobType
  view: SavedMatches
  examples: SavedMatch[]
  more: number
}

const summaries = computed<Summary[]>(() =>
  queries
    .map(({ type, query }) => ({ type, view: query.data.value }))
    .filter((s): s is { type: ImportJobType; view: SavedMatches } =>
      !!s.view && (s.view.entries.length > 0 || !!s.view.columns),
    )
    .map(({ type, view }) => {
      const shown = examples(view.entries)
      return { type, view, examples: shown, more: view.entries.length - shown.length }
    }),
)

const loaded = computed(() => queries.every(({ query }) => query.data.value !== undefined))

function outcome(type: ImportJobType, e: SavedMatch): string {
  if (type === 'assets' && e.field === 'status' && e.target) return assetStatusLabel(t, e.target)
  return e.label ?? e.target ?? ''
}

function line(s: Summary): string {
  const nouns = t('import-matches-nouns', { type: s.type })
  const c = s.view.columns
  if (!c) return t('import-matches-hub-line-values', { values: s.view.entries.length, nouns })
  const date = formatCompactDate(c.at)
  const who =
    c.by && c.by_name
      ? t('import-matches-who', { name: firstName(c.by_name), date })
      : t('import-matches-who-unknown', { date })
  return t('import-matches-hub-line', {
    values: s.view.entries.length,
    columns: c.rows.length,
    nouns,
    who,
  })
}
</script>

<template>
  <SectionCard
    content-padding=""
    action-to="/admin/data-import/matches"
    :action-label="$t('import-matches-manage')"
    data-test="hub-saved-matches"
  >
    <template #title>{{ $t('import-matches-title') }}</template>
    <p v-if="loaded && summaries.length === 0" class="px-3 py-3 text-sm text-secondary">
      {{ $t('import-matches-empty') }}
    </p>
    <div v-else class="flex flex-col divide-y divide-default">
      <RouterLink
        v-for="s in summaries"
        :key="s.type"
        :to="{ path: '/admin/data-import/matches', query: { type: s.type } }"
        class="flex flex-col gap-1 px-3 py-2.5 hover:bg-surface-hover transition-colors"
        data-test="hub-saved-matches-type"
      >
        <p v-if="s.examples.length > 0" class="text-sm text-primary flex flex-wrap items-baseline gap-x-1.5">
          <template v-for="(e, i) in s.examples" :key="e.id">
            <span v-if="i > 0" aria-hidden="true" class="text-tertiary">·</span>
            <span><span class="font-mono text-xs">{{ e.text }}</span> → {{ outcome(s.type, e) }}</span>
          </template>
          <span v-if="s.more > 0" class="text-tertiary">{{ $t('import-matches-hub-more', { count: s.more }) }}</span>
        </p>
        <p class="text-xs text-tertiary">{{ line(s) }}</p>
      </RouterLink>
    </div>
  </SectionCard>
</template>
