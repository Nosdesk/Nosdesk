<script setup lang="ts">
/**
 * Workflow-state glyph. Encodes a ticket's category as a SHAPE
 * (not just a hue) so the meaning reads at a glance without a
 * colour legend, and so the cell stays legible for users with
 * colour-vision deficiency.
 *
 * The six glyphs share a circle base so they read as a visual
 * family — a "progress dial" the user's eye learns once and then
 * scans without thinking. Inspired by Linear's issue-state icons,
 * which set the modern convention for this pattern.
 *
 *   triage    — dashed ring (just arrived, not categorised)
 *   backlog   — empty solid ring (queued, no progress)
 *   active    — half-filled pie (work in motion, ~50%)
 *   in_review — 3/4-filled pie (nearly finished)
 *   done      — filled disc with a check (terminal: success)
 *   cancelled — filled disc with an X (terminal: abandoned)
 *
 * The hue still comes from the workspace-configured
 * `workflow_state.color`. Admins can recolour states; the glyph
 * shape stays anchored to the system category, so the meaning
 * stays readable even after a recolour. Cancelled is the one
 * exception — its terminal-abandoned shape reads better in a
 * neutral grey, so it ignores the per-state colour and pulls
 * the `subtle` palette directly.
 *
 * Replaces the earlier 12px coloured dot in the tickets list's
 * leading state cell. The dot encoded only the colour token; the
 * user had to memorise which colour mapped to which category.
 * The glyph carries that information in the shape itself.
 */
import { computed } from 'vue'
import { paletteForColor } from '@/utils/workflowColors'
import type { WorkflowStateCategory } from '@nosdesk/core/types/workflow'

const props = withDefaults(defineProps<{
  category: WorkflowStateCategory
  /** Hue token from `workflow_state.color`. Resolved via
   *  `paletteForColor` so a recolour propagates without code
   *  changes. */
  color: string
  /** Display name, used for the title attribute. */
  name?: string
  /** Pixel size. The leading state cell is 24px wide and the
   *  glyph wants ~14px to feel like a deliberate icon rather
   *  than a dot, with breathing room around it. */
  size?: number
}>(), {
  size: 14,
  name: '',
})

// Cancelled overrides the workspace colour with the neutral
// `subtle` palette — a vivid red/orange "cancelled" reads as an
// alert rather than the calm "this is done" terminal state we
// want. Other categories honour the per-state colour.
const colourClass = computed(() =>
  paletteForColor(props.category === 'cancelled' ? 'subtle' : props.color).solid
    .split(' ')
    .find((c) => c.startsWith('text-')) ?? 'text-tertiary',
)

const titleText = computed(() => props.name || props.category)
</script>

<template>
  <svg
    :width="size"
    :height="size"
    viewBox="0 0 14 14"
    :class="colourClass"
    aria-hidden="true"
  >
    <title>{{ titleText }}</title>

    <!-- Triage — dashed outer ring. The dashes read as
         "provisional / unsettled," matching the pre-categorised
         intake state. -->
    <circle
      v-if="category === 'triage'"
      cx="7"
      cy="7"
      r="5.5"
      stroke="currentColor"
      stroke-width="1.5"
      fill="none"
      stroke-dasharray="2.2 1.6"
    />

    <!-- Backlog — empty solid ring. "Queued, ready to start;
         nothing has happened yet." -->
    <circle
      v-else-if="category === 'backlog'"
      cx="7"
      cy="7"
      r="5.5"
      stroke="currentColor"
      stroke-width="1.5"
      fill="none"
    />

    <!-- Active — outer ring + right-half pie filled. The pie
         doubles as a half-progress-bar metaphor: "we're partway
         through this." -->
    <template v-else-if="category === 'active'">
      <circle
        cx="7"
        cy="7"
        r="5.5"
        stroke="currentColor"
        stroke-width="1.5"
        fill="none"
      />
      <path d="M7 3 A 4 4 0 0 1 7 11 Z" fill="currentColor" />
    </template>

    <!-- In review — outer ring + 3/4 pie filled. Same
         progress-dial metaphor advanced to nearly-complete; the
         missing top-left quadrant signals "one more step." -->
    <template v-else-if="category === 'in_review'">
      <circle
        cx="7"
        cy="7"
        r="5.5"
        stroke="currentColor"
        stroke-width="1.5"
        fill="none"
      />
      <path d="M7 3 A 4 4 0 1 1 3 7 L 7 7 Z" fill="currentColor" />
    </template>

    <!-- Done — filled disc with an inset checkmark. Terminal-
         success: the disc is fully filled and the check confirms
         the outcome. The check uses white-on-status which reads
         cleanly against every status hue in both themes. -->
    <template v-else-if="category === 'done'">
      <circle cx="7" cy="7" r="5.5" fill="currentColor" />
      <path
        d="M4.5 7.2 L 6.2 8.9 L 9.5 5.3"
        stroke="white"
        stroke-width="1.5"
        fill="none"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
    </template>

    <!-- Cancelled — filled disc with an inset X. Terminal-
         abandoned. The disc fills with the `subtle` palette
         (overridden above) so cancelled doesn't compete for
         attention with the active/done states. -->
    <template v-else-if="category === 'cancelled'">
      <circle cx="7" cy="7" r="5.5" fill="currentColor" />
      <path
        d="M4.8 4.8 L 9.2 9.2 M 9.2 4.8 L 4.8 9.2"
        stroke="white"
        stroke-width="1.5"
        stroke-linecap="round"
      />
    </template>
  </svg>
</template>
