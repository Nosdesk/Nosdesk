<script setup lang="ts">
import { ref } from 'vue';
import { useRouter } from 'vue-router';

const props = defineProps<{
  // Default fallback route if no previous route exists
  fallbackRoute?: string;
  // Custom label for the button (defaults to "Go back")
  label?: string;
  // Optional context for the back button (e.g., 'project', 'ticket')
  context?: string;
  // Optional ID for context-based navigation (e.g., projectId)
  contextId?: number | string;
  // Compact mode - smaller text, tighter spacing
  compact?: boolean;
}>();

const router = useRouter();
const canGoBack = ref(window.history.length > 1);

const handleBack = () => {
  // Use context information for smarter navigation when available
  if (props.context && props.contextId) {
    switch (props.context) {
      case 'project':
        // Navigate to the specific project
        router.push(`/projects/${props.contextId}`);
        return;
      case 'ticket':
        // Navigate to the specific ticket
        router.push(`/tickets/${props.contextId}`);
        return;
      // Add more context-specific cases as needed
    }
  }
  
  // Check if browser history navigation is available
  if (canGoBack.value) {
    router.back();
  } else if (props.fallbackRoute) {
    // Use the provided fallback route
    router.push(props.fallbackRoute);
  } else {
    // Default fallback to home if no fallback route is provided
    router.push('/');
  }
};
</script>

<template>
  <button
    @click="handleBack"
    class="text-secondary hover:text-primary flex items-center gap-1 group"
    :class="compact ? 'text-xs' : 'text-sm'"
  >
    <svg
      class="group-hover:-translate-x-0.5 transition-transform"
      :class="compact ? 'w-3 h-3' : 'w-3.5 h-3.5'"
      fill="none"
      stroke="currentColor"
      viewBox="0 0 24 24"
    >
      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 19l-7-7 7-7" />
    </svg>
    {{ label || 'Go back' }}
  </button>
</template> 