<script setup lang="ts">
/**
 * Dense project row for the desktop projects list. One line per
 * project: status dot + name, a status-breakdown bar with a done/total
 * count, the team (distinct assignees), the active cycle, deep links to
 * Board/Gantt/Cycles, and a compact "updated" age. The whole row opens
 * the project; quick-nav and the actions menu stop propagation. Column
 * widths are fixed so rows align under the header in ProjectsView.
 */
import { useInlineRename } from '@/composables/useInlineRename'
import { projectStatusDot } from '@/utils/projectStatus'
import { formatCompactRelativeTime } from '@/utils/dateUtils'
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
    class="group flex items-center gap-3 px-3 py-2 cursor-pointer transition-colors hover:bg-surface-hover"
    @click="emit('open')"
  >
    <!-- Name -->
    <div class="flex items-center gap-2 min-w-0 flex-1">
      <span class="w-1.5 h-1.5 rounded-full shrink-0" :class="projectStatusDot(project.status)" />
      <input
        v-if="editing"
        ref="inputEl"
        v-model="draft"
        type="text"
        class="min-w-0 flex-1 text-sm font-medium text-primary bg-surface-alt border border-default rounded px-2 py-0.5 focus:outline-none focus:border-accent focus:ring-2 focus:ring-accent/30"
        :aria-label="$t('project-actions-rename')"
        @click.stop
        @keyup.enter="done"
        @keyup.esc="cancel"
        @blur="done"
      />
      <button
        v-else
        type="button"
        class="min-w-0 truncate text-left text-sm font-medium text-primary rounded transition-colors group-hover:text-accent focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        @click.stop="emit('open')"
      >
        {{ project.name }}
      </button>
    </div>

    <!-- Progress -->
    <div class="w-44 shrink-0 flex items-center gap-2">
      <ProjectStatusBar
        class="flex-1"
        :open="rollup?.open ?? 0"
        :in-progress="rollup?.inProgress ?? 0"
        :closed="rollup?.closed ?? 0"
        :total="rollup?.total ?? 0"
      />
      <span class="shrink-0 text-xs tabular-nums text-tertiary">
        {{ rollup?.closed ?? 0 }}/{{ rollup?.total ?? 0 }}
      </span>
    </div>

    <!-- Team -->
    <div class="w-20 shrink-0">
      <AvatarStack
        v-if="rollup && rollup.assignees.length"
        :uuids="rollup.assignees"
        :max="3"
        size="xs"
      />
    </div>

    <!-- Active cycle -->
    <div class="w-48 shrink-0 min-w-0">
      <ProjectCycleGlance v-if="cycle" :summary="cycle" compact />
      <span v-else class="text-xs text-tertiary">{{ $t('projects-no-active-cycle') }}</span>
    </div>

    <!-- Quick nav -->
    <div class="w-40 shrink-0">
      <ProjectQuickNav :project-id="project.id" />
    </div>

    <!-- Updated -->
    <div class="w-12 shrink-0 text-right text-xs text-tertiary">
      <span v-if="project.updated_at">{{ formatCompactRelativeTime(project.updated_at) }}</span>
    </div>

    <!-- Actions -->
    <div class="shrink-0" @click.stop>
      <ProjectActionsMenu
        :status="project.status"
        @rename="start(project.name)"
        @set-status="(s) => emit('set-status', s)"
        @delete="emit('delete')"
      />
    </div>
  </div>
</template>
