<!-- CollaborativeTicketArticle.vue -->
<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import CollaborativeEditor from '@/components/CollaborativeEditor.vue';
import RevisionList from '@/components/editor/RevisionList.vue';
import SectionCard from '@/components/common/SectionCard.vue';
import Icon from '@/components/common/Icon.vue';
import apiClient from '@/services/apiConfig';
import { docUrl } from '@/utils/docUrl';
import { useCollabDocId } from '@/composables/useCollabDocId';
import { useSyncTicketsStore } from '@/sync/stores/tickets';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

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

// Workspace-namespaced docId keyed by the ticket's immutable UUID (not
// the recyclable integer id), resolved from the sync pool. Resolves to
// `null` until both the my-workspaces query and the pool row land; the
// editor is gated on a non-null value below so we never hand a
// half-formed id to Yjs.
const ticketsStore = useSyncTicketsStore();
const docId = useCollabDocId('ticket', () => ticketsStore.byId(props.ticketId).value?.uuid ?? null);

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
    // Exit revision view, return to live document
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

const toggleRevisionHistory = () => {
  // Closing: also exit any in-progress revision preview so the
  // editor returns to the live document instead of stranding the
  // user on a snapshot they can't close from anywhere else.
  if (showRevisionHistory.value && editorRef.value?.isViewingRevision) {
    editorRef.value.exitRevisionView();
  }
  showRevisionHistory.value = !showRevisionHistory.value;
};

// Handle convert to documentation
const handleConvertToDocumentation = async () => {
  try {
    // Backend handles both cases: returns existing page or creates new one
    const response = await apiClient.post(`/tickets/${props.ticketId}/documentation/create`, {
      title: t('tickets-collaborative-article-doc-title', { id: props.ticketId }),
      icon: '📋',
      parent_id: null
    });

    if (response.data && response.data.id) {
      // Navigate to the documentation page (existing or newly created)
      router.push(docUrl(response.data));
    }
  } catch (error) {
    console.error('Failed to convert to documentation:', error);
  }
};
</script>

<template>
  <SectionCard content-padding="">
    <template #title>{{ t('tickets-collaborative-article-title') }}</template>
    <template #headerActions>
      <button
        @click="toggleRevisionHistory"
        class="p-1 text-tertiary hover:text-primary hover:bg-surface-hover rounded transition-colors"
        :class="{ 'bg-surface text-primary': showRevisionHistory }"
        :title="t('tickets-collaborative-article-revision-history')"
      >
        <Icon name="clock" />
      </button>
      <button
        @click="handleConvertToDocumentation"
        class="p-1 text-tertiary hover:text-primary hover:bg-surface-hover rounded transition-colors"
        :title="t('tickets-collaborative-article-convert-doc')"
      >
        <Icon name="documentEdit" />
      </button>
      <button
        @click="handleExpand"
        class="p-1 text-tertiary hover:text-primary hover:bg-surface-hover rounded transition-colors"
        :title="t('tickets-collaborative-article-open-full')"
      >
        <Icon name="openExternal" />
      </button>
    </template>

    <!-- Two-column body: editor stretches, revisions dock to the
         right when open. RevisionList is rendered bare (no panel
         chrome) so it shares the SectionCard's frame instead of
         stacking a second card on top. The toggle in the header
         opens / closes; no separate close affordance needed. -->
    <div class="flex-grow flex flex-col md:flex-row md:items-stretch w-full min-h-[300px] print:min-h-0">
      <CollaborativeEditor
        v-if="docId"
        ref="editorRef"
        v-model="content"
        :doc-id="docId"
        :ticket-id="ticketId"
        :is-binary-update="true"
        :hide-revision-history="true"
        @update:model-value="handleContentChange"
        class="flex-grow w-full"
      />

      <aside
        v-if="showRevisionHistory"
        class="w-full md:w-72 flex-shrink-0 flex flex-col border-t md:border-t-0 md:border-l border-default bg-surface-alt/30"
        :aria-label="t('tickets-collaborative-article-revision-history')"
      >
        <RevisionList
          :ticket-id="ticketId"
          @select-revision="handleSelectRevision"
          @restored="() => console.log('Revision restored')"
        />
      </aside>
    </div>
  </SectionCard>
</template>

<style scoped>
.editor-wrapper {
  position: relative;
  height: auto;
  width: 100%;
  overflow: visible;
}
</style> 