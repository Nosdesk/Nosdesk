<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';

interface Props {
  text: string;
  details: {
    title: string;
    date: string;
    tickets?: Array<{ id: number; title: string }>;
    totalTickets?: number;
  };
}

const props = defineProps<Props>();
const tooltipRef = ref<HTMLElement | null>(null);
const showTooltip = ref(false);
const position = ref({ x: 0, y: 0 });

const updatePosition = (event: MouseEvent) => {
  if (!tooltipRef.value) return;
  
  const tooltipWidth = tooltipRef.value.offsetWidth;
  const tooltipHeight = tooltipRef.value.offsetHeight;
  const windowWidth = window.innerWidth;
  const windowHeight = window.innerHeight;
  
  // Default position (below and to the right of the cursor)
  let x = event.clientX + 10;
  let y = event.clientY + 10;
  
  // Adjust if tooltip would go off the right edge
  if (x + tooltipWidth > windowWidth) {
    x = event.clientX - tooltipWidth - 10;
  }
  
  // Adjust if tooltip would go off the bottom
  if (y + tooltipHeight > windowHeight) {
    y = event.clientY - tooltipHeight - 10;
  }
  
  position.value = { x, y };
};

const handleMouseEnter = (event: MouseEvent) => {
  showTooltip.value = true;
  updatePosition(event);
};

const handleMouseMove = (event: MouseEvent) => {
  updatePosition(event);
};

const handleMouseLeave = () => {
  showTooltip.value = false;
};

onMounted(() => {
  window.addEventListener('mousemove', handleMouseMove);
});

onUnmounted(() => {
  window.removeEventListener('mousemove', handleMouseMove);
});
</script>

<template>
  <div
    class="relative"
    @mouseenter="handleMouseEnter"
    @mouseleave="handleMouseLeave"
  >
    <slot />
    
    <div
      v-if="showTooltip"
      ref="tooltipRef"
      class="fixed z-50 bg-surface border border-default rounded-lg shadow-lg p-3 min-w-[200px] max-w-[300px]"
      :style="{
        left: `${position.x}px`,
        top: `${position.y}px`
      }"
    >
      <div class="text-sm text-secondary mb-1">{{ details.date }}</div>
      <div class="text-primary font-medium mb-2">{{ details.title }}</div>

      <div v-if="details.tickets && details.tickets.length > 0" class="flex flex-col gap-1">
        <div
          v-for="ticket in details.tickets"
          :key="ticket.id"
          class="text-sm text-secondary truncate"
        >
          #{{ ticket.id }}: {{ ticket.title }}
        </div>

        <div
          v-if="details.totalTickets && details.totalTickets > 5"
          class="text-xs text-tertiary"
        >
          ...and {{ details.totalTickets - 5 }} more
        </div>
      </div>
    </div>
  </div>
</template> 