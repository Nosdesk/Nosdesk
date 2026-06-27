<!-- Floating drag preview for sidebar (and other HTML5) ticket drags. -->
<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import StatusIndicator from '@/components/common/StatusIndicator.vue'
import PriorityIndicator from '@/components/common/PriorityIndicator.vue'
import UserAvatar from '@/components/UserAvatar.vue'
import type { WorkflowStateCategory } from '@nosdesk/core/types/workflow'

const props = defineProps<{
  ticket: {
    id: number
    title: string
    category?: WorkflowStateCategory
    assigneeUuid?: string | null
    priority?: 'low' | 'medium' | 'high' | 'none'
  }
  position: { x: number; y: number }
  /** Additional cards in a multi-select drag (shown as "+ N more"). */
  extraCount?: number
}>()

/** Keep the preview inside the viewport with a small breathing room. */
const VIEWPORT_MARGIN = 8
/** Default cursor anchor within the card (matches kanban grab point). */
const CURSOR_ANCHOR_X = 0.4
const CURSOR_ANCHOR_Y = 0.5
/** Fallback size before the card is measured (w-64 × typical two-line card). */
const ESTIMATED_WIDTH = 256
const ESTIMATED_HEIGHT = 80

const cardRef = ref<HTMLElement | null>(null)
const measureVersion = ref(0)

function bumpMeasure(): void {
  measureVersion.value++
}

watch(
  () => [props.position.x, props.position.y, props.ticket.title, props.extraCount],
  () => { void nextTick(bumpMeasure) },
)

onMounted(() => { void nextTick(bumpMeasure) })

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value))
}

const wrapperStyle = computed(() => {
  void measureVersion.value

  const { x, y } = props.position
  const w = cardRef.value?.offsetWidth ?? ESTIMATED_WIDTH
  const h = cardRef.value?.offsetHeight ?? ESTIMATED_HEIGHT
  const vw = window.innerWidth
  const vh = window.innerHeight
  const m = VIEWPORT_MARGIN

  const idealLeft = x - CURSOR_ANCHOR_X * w
  const idealTop = y - CURSOR_ANCHOR_Y * h

  const left = clamp(idealLeft, m, Math.max(m, vw - m - w))
  const top = clamp(idealTop, m, Math.max(m, vh - m - h))

  return {
    left: `${left}px`,
    top: `${top}px`,
  }
})

const showPriority = computed(() =>
  props.ticket.priority != null && props.ticket.priority !== 'none',
)

const priorityLevel = computed((): 'low' | 'medium' | 'high' | undefined => {
  const p = props.ticket.priority
  if (p === 'low' || p === 'medium' || p === 'high') return p
  return undefined
})
</script>

<template>
  <Teleport to="body">
    <Transition name="ticket-drag-preview" appear>
      <div
        class="fixed top-0 left-0 pointer-events-none z-overlay will-change-[left,top]"
        :style="wrapperStyle"
      >
        <!-- Mirrors the compact kanban card chrome so a sidebar drag
             reads as the same object landing on the board. -->
        <div
          ref="cardRef"
          class="w-60 sm:w-64 rounded-lg border border-strong bg-surface p-2 shadow-xl ring-1 ring-accent/20"
        >
          <div class="flex items-start gap-1.5 min-w-0">
            <StatusIndicator
              v-if="ticket.category"
              :category="ticket.category"
              size="xs"
              class="mt-0.5 shrink-0"
            />
            <h4 class="text-[13px] font-medium text-primary line-clamp-2 flex-1 min-w-0">
              {{ ticket.title }}
            </h4>
            <PriorityIndicator
              v-if="showPriority && priorityLevel"
              :priority="priorityLevel"
              size="xs"
              class="shrink-0 mt-0.5"
            />
          </div>
          <div class="flex items-center justify-between mt-1.5 text-[11px] text-tertiary">
            <span class="font-mono">#{{ ticket.id }}</span>
            <UserAvatar
              v-if="ticket.assigneeUuid"
              :uuid="ticket.assigneeUuid"
              size="xxs"
              :show-name="false"
              :clickable="false"
            />
            <span v-else class="italic text-[10px]">{{ $t('filter-assignee-unassigned') }}</span>
          </div>
          <div
            v-if="extraCount && extraCount > 0"
            class="text-[10px] text-tertiary mt-1"
          >
            + {{ extraCount }} more
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.line-clamp-2 {
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.ticket-drag-preview-enter-active {
  transition: opacity 120ms ease-out;
}

.ticket-drag-preview-leave-active {
  transition: opacity 80ms ease-in;
}

.ticket-drag-preview-enter-from,
.ticket-drag-preview-leave-to {
  opacity: 0;
}

@media (prefers-reduced-motion: reduce) {
  .ticket-drag-preview-enter-active,
  .ticket-drag-preview-leave-active {
    transition: none;
  }
}
</style>
