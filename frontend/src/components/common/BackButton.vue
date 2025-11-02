<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { useRoute, useRouter } from 'vue-router';

const props = defineProps<{
  // Default fallback route if no previous route exists
  fallbackRoute?: string;
  // Custom label for the button (defaults to "Go back")
  label?: string;
  // Optional context for the back button (e.g., 'project', 'ticket')
  context?: string;
  // Optional ID for context-based navigation (e.g., projectId)
  contextId?: number | string;
}>();

const route = useRoute();
const router = useRouter();
const canGoBack = ref(window.history.length > 1);

const handleBack = () => {
  // If we have context information, use it for smarter navigation
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
  
  // Check if we can go back in history
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
    class="text-secondary hover:text-primary text-sm flex items-center gap-1 group"
  >
    <span class="text-xs group-hover:-translate-x-0.5 transition-transform">←</span>
    {{ label || 'Go back' }}
  </button>
</template> 