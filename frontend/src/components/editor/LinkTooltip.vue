<script setup lang="ts">
import { ref, computed, watch, onMounted, onBeforeUnmount, nextTick } from 'vue';

// Props
interface Props {
  visible: boolean;
  url: string;
  x: number;
  y: number;
  isEditing: boolean;
}

const props = withDefaults(defineProps<Props>(), {
  visible: false,
  url: '',
  x: 0,
  y: 0,
  isEditing: false,
});

// Emits
const emit = defineEmits<{
  (e: 'apply', url: string): void;
  (e: 'remove'): void;
  (e: 'close'): void;
  (e: 'open-link', url: string): void;
  (e: 'request-reposition'): void;
}>();

// State
const tooltipRef = ref<HTMLElement | null>(null);
const inputRef = ref<HTMLInputElement | null>(null);
const linkUrl = ref('');
const showInput = ref(false);
const adjustedPosition = ref({ x: 0, y: 0, openUp: false });

// Update adjusted position when props change
watch([() => props.x, () => props.y, () => props.visible], () => {
  if (props.visible) {
    updateAdjustedPosition();
  }
}, { immediate: true });

// Calculate viewport-constrained position
const updateAdjustedPosition = () => {
  if (!props.visible) return;

  const tooltipWidth = 350; // approximate max width
  const tooltipHeight = 60; // approximate height
  const margin = 8;
  const viewportWidth = window.innerWidth;
  const viewportHeight = window.innerHeight;

  let x = props.x;
  let y = props.y;
  let openUp = false;

  // Check if tooltip would overflow right edge
  if (x + tooltipWidth > viewportWidth - margin) {
    x = Math.max(margin, viewportWidth - tooltipWidth - margin);
  }

  // Check if tooltip would overflow left edge
  if (x < margin) {
    x = margin;
  }

  // Check if tooltip would overflow bottom edge - open upward if needed
  if (y + tooltipHeight > viewportHeight - margin) {
    // Position above the anchor instead
    y = props.y - tooltipHeight - 16; // Move above with some gap
    openUp = true;
  }

  // Ensure tooltip is not off the top edge
  if (y < margin) {
    y = margin;
    openUp = false;
  }

  adjustedPosition.value = { x, y, openUp };
};

// Handle scroll events to request repositioning from parent
let scrollRafId: number | null = null;
const handleScroll = () => {
  if (!props.visible) return;

  if (scrollRafId) {
    cancelAnimationFrame(scrollRafId);
  }

  scrollRafId = requestAnimationFrame(() => {
    emit('request-reposition');
    scrollRafId = null;
  });
};

// Computed style with viewport awareness
const tooltipStyle = computed(() => {
  if (!props.visible) return { display: 'none' };

  return {
    position: 'fixed' as const,
    left: `${adjustedPosition.value.x}px`,
    top: `${adjustedPosition.value.y}px`,
    zIndex: 9999,
  };
});

// Watch for URL changes
watch(() => props.url, (newUrl) => {
  linkUrl.value = newUrl;
  showInput.value = props.isEditing;
}, { immediate: true });

// Watch for visibility and editing mode
watch(() => props.visible, async (visible) => {
  if (visible && props.isEditing) {
    showInput.value = true;
    // Focus input after DOM update
    await nextTick();
    inputRef.value?.focus();
    inputRef.value?.select();
  } else if (!visible) {
    showInput.value = false;
  }
});

// Methods
const handleApply = () => {
  const trimmedUrl = linkUrl.value.trim();
  if (trimmedUrl) {
    // Add https:// if no protocol specified
    const finalUrl = trimmedUrl.match(/^https?:\/\//)
      ? trimmedUrl
      : `https://${trimmedUrl}`;
    emit('apply', finalUrl);
  }
  showInput.value = false;
};

const handleRemove = () => {
  emit('remove');
};

const handleClose = () => {
  showInput.value = false;
  emit('close');
};

const handleEdit = () => {
  showInput.value = true;
  setTimeout(() => {
    inputRef.value?.focus();
    inputRef.value?.select();
  }, 50);
};

const handleOpenLink = () => {
  if (linkUrl.value) {
    emit('open-link', linkUrl.value);
  }
};

const handleKeydown = (e: KeyboardEvent) => {
  if (e.key === 'Enter') {
    e.preventDefault();
    handleApply();
  } else if (e.key === 'Escape') {
    e.preventDefault();
    handleClose();
  }
};

// Click outside handler
const handleClickOutside = (event: MouseEvent) => {
  if (!props.visible || !tooltipRef.value) return;

  const target = event.target as Node;

  // Don't close if clicking inside the tooltip
  if (tooltipRef.value.contains(target)) return;

  // Don't close if clicking inside the ProseMirror editor
  const proseMirrorEditor = document.querySelector('.ProseMirror');
  if (proseMirrorEditor && proseMirrorEditor.contains(target)) return;

  // Close for all other clicks
  handleClose();
};

onMounted(() => {
  document.addEventListener('mousedown', handleClickOutside);
  window.addEventListener('scroll', handleScroll, true);
  window.addEventListener('resize', updateAdjustedPosition);
});

onBeforeUnmount(() => {
  document.removeEventListener('mousedown', handleClickOutside);
  window.removeEventListener('scroll', handleScroll, true);
  window.removeEventListener('resize', updateAdjustedPosition);
  if (scrollRafId) {
    cancelAnimationFrame(scrollRafId);
  }
});
</script>

<template>
  <div
    v-if="visible"
    ref="tooltipRef"
    :style="tooltipStyle"
    class="link-tooltip"
  >
    <!-- Input mode (editing or adding link) -->
    <div v-if="showInput" class="flex items-center gap-2 p-2">
      <input
        ref="inputRef"
        v-model="linkUrl"
        type="text"
        placeholder="Enter URL..."
        class="flex-1 px-3 py-1.5 bg-surface border border-default rounded-lg text-sm text-primary focus:ring-2 focus:ring-accent focus:border-accent focus:outline-none"
        @keydown="handleKeydown"
      />
      <button
        @click="handleApply"
        class="px-3 py-1.5 bg-accent text-white rounded-lg hover:opacity-90 transition-colors text-sm font-medium"
        title="Apply link"
      >
        Apply
      </button>
      <button
        @click="handleClose"
        class="px-2 py-1.5 bg-surface-hover text-secondary rounded-lg hover:bg-surface-alt transition-colors text-sm"
        title="Cancel"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" viewBox="0 0 20 20" fill="currentColor">
          <path fill-rule="evenodd" d="M4.293 4.293a1 1 0 011.414 0L10 8.586l4.293-4.293a1 1 0 111.414 1.414L11.414 10l4.293 4.293a1 1 0 01-1.414 1.414L10 11.414l-4.293 4.293a1 1 0 01-1.414-1.414L8.586 10 4.293 5.707a1 1 0 010-1.414z" clip-rule="evenodd" />
        </svg>
      </button>
    </div>

    <!-- Display mode (showing existing link) -->
    <div v-else class="flex items-center gap-2 p-2">
      <a
        :href="linkUrl"
        target="_blank"
        rel="noopener noreferrer"
        class="flex-1 px-2 py-1 text-sm text-accent hover:text-accent truncate max-w-xs"
        @click.prevent="handleOpenLink"
        :title="linkUrl"
      >
        {{ linkUrl }}
      </a>
      <button
        @click="handleEdit"
        class="p-1.5 text-secondary hover:text-primary hover:bg-surface-hover rounded transition-colors"
        title="Edit link"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" viewBox="0 0 20 20" fill="currentColor">
          <path d="M13.586 3.586a2 2 0 112.828 2.828l-.793.793-2.828-2.828.793-.793zM11.379 5.793L3 14.172V17h2.828l8.38-8.379-2.83-2.828z" />
        </svg>
      </button>
      <button
        @click="handleRemove"
        class="p-1.5 text-secondary hover:text-status-error hover:bg-surface-hover rounded transition-colors"
        title="Remove link"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" viewBox="0 0 20 20" fill="currentColor">
          <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.707 7.293a1 1 0 00-1.414 1.414L8.586 10l-1.293 1.293a1 1 0 101.414 1.414L10 11.414l1.293 1.293a1 1 0 001.414-1.414L11.414 10l1.293-1.293a1 1 0 00-1.414-1.414L10 8.586 8.707 7.293z" clip-rule="evenodd" />
        </svg>
      </button>
    </div>
  </div>
</template>

<style scoped>
.link-tooltip {
  background-color: var(--color-surface);
  border: 1px solid var(--color-default);
  border-radius: 0.5rem;
  box-shadow: 0 10px 15px -3px rgba(0, 0, 0, 0.3), 0 4px 6px -2px rgba(0, 0, 0, 0.2);
  min-width: 300px;
  max-width: 500px;
}
</style>
