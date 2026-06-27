<script setup lang="ts">
/**
 * Enriched project card for the mobile projects list (the desktop
 * view uses ProjectRow). Carries the same signals as the row, laid
 * out vertically with more room: name + actions, description, a
 * status-breakdown bar with done/total, the active cycle, and a
 * footer with status, team, updated age, and deep links to
 * Board/Gantt/Cycles. The card opens the project; quick-nav and the
 * actions menu stop propagation.
 */
import { useInlineRename } from '@/composables/useInlineRename'
import { projectStatusDot } from '@nosdesk/core/utils/projectStatus'
import { formatRelativeTime } from '@nosdesk/core/utils/dateUtils'
import type { SyncProject } from '@/sync/stores/projects'
import type { ProjectRollup } from '@/composables/useProjectRollups'
import type { ActiveCycleSummary } from '@/composables/useActiveCycleSummaries'
import ProjectStatusBar from './ProjectStatusBar.vue'
import ProjectQuickNav from './ProjectQuickNav.vue'
import ProjectCycleGlance from './ProjectCycleGlance.vue'
import ProjectActionsMenu from './ProjectActionsMenu.vue'
import AvatarStack from '@/components/common/AvatarStack.vue'

defineProps<{
  project: SyncProject
  rollup: ProjectRollup | null
  cycle: ActiveCycleSummary | null
}>()

const emit = defineEmits<{
  (e: 'open'): void
  (e: 'contextmenu', event: MouseEvent): void
  (e: 'rename', name: string): void
  (e: 'set-status', status: string): void
  (e: 'delete'): void
}>()

const { editing, draft, inputEl, start, done, cancel } = useInlineRename((name) =>
  emit('rename', name),
)
</script>

<template>
  <div
    class="group flex flex-col gap-3 bg-surface border border-default rounded-lg p-4 cursor-pointer transition-colors hover:border-strong"
    @click="emit('open')"
    @contextmenu.prevent="emit('contextmenu', $event)"
  >
    <!-- Title + actions -->
    <div class="flex items-start justify-between gap-2">
      <input
        v-if="editing"
        ref="inputEl"
        v-model="draft"
        type="text"
        class="flex-1 min-w-0 text-base font-medium text-primary bg-surface-alt border border-default rounded px-2 py-0.5 focus:outline-none focus:border-accent focus:ring-2 focus:ring-accent/30"
        :aria-label="$t('project-actions-rename')"
        @click.stop
        @keyup.enter="done"
        @keyup.esc="cancel"
        @blur="done"
      />
      <button
        v-else
        type="button"
        class="flex-1 min-w-0 text-left text-base font-medium text-primary truncate rounded transition-colors group-hover:text-accent focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        @click.stop="emit('open')"
      >
        {{ project.name }}
      </button>
      <div class="shrink-0" @click.stop>
        <ProjectActionsMenu
          :status="project.status"
          @rename="start(project.name)"
          @set-status="(s) => emit('set-status', s)"
          @delete="emit('delete')"
        />
      </div>
    </div>

    <p v-if="project.description" class="text-sm text-secondary line-clamp-2">
      {{ project.description }}
    </p>

    <!-- Progress -->
    <div class="flex items-center gap-2">
      <ProjectStatusBar
        class="flex-1"
        :open="rollup?.open ?? 0"
        :in-progress="rollup?.inProgress ?? 0"
        :closed="rollup?.closed ?? 0"
        :total="rollup?.total ?? 0"
      />
      <span class="shrink-0 text-xs tabular-nums text-tertiary">
        {{ $t('projects-progress-done', { done: rollup?.closed ?? 0, total: rollup?.total ?? 0 }) }}
      </span>
    </div>

    <ProjectCycleGlance v-if="cycle" :summary="cycle" />

    <!-- Footer -->
    <div class="flex flex-col gap-2 mt-auto pt-1">
      <div class="flex items-center gap-3 text-xs text-tertiary">
        <span class="inline-flex items-center gap-1.5">
          <span class="w-1.5 h-1.5 rounded-full" :class="projectStatusDot(project.status)" />
          {{ $t(`project-actions-status-${project.status}`) }}
        </span>
        <AvatarStack
          v-if="rollup && rollup.assignees.length"
          class="ml-auto"
          :uuids="rollup.assignees"
          :max="4"
          size="xs"
        />
        <span v-if="project.updated_at" :class="rollup && rollup.assignees.length ? '' : 'ml-auto'">
          {{ formatRelativeTime(project.updated_at) }}
        </span>
      </div>
      <ProjectQuickNav :project-id="project.id" />
    </div>
  </div>
</template>
