<script setup lang="ts">
import { ref, computed, onUnmounted } from 'vue';

const props = defineProps<{
  currentTime: number;
  duration: number;
}>();

const emit = defineEmits<{
  (e: 'seek', time: number): void;
  (e: 'dragstart'): void;
  (e: 'dragend'): void;
}>();

const progressBarRef = ref<HTMLElement | null>(null);
const isDragging = ref(false);
const draggedPosition = ref(0);
const rafId = ref<number | null>(null);
const lastSeekTime = ref<number>(0);
const SEEK_THROTTLE = 50; // Throttle for actual seek events

// Use the draggedPosition directly for visual updates when dragging
const indicatorPosition = computed(() => {
  return isDragging.value ? draggedPosition.value : (props.currentTime / props.duration) * 100 || 0;
});

// Disable logging for production, only enable for debugging
const log = (event: string, details?: any) => {
  // Uncomment for debugging
  // console.log(`[ProgressBar] ${event}`, details || '');
};

const updateSeek = (newPosition: number) => {
  if (!props.duration) return;
  
  // Always update visual position immediately for smooth UI
  draggedPosition.value = newPosition;
  
  const now = performance.now();
  const timeSinceLastSeek = now - lastSeekTime.value;
  
  // Throttle actual seek events to reduce performance impact
  if (timeSinceLastSeek >= SEEK_THROTTLE) {
    const seekTime = (newPosition / 100) * props.duration;
    emit('seek', seekTime);
    lastSeekTime.value = now;
  } else if (!rafId.value) {
    // Schedule a future seek if we're throttling
    rafId.value = requestAnimationFrame(() => {
      if (isDragging.value) { // Only emit if still dragging
        const seekTime = (draggedPosition.value / 100) * props.duration;
        emit('seek', seekTime);
      }
      lastSeekTime.value = performance.now();
      rafId.value = null;
    });
  }
};

const updatePositionFromMouse = (event: MouseEvent) => {
  if (!progressBarRef.value || !props.duration) return;
  const rect = progressBarRef.value.getBoundingClientRect();
  const x = Math.max(0, Math.min(event.clientX - rect.left, rect.width));
  const newPosition = (x / rect.width) * 100;
  updateSeek(newPosition);
};

const updatePositionFromTouch = (event: TouchEvent) => {
  if (!progressBarRef.value || !props.duration) return;
  const touch = event.touches[0];
  const rect = progressBarRef.value.getBoundingClientRect();
  const x = Math.max(0, Math.min(touch.clientX - rect.left, rect.width));
  const newPosition = (x / rect.width) * 100;
  updateSeek(newPosition);
};

const handleMouseDown = (event: MouseEvent) => {
  event.preventDefault();
  if (!props.duration || !progressBarRef.value) return;
  emit('dragstart');
  isDragging.value = true;
  updatePositionFromMouse(event);
  document.addEventListener('mousemove', handleMouseMove, { passive: true });
  document.addEventListener('mouseup', handleMouseUp);
};

const handleMouseMove = (event: MouseEvent) => {
  if (!isDragging.value) return;
  updatePositionFromMouse(event);
};

const handleMouseUp = () => {
  if (!props.duration) return;
  
  // Cancel any pending animation frame
  if (rafId.value) {
    cancelAnimationFrame(rafId.value);
    rafId.value = null;
  }
  
  // Ensure one final seek to the exact position
  const seekTime = (draggedPosition.value / 100) * props.duration;
  emit('seek', seekTime);
  emit('dragend');
  isDragging.value = false;
  document.removeEventListener('mousemove', handleMouseMove);
  document.removeEventListener('mouseup', handleMouseUp);
};

const handleTouchStart = (event: TouchEvent) => {
  event.preventDefault();
  if (!props.duration || !progressBarRef.value) return;
  emit('dragstart');
  isDragging.value = true;
  updatePositionFromTouch(event);
  document.addEventListener('touchmove', handleTouchMove, { passive: false });
  document.addEventListener('touchend', handleTouchEnd);
};

const handleTouchMove = (event: TouchEvent) => {
  if (!isDragging.value) return;
  event.preventDefault();
  updatePositionFromTouch(event);
};

const handleTouchEnd = () => {
  if (!props.duration) return;
  
  if (rafId.value) {
    cancelAnimationFrame(rafId.value);
    rafId.value = null;
  }
  
  const seekTime = (draggedPosition.value / 100) * props.duration;
  emit('seek', seekTime);
  emit('dragend');
  isDragging.value = false;
  document.removeEventListener('touchmove', handleTouchMove);
  document.removeEventListener('touchend', handleTouchEnd);
};

onUnmounted(() => {
  if (rafId.value) {
    cancelAnimationFrame(rafId.value);
  }
  document.removeEventListener('mousemove', handleMouseMove);
  document.removeEventListener('mouseup', handleMouseUp);
  document.removeEventListener('touchmove', handleTouchMove);
  document.removeEventListener('touchend', handleTouchEnd);
});
</script>

<template>
  <div
    ref="progressBarRef"
    class="w-full h-1.5 bg-surface-hover rounded-full cursor-pointer relative group"
    @mousedown.stop="handleMouseDown"
    @touchstart.stop="handleTouchStart"
  >
    <div
      class="absolute inset-y-0 left-0 bg-blue-500 rounded-full"
      :style="{ width: `${indicatorPosition}%` }"
    ></div>
    <div
      class="absolute w-3 h-3 bg-blue-500 rounded-full shadow-lg -translate-x-1/2 -translate-y-1/2 transition-transform"
      :style="{ left: `${indicatorPosition}%`, top: '50%' }"
      :class="{ 'scale-125': isDragging }"
    ></div>
  </div>
</template>