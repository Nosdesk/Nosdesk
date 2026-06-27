<!--
Tiny progress-state icon for a ticket, in the same vocabulary as
GitHub's PR status glyphs. Driven by `WorkflowStateCategory` (the
system-level 7-value enum) rather than the legacy three-bucket
`status` string, so workspace-configured state names render the
correct lifecycle position without the FE having to know the name.

Visual groups, derived from category:
  open        triage, backlog        empty outlined circle
  in_progress active, in_review      half-filled circle
  closed      done, cancelled        filled with check
  merged      merged                 filled with merge glyph
                                     (distinct from closed so a
                                     consumed-by-merge ticket reads
                                     visually different from a
                                     finished one)

`category` is optional because the parent typically derives it from
`workflowStatesStore.findById(workflow_state_id)?.category`, which is
undefined while the store is loading. In that window the icon renders
nothing rather than a misleading default.
-->
<script setup lang="ts">
import { computed } from 'vue'
import type { WorkflowStateCategory } from '@nosdesk/core/types/workflow'

const props = defineProps<{
  category?: WorkflowStateCategory
  /** Optional title override for the native tooltip + aria-label.
   *  Defaults to the visual group's English label. Callers that
   *  resolve the workflow-state's display name typically pass it
   *  here so the tooltip reads "In Review" instead of the generic
   *  "In progress". */
  title?: string
}>()

type VisualGroup = 'open' | 'in_progress' | 'closed' | 'merged'

const visualGroup = computed<VisualGroup | null>(() => {
  switch (props.category) {
    case 'triage':
    case 'backlog':
      return 'open'
    case 'active':
    case 'in_review':
      return 'in_progress'
    case 'done':
    case 'cancelled':
      return 'closed'
    case 'merged':
      return 'merged'
    default:
      return null
  }
})

const label = computed(() => {
  if (props.title) return props.title
  switch (visualGroup.value) {
    case 'open':
      return 'Open'
    case 'in_progress':
      return 'In progress'
    case 'closed':
      return 'Closed'
    case 'merged':
      return 'Merged'
    default:
      return ''
  }
})

const toneClass = computed(() => {
  switch (visualGroup.value) {
    case 'open':
      return 'text-status-open'
    case 'in_progress':
      return 'text-status-in-progress'
    case 'closed':
      return 'text-status-closed'
    case 'merged':
      return 'text-status-merged'
    default:
      return 'text-tertiary'
  }
})
</script>

<template>
  <svg
    v-if="visualGroup"
    :class="['flex-shrink-0', toneClass]"
    viewBox="0 0 24 24"
    :aria-label="label"
    role="img"
  >
    <title>{{ label }}</title>

    <template v-if="visualGroup === 'open'">
      <circle cx="12" cy="12" r="8" fill="none" stroke="currentColor" stroke-width="2.5" />
    </template>

    <template v-else-if="visualGroup === 'in_progress'">
      <circle cx="12" cy="12" r="8" fill="none" stroke="currentColor" stroke-width="2.5" />
      <path d="M12 4 A 8 8 0 0 1 12 20 Z" fill="currentColor" />
    </template>

    <template v-else-if="visualGroup === 'closed'">
      <circle cx="12" cy="12" r="9" fill="currentColor" />
      <path
        d="M8 12 L11 15 L16 9"
        fill="none"
        stroke="white"
        stroke-width="2.5"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
    </template>

    <!-- Merged: filled disc with a Y-shaped merge arrow ending in
         a small node, the canonical "two paths converged" glyph
         (Git network graphs, GitHub merge button). Readable at
         14px because the arrow joins at the centre and the node
         is a single dot. -->
    <template v-else-if="visualGroup === 'merged'">
      <circle cx="12" cy="12" r="9" fill="currentColor" />
      <path
        d="M9 7 L9 13 L12 16 L15 16"
        fill="none"
        stroke="white"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
      <path
        d="M15 11 L15 16"
        fill="none"
        stroke="white"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
      <circle cx="15" cy="9" r="1.6" fill="white" />
    </template>
  </svg>
</template>
