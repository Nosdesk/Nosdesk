<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount, computed } from 'vue';

const props = defineProps({
  // Initial height of the pane in pixels
  initialHeight: {
    type: Number,
    default: 200
  },
  // Minimum height of this pane
  minHeight: {
    type: Number,
    default: 60
  },
  // Minimum height for the remaining space (if available)
  minOtherHeight: {
    type: Number,
    default: 60
  },
  // Storage key for persisting the height
  storageKey: {
    type: String,
    default: 'resizable-pane-height'
  },
  // Direction of resizing: 'ns' (north-south) or 'ew' (east-west)
  direction: {
    type: String,
    default: 'ns',
    validator: (value: string) => ['ns', 'ew'].includes(value)
  },
  // Optional: Maximum available height/width for constraints
  maxAvailableSize: {
    type: Number,
    default: null
  }
});

const emit = defineEmits(['heightChange', 'resize-start', 'resize-end']);

// Refs
const containerRef = ref<HTMLElement | null>(null);
const paneRef = ref<HTMLElement | null>(null);
const resizerRef = ref<HTMLElement | null>(null);

// State
const paneSize = ref(props.initialHeight);
const isResizing = ref(false);
let dragStartPos = 0;
let initialSize = 0;
let rafId: number | null = null;

// Load saved size on mount
onMounted(() => {
  const storedSize = localStorage.getItem(props.storageKey);
  if (storedSize) {
    paneSize.value = parseInt(storedSize, 10);
  }
});

// Compute the appropriate style property based on direction
const sizeStyleProp = computed(() => {
  return props.direction === 'ns' ? 'height' : 'width';
});

// Compute the cursor style for the resizer
const cursorStyle = computed(() => {
  return props.direction === 'ns' ? 'ns-resize' : 'ew-resize';
});

// Compute the client coordinate to use based on direction
const getClientPos = (event: MouseEvent | TouchEvent): number => {
  const isTouch = 'touches' in event;
  return props.direction === 'ns' 
    ? (isTouch ? event.touches[0].clientY : event.clientY)
    : (isTouch ? event.touches[0].clientX : event.clientX);
};

// Start resize operation
const startResize = (event: MouseEvent | TouchEvent) => {
  event.preventDefault();
  
  if (!paneRef.value) return;
  
  // Enable hardware acceleration
  paneRef.value.style.willChange = props.direction === 'ns' ? 'height' : 'width';
  
  // Get current size directly from DOM
  initialSize = props.direction === 'ns' 
    ? paneRef.value.offsetHeight 
    : paneRef.value.offsetWidth;
  
  // Get cursor position
  dragStartPos = getClientPos(event);
  
  // Set resizing state
  isResizing.value = true;
  emit('resize-start', { size: initialSize });
  
  // Add event listeners
  document.addEventListener('mousemove', handleResize, { passive: false, capture: true });
  document.addEventListener('mouseup', stopResize, { capture: true });
  document.addEventListener('touchmove', handleResize, { passive: false, capture: true });
  document.addEventListener('touchend', stopResize, { capture: true });
  document.addEventListener('touchcancel', stopResize, { capture: true });
  
  document.body.classList.add('resize-active');
};

// Handle resize with requestAnimationFrame
const handleResize = (event: MouseEvent | TouchEvent) => {
  if (!isResizing.value) return;
  
  event.preventDefault();
  
  // Cancel any pending animation frame
  if (rafId !== null) {
    cancelAnimationFrame(rafId);
  }
  
  // Use requestAnimationFrame for smooth performance
  rafId = requestAnimationFrame(() => {
    if (!paneRef.value) return;
    
    // Get current position
    const clientPos = getClientPos(event);
    
    // Calculate delta
    const delta = props.direction === 'ns'
      ? clientPos - dragStartPos  // For NS, positive delta means increase height
      : dragStartPos - clientPos; // For EW, negative delta means increase width (RTL adjustment)
    
    // Calculate new size
    let newSize = initialSize + delta;
    
    // Apply constraints
    let maxSize = props.maxAvailableSize;
    if (maxSize === null && containerRef.value) {
      maxSize = props.direction === 'ns'
        ? containerRef.value.offsetHeight - props.minOtherHeight
        : containerRef.value.offsetWidth - props.minOtherHeight;
    }
    
    if (maxSize !== null) {
      newSize = Math.min(newSize, maxSize);
    }
    
    // Enforce minimum size
    newSize = Math.max(props.minHeight, newSize);
    
    // Update the size
    paneSize.value = newSize;
    applySize(newSize);
    
    // Emit height change event
    emit('heightChange', newSize);
  });
};

// Apply size update to the DOM
const applySize = (size: number) => {
  if (!paneRef.value) return;
  
  const prop = props.direction === 'ns' ? 'height' : 'width';
  paneRef.value.style[prop] = `${size}px`;
};

// Stop resize operation
const stopResize = () => {
  if (!isResizing.value) return;
  
  // Cancel any pending animation frame
  if (rafId !== null) {
    cancelAnimationFrame(rafId);
    rafId = null;
  }
  
  isResizing.value = false;
  
  // Disable hardware acceleration hints
  if (paneRef.value) {
    paneRef.value.style.willChange = 'auto';
  }
  
  // Clean up listeners
  document.removeEventListener('mousemove', handleResize, { capture: true });
  document.removeEventListener('mouseup', stopResize, { capture: true });
  document.removeEventListener('touchmove', handleResize, { capture: true });
  document.removeEventListener('touchend', stopResize, { capture: true });
  document.removeEventListener('touchcancel', stopResize, { capture: true });
  document.body.classList.remove('resize-active');
  
  // Save preference
  localStorage.setItem(props.storageKey, paneSize.value.toString());
  
  // Emit resize end event
  emit('resize-end', { size: paneSize.value });
};

// Cleanup
onBeforeUnmount(() => {
  if (rafId !== null) {
    cancelAnimationFrame(rafId);
  }
  stopResize();
});

// Reset to 50% of available space
const resetToHalf = () => {
  if (!containerRef.value) return;
  
  const totalSize = props.direction === 'ns'
    ? containerRef.value.offsetHeight
    : containerRef.value.offsetWidth;
    
  const halfSize = Math.floor(totalSize / 2);
  const newSize = Math.max(props.minHeight, halfSize);
  
  paneSize.value = newSize;
  applySize(newSize);
  localStorage.setItem(props.storageKey, newSize.toString());
  emit('heightChange', newSize);
};

// Expose public API
defineExpose({
  paneSize,
  isResizing,
  resetToHalf
});
</script>

<template>
  <div ref="containerRef" class="resizable-container">
    <!-- The resizable pane -->
    <div 
      ref="paneRef" 
      class="resizable-pane"
      :class="[direction === 'ns' ? 'ns-direction' : 'ew-direction']"
      :style="{ 
        [sizeStyleProp]: `${paneSize}px`,
        transition: isResizing ? 'none' : `${sizeStyleProp} 0.2s cubic-bezier(0.25, 1, 0.5, 1)`
      }"
    >
      <slot name="pane-content"></slot>
    </div>
    
    <!-- Resizer handle -->
    <div 
      ref="resizerRef"
      class="resizer-handle"
      :class="[
        direction === 'ns' ? 'horizontal-resizer' : 'vertical-resizer', 
        { 'active': isResizing }
      ]"
      @mousedown="startResize"
      @touchstart.prevent="startResize"
      :style="{ cursor: cursorStyle }"
    >
      <slot name="resizer">
        <!-- Default resizer bar -->
        <div class="resizer-indicator"></div>
      </slot>
    </div>
    
    <!-- Content after the resizable pane -->
    <div class="after-pane">
      <slot name="after-content"></slot>
    </div>
  </div>
</template>

<style scoped>
.resizable-container {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  position: relative;
  overflow: hidden;
}

/* For east-west direction */
.resizable-container:has(.ew-direction) {
  flex-direction: row;
}

.resizable-pane {
  flex-shrink: 0;
  overflow: hidden;
  position: relative;
  will-change: height, width;
  transform: translateZ(0);
  backface-visibility: hidden;
  perspective: 1000px;
}

.ns-direction {
  width: 100%;
}

.ew-direction {
  height: 100%;
}

.after-pane {
  flex: 1;
  min-height: v-bind('props.minOtherHeight + "px"');
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

/* For east-west direction */
.resizable-container:has(.ew-direction) .after-pane {
  min-height: 0;
  min-width: v-bind('props.minOtherHeight + "px"');
}

.resizer-handle {
  position: relative;
  touch-action: none;
  z-index: 1;
  flex-shrink: 0;
}

.horizontal-resizer {
  height: 5px;
  width: 100%;
  cursor: ns-resize;
  background-color: var(--color-surface);
  border-top: 1px solid var(--color-border-default);
  border-bottom: 1px solid var(--color-border-default);
}

.vertical-resizer {
  width: 5px;
  height: 100%;
  cursor: ew-resize;
  background-color: var(--color-surface);
  border-left: 1px solid var(--color-border-default);
  border-right: 1px solid var(--color-border-default);
}

.resizer-handle:hover {
  background-color: var(--color-surface-hover);
}

.resizer-handle.active {
  background-color: rgba(96, 165, 250, 0.3);
}

/* Subtle indicator lines */
.horizontal-resizer:hover::after {
  content: '';
  position: absolute;
  left: 0;
  right: 0;
  height: 0.5px;
  background-color: rgba(96, 165, 250, 0.3);
  top: 50%;
  transform: translateY(-50%);
  opacity: 0.5;
  z-index: 5;
  pointer-events: none;
}

.horizontal-resizer.active::after {
  content: '';
  position: absolute;
  left: 0;
  right: 0;
  height: 0.5px;
  background-color: rgba(96, 165, 250, 0.5);
  top: 50%;
  transform: translateY(-50%);
  opacity: 0.6;
  z-index: 5;
  pointer-events: none;
}

.vertical-resizer:hover::after {
  content: '';
  position: absolute;
  top: 0;
  bottom: 0;
  width: 0.5px;
  background-color: rgba(96, 165, 250, 0.3);
  left: 50%;
  transform: translateX(-50%);
  opacity: 0.5;
  z-index: 5;
  pointer-events: none;
}

.vertical-resizer.active::after {
  content: '';
  position: absolute;
  top: 0;
  bottom: 0;
  width: 0.5px;
  background-color: rgba(96, 165, 250, 0.5);
  left: 50%;
  transform: translateX(-50%);
  opacity: 0.6;
  z-index: 5;
  pointer-events: none;
}

/* Global styles (you may need to move this to a global CSS file) */
:global(.resize-active) {
  cursor: ns-resize !important; 
  user-select: none !important;
}

:global(.resize-active:has(.vertical-resizer)) {
  cursor: ew-resize !important;
}

:global(.resize-active *) {
  user-select: none !important;
  pointer-events: none !important;
}

:global(.resize-active .resizer-handle) {
  pointer-events: auto !important;
}
</style> 