<script setup lang="ts">
/**
 * A running import, honest about the mechanism: the rows load in one
 * transaction (a single stage, so a failure there leaves the workspace as
 * it was), then the files restore. The page follows the job; the user can
 * leave and come back.
 */
import { computed } from 'vue'

import Button from '@/components/common/Button.vue'
import Icon from '@/components/common/Icon.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import Spinner from '@/components/common/Spinner.vue'
import type { WorkspaceImportJob } from '@nosdesk/core/services/workspaceImportService'

import { STAGES, archiveName, stageIndex } from './workspaceImportModel'

const props = defineProps<{ job: WorkspaceImportJob }>()

const at = computed(() => stageIndex(props.job))
const files = computed(() => props.job.report?.source?.files ?? 0)
const started = computed(() => {
  const d = props.job.confirmed_at ? new Date(props.job.confirmed_at) : null
  return d && !Number.isNaN(d.getTime()) ? d.toLocaleTimeString(undefined, { timeStyle: 'short' }) : ''
})

const state = (i: number) => (i < at.value ? 'ok' : i === at.value ? 'wait' : 'todo')
</script>

<template>
  <section class="flex flex-col gap-6" data-test="workspace-progress">
    <SectionCard content-padding="">
      <template #title>
        {{ $t('workspace-import-progress-heading', { name: archiveName(job) }) }}
        <span v-if="started" class="font-medium text-tertiary"> · {{ $t('workspace-import-progress-started', { time: started }) }}</span>
      </template>
      <ul class="flex flex-col divide-y divide-default" aria-live="polite">
        <li
          v-for="(s, i) in STAGES"
          :key="s"
          class="flex items-start gap-3 px-3 py-2.5"
          :data-stage="s"
          :data-state="state(i)"
        >
          <span
            class="w-5 h-5 shrink-0 mt-px flex items-center justify-center"
            :class="state(i) === 'ok' ? 'text-status-success' : 'text-tertiary'"
          >
            <Spinner v-if="state(i) === 'wait'" size="sm" />
            <Icon v-else :name="state(i) === 'ok' ? 'checkCircle' : 'circleDot'" size="sm" :class="state(i) === 'todo' ? 'opacity-50' : ''" />
          </span>
          <div class="flex flex-col gap-0.5 min-w-0 flex-1">
            <p class="text-sm font-medium" :class="state(i) === 'todo' ? 'text-tertiary' : 'text-primary'">
              {{ $t(`workspace-import-stage-${s}`) }}
            </p>
            <p class="text-sm" :class="state(i) === 'todo' ? 'text-tertiary' : 'text-secondary'">
              {{ $t(`workspace-import-stage-${s}-detail`, { count: files }) }}
            </p>
            <div
              v-if="state(i) === 'wait' && (s === 'rows' || s === 'files')"
              class="relative w-full max-w-xs h-1.5 mt-1.5 rounded-full bg-surface-hover overflow-hidden"
            >
              <span class="indet absolute inset-y-0 w-1/3 bg-accent rounded-full" />
            </div>
          </div>
        </li>
      </ul>
      <p class="px-3 py-2 border-t border-default bg-surface-alt text-xs text-tertiary">{{ $t('workspace-import-progress-leave') }}</p>
    </SectionCard>

    <div class="sticky bottom-0 z-10 -mx-4 sm:mx-0 px-4 sm:px-0 py-3 bg-app border-t border-default flex flex-wrap items-center gap-3">
      <Button variant="ghost" @click="$router.push({ name: 'admin-data-import' })">{{ $t('import-history-title') }}</Button>
      <Button class="ml-auto" disabled :loading="true">{{ $t('workspace-import-importing') }}</Button>
    </div>
  </section>
</template>

<style scoped>
.indet {
  animation: workspace-import-indet 1.4s ease-in-out infinite;
}
@keyframes workspace-import-indet {
  0% { transform: translateX(-100%); }
  100% { transform: translateX(300%); }
}
</style>
