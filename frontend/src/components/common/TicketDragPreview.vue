<!-- TicketDragPreview.vue - Shared floating drag preview for tickets -->
<script setup lang="ts">
defineProps<{
  ticket: {
    id: number
    title: string
    priority?: 'low' | 'medium' | 'high'
    assignee?: string | null
  }
  position: { x: number; y: number }
}>()

const getPriorityBorderClass = (priority?: string) => {
  switch (priority) {
    case 'high': return 'border-l-priority-high'
    case 'medium': return 'border-l-priority-medium'
    case 'low': return 'border-l-priority-low'
    default: return 'border-l-subtle'
  }
}
</script>

<template>
  <Teleport to="body">
    <div
      class="fixed pointer-events-none z-[9999] w-64 md:w-72"
      :style="{
        left: `${position.x}px`,
        top: `${position.y}px`,
        transform: 'translate(-50%, -50%)'
      }"
    >
      <div
        class="bg-surface rounded-lg border-l-4 border border-accent shadow-lg p-3"
        :class="getPriorityBorderClass(ticket.priority)"
      >
        <span class="text-xs text-tertiary font-mono">#{{ ticket.id }}</span>
        <h4 class="text-sm font-medium text-primary mt-1 line-clamp-2">
          {{ ticket.title }}
        </h4>
        <div v-if="ticket.assignee || ticket.priority" class="flex items-center justify-between mt-3">
          <span class="text-xs text-tertiary">
            {{ ticket.assignee || 'Unassigned' }}
          </span>
          <span v-if="ticket.priority" class="text-xs text-tertiary capitalize">
            {{ ticket.priority }}
          </span>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.line-clamp-2 {
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

@keyframes drag-pickup {
  from {
    transform: translate(-50%, -50%) scale(0.95);
    opacity: 0;
  }
  to {
    transform: translate(-50%, -50%) scale(1);
    opacity: 1;
  }
}

.fixed.pointer-events-none {
  animation: drag-pickup 0.15s ease-out forwards;
}
</style>
