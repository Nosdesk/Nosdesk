<!-- CollaborativeTicketArticle.vue -->
<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { useRouter } from 'vue-router';
import CollaborativeEditor from '@/components/CollaborativeEditor.vue';
import RevisionHistory from '@/components/editor/RevisionHistory.vue';
import apiClient from '@/services/apiConfig';

// Define props
interface Props {
  initialContent?: string;
  ticketId: number;
  initializing?: boolean;
}

const props = withDefaults(defineProps<Props>(), {
  initialContent: '',
  initializing: false,
});

const emit = defineEmits<{
  'update:content': [content: string];
  'initialization-complete': [];
}>();

// Use binary content for Yjs document
const content = ref('');
const router = useRouter();
const isLoading = ref(false); // Editor syncs via WebSocket, no need to wait for HTTP load

// Revision history state
const showRevisionHistory = ref(false);
const editorRef = ref<InstanceType<typeof CollaborativeEditor> | null>(null);

// No need to load content via HTTP - the CollaborativeEditor handles everything via WebSocket
// The editor will sync with the backend's in-memory Yjs document automatically
onMounted(() => {
  // Just mark as loaded immediately - editor handles content sync via WebSocket
  isLoading.value = false;
  emit('initialization-complete');
  if (import.meta.env.DEV) {
    console.log('CollaborativeTicketArticle mounted for ticket', props.ticketId, '- editor will sync via WebSocket');
  }
});

// Handle expand to full page editor
const handleExpand = () => {
  router.push({ 
    path: '/documentation', 
    query: { ticketId: String(props.ticketId) } 
  });
};

// No need to save via HTTP POST - backend automatically saves via WebSocket sync protocol
// Just update local state for any watchers
const handleContentChange = (newValue: string) => {
  content.value = newValue;
};

// Revision history handlers
const handleSelectRevision = async (revisionNumber: number | null) => {
  if (!editorRef.value) {
    console.error('Editor ref not available');
    return;
  }

  if (revisionNumber === null) {
    // Exit revision view and return to live document
    editorRef.value.exitRevisionView();
    return;
  }

  try {
    // Fetch the specific revision snapshot from the API
    const response = await apiClient.get(
      `/collaboration/tickets/${props.ticketId}/revisions/${revisionNumber}`
    );

    const revisionData = response.data;

    // Display the revision in the editor (read-only mode)
    editorRef.value.viewSnapshot(revisionData);
    console.log('Revision data received:', revisionData);
  } catch (error) {
    console.error('Failed to fetch revision:', error);
  }
};

const handleCloseRevisionHistory = () => {
  showRevisionHistory.value = false;
  // Also exit revision view if we're currently viewing one
  if (editorRef.value && editorRef.value.isViewingRevision) {
    editorRef.value.exitRevisionView();
  }
};

const toggleRevisionHistory = () => {
  showRevisionHistory.value = !showRevisionHistory.value;
};

// Handle convert to documentation
const handleConvertToDocumentation = async () => {
  try {
    // Backend handles both cases: returns existing page or creates new one
    const response = await apiClient.post(`/tickets/${props.ticketId}/documentation/create`, {
      title: `Documentation: Ticket #${props.ticketId}`,
      icon: '📋',
      parent_id: null
    });

    if (response.data && response.data.id) {
      // Navigate to the documentation page (existing or newly created)
      router.push(`/documentation/${response.data.id}`);
    }
  } catch (error) {
    console.error('Failed to convert to documentation:', error);
  }
};
</script>

<template>
  <div class="bg-surface rounded-xl border border-default flex flex-col w-full h-auto hover:border-strong transition-colors overflow-hidden">
    <!-- Header -->
    <div class="px-4 py-3 bg-surface-alt border-b border-default flex justify-between items-center">
      <h2 class="text-lg font-medium text-primary">Ticket Notes</h2>
      <div class="flex items-center gap-2">
        <!-- Revision History Toggle -->
        <button
          @click="toggleRevisionHistory"
          class="p-1.5 text-tertiary hover:text-primary hover:bg-surface-hover rounded-md transition-colors"
          :class="{ 'bg-surface-alt text-primary': showRevisionHistory }"
          title="Revision history"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 20 20" fill="currentColor">
            <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm1-12a1 1 0 10-2 0v4a1 1 0 00.293.707l2.828 2.829a1 1 0 101.415-1.415L11 9.586V6z" clip-rule="evenodd" />
          </svg>
        </button>

        <!-- Convert to Documentation -->
        <button
          @click="handleConvertToDocumentation"
          class="p-1.5 text-tertiary hover:text-primary hover:bg-surface-hover rounded-md transition-colors"
          title="Convert to documentation page"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 20 20" fill="currentColor">
            <path d="M9 4.804A7.968 7.968 0 005.5 4c-1.255 0-2.443.29-3.5.804v10A7.969 7.969 0 015.5 14c1.669 0 3.218.51 4.5 1.385A7.962 7.962 0 0114.5 14c1.255 0 2.443.29 3.5.804v-10A7.968 7.968 0 0014.5 4c-1.255 0-2.443.29-3.5.804V12a1 1 0 11-2 0V4.804z" />
          </svg>
        </button>

        <!-- Open Full Editor -->
        <button
          @click="handleExpand"
          class="p-1.5 text-tertiary hover:text-primary hover:bg-surface-hover rounded-md transition-colors"
          title="Open full editor"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 20 20" fill="currentColor">
            <path fill-rule="evenodd" d="M3 4a1 1 0 011-1h4a1 1 0 010 2H6.414l2.293 2.293a1 1 0 11-1.414 1.414L5 6.414V8a1 1 0 01-2 0V4zm9 1a1 1 0 010-2h4a1 1 0 011 1v4a1 1 0 01-2 0V6.414l-2.293 2.293a1 1 0 11-1.414-1.414L13.586 5H12zm-9 7a1 1 0 012 0v1.586l2.293-2.293a1 1 0 111.414 1.414L6.414 15H8a1 1 0 010 2H4a1 1 0 01-1-1v-4zm13-1a1 1 0 011 1v4a1 1 0 01-1 1h-4a1 1 0 010-2h1.586l-2.293-2.293a1 1 0 111.414-1.414L15 13.586V12a1 1 0 011-1z" clip-rule="evenodd" />
          </svg>
        </button>
      </div>
    </div>
    
    <!-- Content -->
    <div class="flex-grow flex items-stretch w-full min-h-[300px]">
      <!-- Editor Container - always rendered, syncs content via WebSocket -->
      <div class="flex-grow flex w-full">
        <CollaborativeEditor
          ref="editorRef"
          v-model="content"
          :doc-id="`ticket-${ticketId}`"
          :ticket-id="ticketId"
          :is-binary-update="true"
          :hide-revision-history="true"
          @update:model-value="handleContentChange"
          class="flex-grow w-full"
        />
      </div>

      <!-- Revision History Sidebar -->
      <RevisionHistory
        v-if="showRevisionHistory"
        :ticket-id="ticketId"
        @close="handleCloseRevisionHistory"
        @select-revision="handleSelectRevision"
        @restored="() => console.log('Revision restored')"
      />
    </div>
  </div>
</template>

<style scoped>
.editor-wrapper {
  position: relative;
  height: auto;
  width: 100%;
  overflow: visible;
}
</style> 