<script setup lang="ts">
/// <reference types="node" />
import { computed, onMounted, onUnmounted, watch, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useAuthStore } from "@/stores/auth";
import { STATUS_OPTIONS, PRIORITY_OPTIONS } from "@/constants/ticketOptions";
import ticketService from "@/services/ticketService";
import { stripHtml } from "@/composables/useSanitise";
import { categoryService } from "@/services/categoryService";
import type { TicketCategory } from "@/types/category";
import type { Ticket } from "@/types/ticket";
import Icon from "@/components/common/Icon.vue";

// Composables
import { useTicketData } from "@/composables/useTicketData";
import { useTicketUiStore } from "@/stores/ticketUi";
import { useTicketSSE } from "@/composables/useTicketSSE";
import { useTicketDevices } from "@/composables/useTicketDevices";
import { useTicketRelationships } from "@/composables/useTicketRelationships";
import { useTicketComments } from "@/composables/useTicketComments";
import { useTitleManager } from "@/composables/useTitleManager";
import { useTicketDrag } from "@/composables/useTicketDrag";
import { parseTicketUrl } from "@/components/editor/ticketLinkPlugin";

// Components
import CollaborativeTicketArticle from "@/components/ticketComponents/CollaborativeTicketArticle.vue";
import TicketDetails from "@/components/ticketComponents/TicketDetails.vue";
import DeviceDetails from "@/components/ticketComponents/DeviceDetails.vue";
import DeviceSelectionModal from "@/components/ticketComponents/DeviceSelectionModal.vue";
import CommentsAndAttachments from "@/components/ticketComponents/CommentsAndAttachments.vue";
import LinkedTicketModal from "@/components/ticketComponents/LinkedTicketModal.vue";
import LinkedTicketPreview from "@/components/ticketComponents/LinkedTicketPreview.vue";
import ProjectSelectionModal from "@/components/ticketComponents/ProjectSelectionModal.vue";
import ProjectInfo from "@/components/ticketComponents/ProjectInfo.vue";
import SidebarSection from "@/components/ticketComponents/SidebarSection.vue";
import TicketLinkedDocs from "@/components/ticketComponents/TicketLinkedDocs.vue";
import TicketGapFlag from "@/components/ticketComponents/TicketGapFlag.vue";
import documentationService from "@/services/documentationService";
import { docUrl } from "@/utils/docUrl";
import { pageTicketLinkKeys } from "@/composables/usePageTicketLinks";
import { useFlagTicketMutation } from "@/composables/useKnowledgeGaps";
import { useQueryCache } from "@pinia/colada";
import SidebarAddMenu from "@/components/ticketComponents/SidebarAddMenu.vue";
import type { SidebarAddMenuItem } from "@/components/ticketComponents/SidebarAddMenu.vue";
import BackButton from "@/components/common/BackButton.vue";
import DeleteButton from "@/components/common/DeleteButton.vue";
import NotFoundIllustration from "@/components/common/NotFoundIllustration.vue";
import SectionCard from "@/components/common/SectionCard.vue";
import PluginSlot from "@/plugins/components/PluginSlot.vue";
import { getActionRegistrations } from "@/plugins/loader";
import { usePageCreateAction } from "@/composables/usePageCreateAction";


const route = useRoute();
const router = useRouter();
const authStore = useAuthStore();
const titleManager = useTitleManager();

// Ticket data management
const {
    ticket,
    error,
    selectedStatus,
    selectedPriority,
    selectedCategory,
    selectedWorkflowStateId,
    formattedCreatedDate,
    formattedModifiedDate,
    comments,
    devices,
    fetchTicket,
    refreshTicket,
    updateStatus,
    updateWorkflowState,
    updatePriority,
    updateCategory,
    updateRequester,
    updateAssignee,
    deleteTicket,
} = useTicketData();

// Categories
const categories = ref<TicketCategory[]>([]);
const categoryOptions = computed(() => [
    { value: '', label: 'No category' },
    ...categories.value.map(cat => ({
        value: String(cat.id),
        label: cat.name,
        color: cat.color || undefined
    }))
]);

const loadCategories = async () => {
    try {
        categories.value = await categoryService.getCategories();
    } catch (err) {
        console.error('Failed to load categories:', err);
    }
};

// SSE real-time updates
const ticketId = computed(() =>
    route.params.id ? Number(route.params.id) : undefined,
);
const {
    isConnected,
    recentlyAddedCommentIds,
    activeViewerCount,
    startEditing,
    stopEditing,
} = useTicketSSE(
    ticket,
    ticketId,
    selectedStatus,
    selectedPriority,
);

// Device management - uses centralized mutations with optimistic updates
const { showDeviceModal, addDevice, removeDevice } =
    useTicketDevices(ticket);

// Relationships (linked tickets & projects) - SSE handles state updates
const {
    showLinkedTicketModal,
    showProjectModal,
    linkTicket,
    unlinkTicket,
    addToProject,
    removeFromProject,
} = useTicketRelationships(ticket);

// Drag-to-link state
const { dragState } = useTicketDrag();
const isLinkDropTarget = ref(false);
let inactiveTimeout: ReturnType<typeof setTimeout> | null = null;

// Show drop zone affordance when dragging a ticket (that isn't this ticket)
const showDropAffordance = computed(() => {
    return dragState.value.isDragging &&
           dragState.value.ticket?.id !== ticket.value?.id;
});

// Unified "+ Add" menu state — plugin activation counters live in
// `useTicketUiStore` keyed by ticket id so they survive component
// unmount (Phase 4 will drop KeepAlive on TicketView; the store is
// the new home for this state). Falls back to an empty map for
// pre-route or transitional states.
const ticketUi = useTicketUiStore();
const pluginActionActivatedMap = computed(() =>
    ticketId.value !== undefined
        ? ticketUi.getPluginActivations(ticketId.value)
        : new Map<string, number>()
);

const sidebarAddItems = computed<SidebarAddMenuItem[]>(() => {
    const items: SidebarAddMenuItem[] = [
        { id: 'device', label: 'Add device', type: 'native', icon: 'M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z' },
        { id: 'linked-ticket', label: 'Link ticket', type: 'native', icon: 'M13.828 10.172a4 4 0 00-5.656 0l-4 4a4 4 0 105.656 5.656l1.102-1.101m-.758-4.899a4 4 0 005.656 0l4-4a4 4 0 00-5.656-5.656l-1.1 1.1' },
        { id: 'project', label: 'Add to project', type: 'native', icon: 'M4 4h4v16H4V4zm6 0h4v12h-4V4zm6 0h4v8h-4V4z' },
        { id: 'save-as-doc', label: 'Save as doc', type: 'native', icon: 'M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z' },
        { id: 'flag-for-docs', label: 'Flag for documentation', type: 'native', icon: 'M3 21v-4m0 0V5a2 2 0 012-2h6.5l1 1H21l-3 6 3 6h-8.5l-1-1H5a2 2 0 00-2 2zm9-13.5V9' },
    ];

    for (const action of getActionRegistrations('ticket-sidebar')) {
        items.push({
            id: `plugin:${action.pluginUuid}:${action.componentName}`,
            label: action.label,
            type: 'plugin',
            pluginName: action.componentLabel || action.pluginName,
            icon: action.icon,
        });
    }

    return items;
});

const queryCache = useQueryCache();
const flagMutation = useFlagTicketMutation();

/** Mark this ticket as a knowledge gap. Idempotent: the backend
 *  attaches a fresh manual_flag signal to an existing open gap if
 *  one already covers the ticket, otherwise creates a new gap. */
const handleFlagForDocs = async () => {
    if (ticketId.value === undefined) return;
    await flagMutation.mutateAsync({ ticketId: ticketId.value });
};

/**
 * Promote the current ticket to a documentation page. Creates a
 * draft page anchored to this ticket via a 'resolves' link
 * (the backend handler does both in one call), invalidates the
 * ticket's linked-docs query so the sidebar refreshes, and routes
 * the user to the new doc so they can edit straight away.
 */
const handleSaveAsDoc = async () => {
    if (!ticket.value || ticketId.value === undefined) return;
    const titleSeed = ticket.value.title?.trim() || `Ticket #${ticketId.value}`;
    const created = await documentationService.createPageFromTicket(ticketId.value, {
        title: titleSeed,
        icon: '📄',
    });
    if (!created) return;
    queryCache.invalidateQueries({
        key: pageTicketLinkKeys.forTicket(ticketId.value),
    });
    router.push(docUrl({ slug: created.slug, id: created.id as number }));
};

const handleSidebarAddAction = (itemId: string) => {
    if (itemId === 'device') {
        showDeviceModal.value = true;
    } else if (itemId === 'linked-ticket') {
        showLinkedTicketModal.value = true;
    } else if (itemId === 'project') {
        showProjectModal.value = true;
    } else if (itemId === 'save-as-doc') {
        handleSaveAsDoc();
    } else if (itemId === 'flag-for-docs') {
        handleFlagForDocs();
    } else if (itemId.startsWith('plugin:') && ticketId.value !== undefined) {
        const key = itemId.replace('plugin:', '');
        ticketUi.activatePluginAction(ticketId.value, key);
    }
};

// Check if there are any comments with actual content (for print visibility)
const hasCommentsWithContent = computed(() => {
    if (!comments.value || comments.value.length === 0) return false;
    // Check if any comment has content or attachments
    return comments.value.some(comment =>
        (comment.content && comment.content.trim().length > 0) ||
        (comment.attachments && comment.attachments.length > 0)
    );
});

// Print visibility for the ticket article. Tickets routinely have no
// running notes — printing the empty article card just steals space
// from the comment timeline. We use the shared `stripHtml` helper
// (DOMPurify-backed, decodes entities, handles malformed markup
// gracefully) so an editor "empty" representation like `<p></p>` or
// `<p>&nbsp;</p>` correctly reads as empty.
const hasArticleContent = computed(() => {
    const raw = ticket.value?.article_content;
    if (!raw) return false;
    return stripHtml(raw).trim().length > 0;
});

const setDropTargetActive = () => {
    isLinkDropTarget.value = true;
    if (inactiveTimeout) {
        clearTimeout(inactiveTimeout);
        inactiveTimeout = null;
    }
};

const setDropTargetInactive = () => {
    // Debounce to prevent flickering when moving over child elements
    inactiveTimeout = setTimeout(() => {
        isLinkDropTarget.value = false;
    }, 50);
};

const resetDropState = () => {
    isLinkDropTarget.value = false;
    if (inactiveTimeout) {
        clearTimeout(inactiveTimeout);
        inactiveTimeout = null;
    }
};

const handleLinkDrop = async (event: DragEvent) => {
    event.preventDefault();
    resetDropState();

    // Try to get ticket ID from JSON data first
    let droppedTicketId: number | null = null;

    const jsonData = event.dataTransfer?.getData('application/json');
    if (jsonData) {
        try {
            const data = JSON.parse(jsonData);
            if (data.ticketId) {
                droppedTicketId = data.ticketId;
            }
        } catch {
            // Invalid JSON
        }
    }

    // Fallback to URL parsing
    if (!droppedTicketId) {
        const text = event.dataTransfer?.getData('text/plain');
        if (text) {
            droppedTicketId = parseTicketUrl(text.trim());
        }
    }

    if (droppedTicketId && droppedTicketId !== ticket.value?.id) {
        await linkTicket(droppedTicketId);
    }
};

// Document-level dragend cleanup (fires when drag ends anywhere)
const handleDocumentDragEnd = () => {
    resetDropState();
};

onMounted(() => {
    document.addEventListener('dragend', handleDocumentDragEnd);

    // Register save handler for SiteHeader title edits
    titleManager.onTicketTitleSave(async (title: string) => {
        if (ticket.value) {
            await ticketService.updateTicket(ticket.value.id, { title });
        }
    });
});

onUnmounted(() => {
    document.removeEventListener('dragend', handleDocumentDragEnd);
    titleManager.onTicketTitleSave(null);
    if (inactiveTimeout) {
        clearTimeout(inactiveTimeout);
    }
});

// Comments
const { addComment, deleteAttachment, deleteComment } = useTicketComments(
    ticket,
    refreshTicket,
);

// Debounced backend save for title
let titleUpdateTimeout: NodeJS.Timeout | null = null;
let lastSavedTitle: string | null = null;

// Called when user starts editing the title (focus)
const handleTitleFocus = () => {
    startEditing('title');
};

// Called when user stops editing the title (blur)
const handleTitleBlur = () => {
    stopEditing('title');
    lastSavedTitle = null; // Reset for next edit session
};

const handleTitleUpdate = (newTitle: string) => {
    // Update local ticket immediately for instant UI feedback
    if (ticket.value) {
        // Store the last saved title on first edit
        if (lastSavedTitle === null) {
            lastSavedTitle = ticket.value.title;
        }

        // Update locally immediately
        ticket.value.title = newTitle;

        // Update title manager immediately so header updates
        titleManager.setTicket(ticket.value);
    }

    // Clear any pending backend save
    if (titleUpdateTimeout) {
        clearTimeout(titleUpdateTimeout);
    }

    // Debounce the backend save (300ms)
    titleUpdateTimeout = setTimeout(async () => {
        if (ticket.value && lastSavedTitle !== newTitle) {
            try {
                // Call the API directly without reverting local state
                await ticketService.updateTicket(ticket.value.id, { title: newTitle });

                // Update our saved reference
                lastSavedTitle = newTitle;
            } catch (error) {
                console.error('Failed to save title:', error);
            }
        }
    }, 300);
};

// Emit ticket updates - pass the full reactive ticket object
const emit = defineEmits<{
    (e: "update:ticket", ticket: Ticket | null): void;
}>();

watch(
    ticket,
    (newTicket) => {
        // Only emit when valid ticket data exists - prevents title flash during loading
        // Clearing is handled by App.vue on route leave
        if (newTicket) {
            emit("update:ticket", newTicket);
        }
    },
    { immediate: true, deep: true }, // deep: true to watch nested property changes
);

// Navigation
function navigateToDeviceView(deviceId: number): void {
    router.push({
        path: `/devices/${deviceId}`,
        query: { fromTicket: String(ticket.value?.id) },
    });
}

function viewProject(projectId: string): void {
    router.push(`/projects/${projectId}`);
}

// Load ticket on mount and route change
onMounted(async () => {
    // Load categories in parallel with ticket
    loadCategories();

    if (route.params.id) {
        await fetchTicket(route.params.id);
    }
});

watch(
    () => route.params.id,
    async (newId) => {
        if (newId) {
            await fetchTicket(newId);
        }
    },
);

// Create ticket handler for SiteHeader button
const handleCreateTicket = async () => {
    try {
        const newTicket = await ticketService.createEmptyTicket();
        router.push(`/tickets/${newTicket.id}`);
    } catch (error) {
        console.error("Failed to create empty ticket:", error);
    }
};

// Expose methods for parent component access (SiteHeader create button)
usePageCreateAction(handleCreateTicket);
</script>

<template>
    <div class="flex-1">
        <!-- Error state -->
        <div v-if="error" class="flex flex-col items-center justify-center min-h-[calc(100vh-8rem)] px-4 gap-4">
            <NotFoundIllustration />
            <router-link
                to="/tickets"
                class="px-4 py-2 bg-accent text-white rounded-lg hover:bg-accent-hover transition-colors"
            >
                Back to Tickets
            </router-link>
        </div>

        <!-- Ticket content (layout always rendered; skeletons swap to real components) -->
        <div v-else class="flex flex-col">
            <!-- Navigation and actions bar (hidden on print) -->
            <div class="print:hidden pt-4 px-4 sm:px-6 flex justify-between items-center">
                <div class="flex items-center gap-4">
                    <template v-if="ticket">
                        <BackButton fallbackRoute="/tickets" />

                        <!-- SSE Connection Status -->
                        <div class="flex items-center gap-2 text-sm">
                            <div
                                class="w-2 h-2 rounded-full"
                                :class="{
                                    'bg-status-success': isConnected,
                                    'bg-status-warning animate-pulse': !isConnected,
                                }"
                            ></div>
                            <span class="text-secondary">
                                {{ isConnected ? "Live updates" : "Connecting..." }}
                            </span>
                            <span v-if="activeViewerCount > 0" class="text-secondary ml-2">
                                <span class="text-accent">{{ activeViewerCount }}</span> viewing
                            </span>
                        </div>
                    </template>
                    <!-- Skeleton back button -->
                    <div v-else class="h-8 w-24 bg-surface-alt rounded-lg animate-pulse"></div>
                </div>

                <DeleteButton
                    v-if="ticket"
                    fallbackRoute="/tickets"
                    itemName="Ticket"
                    @delete="deleteTicket"
                />
            </div>

            <div class="flex flex-col gap-4 px-4 py-4 sm:px-6 mx-auto w-full max-w-8xl">
                <!-- Grid Container with named areas -->
                <div class="ticket-grid items-start">
                    <!-- Narrow column: metadata + sidebar sections. -->
                    <div class="ticket-details-column">
                        <!-- Details Sidebar -->
                        <div class="ticket-details flex flex-col gap-3">

                        <!-- Skeleton: Details -->
                        <SectionCard v-if="!ticket" content-padding="p-3">
                            <template #title>Ticket Details</template>
                            <!-- Content (matches TicketDetails inner layout) -->
                            <div class="flex flex-col gap-3">
                                <!-- Title -->
                                <div class="flex flex-col gap-1.5">
                                    <h3 class="text-xs font-medium text-tertiary uppercase tracking-wide">Title</h3>
                                    <div class="bg-surface-alt rounded-lg border border-subtle min-h-[1.75rem] px-2 py-1">
                                        <div class="h-4 w-3/4 bg-surface-hover rounded animate-pulse"></div>
                                    </div>
                                </div>
                                <!-- Requester / Assignee -->
                                <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
                                    <div class="flex flex-col gap-1.5">
                                        <h3 class="text-xs font-medium text-tertiary uppercase tracking-wide">Requester</h3>
                                        <div class="bg-surface-alt rounded-lg border border-subtle min-h-[44px] sm:min-h-[40px] flex items-center px-2.5 sm:px-3">
                                            <div class="w-7 h-7 sm:w-6 sm:h-6 rounded-full bg-surface-hover animate-pulse shrink-0"></div>
                                            <div class="h-4 w-20 bg-surface-hover rounded animate-pulse ml-2"></div>
                                        </div>
                                    </div>
                                    <div class="flex flex-col gap-1.5">
                                        <h3 class="text-xs font-medium text-tertiary uppercase tracking-wide">Assignee</h3>
                                        <div class="bg-surface-alt rounded-lg border border-subtle min-h-[44px] sm:min-h-[40px] flex items-center px-2.5 sm:px-3">
                                            <div class="w-7 h-7 sm:w-6 sm:h-6 rounded-full bg-surface-hover animate-pulse shrink-0"></div>
                                            <div class="h-4 w-20 bg-surface-hover rounded animate-pulse ml-2"></div>
                                        </div>
                                    </div>
                                </div>
                                <!-- Status / Priority -->
                                <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
                                    <div class="flex flex-col gap-1.5">
                                        <h3 class="text-xs font-medium text-tertiary uppercase tracking-wide">Status</h3>
                                        <div class="bg-surface-alt rounded-lg border border-subtle min-h-[44px] sm:min-h-[40px] flex items-center px-3">
                                            <div class="h-4 w-16 bg-surface-hover rounded animate-pulse"></div>
                                        </div>
                                    </div>
                                    <div class="flex flex-col gap-1.5">
                                        <h3 class="text-xs font-medium text-tertiary uppercase tracking-wide">Priority</h3>
                                        <div class="bg-surface-alt rounded-lg border border-subtle min-h-[44px] sm:min-h-[40px] flex items-center px-3">
                                            <div class="h-4 w-16 bg-surface-hover rounded animate-pulse"></div>
                                        </div>
                                    </div>
                                </div>
                                <!-- Category -->
                                <div class="flex flex-col gap-1.5">
                                    <h3 class="text-xs font-medium text-tertiary uppercase tracking-wide">Category</h3>
                                    <div class="bg-surface-alt rounded-lg border border-subtle min-h-[44px] sm:min-h-[40px] flex items-center px-3">
                                        <div class="h-4 w-24 bg-surface-hover rounded animate-pulse"></div>
                                    </div>
                                </div>
                                <!-- Timestamps -->
                                <div class="pt-2 border-t border-default">
                                    <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
                                        <div class="flex flex-col gap-1">
                                            <span class="text-xs text-tertiary uppercase tracking-wide font-medium">Created</span>
                                            <div class="h-5 w-28 bg-surface-hover rounded animate-pulse"></div>
                                        </div>
                                        <div class="flex flex-col gap-1">
                                            <span class="text-xs text-tertiary uppercase tracking-wide font-medium">Last Modified</span>
                                            <div class="h-5 w-28 bg-surface-hover rounded animate-pulse"></div>
                                        </div>
                                    </div>
                                </div>
                            </div>
                        </SectionCard>

                        <TicketDetails
                            v-else
                            :ticket="ticket"
                            :created-date="formattedCreatedDate"
                            :modified-date="formattedModifiedDate"
                            :selected-status="selectedStatus"
                            :selected-priority="selectedPriority"
                            :selected-category="selectedCategory"
                            :selected-workflow-state-id="selectedWorkflowStateId"
                            :status-options="STATUS_OPTIONS"
                            :priority-options="PRIORITY_OPTIONS"
                            :category-options="categoryOptions"
                            @update:selectedStatus="updateStatus"
                            @update:selectedWorkflowStateId="updateWorkflowState"
                            @update:selectedPriority="updatePriority"
                            @update:selectedCategory="updateCategory"
                            @update:requester="updateRequester"
                            @update:assignee="updateAssignee"
                            @update:title="handleTitleUpdate"
                            @titleFocus="handleTitleFocus"
                            @titleBlur="handleTitleBlur"
                        />

                        <!-- Sidebar sections (hidden during loading) -->
                        <template v-if="ticket">
                            <!-- Unified "+ Add" menu -->
                            <SidebarAddMenu
                                :items="sidebarAddItems"
                                @select="handleSidebarAddAction"
                            />

                            <!-- Devices -->
                            <SidebarSection
                                title="Devices"
                                add-label="Add device"
                                :has-items="devices.length > 0"
                                hide-empty-state
                                @add="showDeviceModal = true"
                            >
                                <div class="flex flex-col gap-2">
                                    <DeviceDetails
                                        v-for="device in devices"
                                        :key="device.id"
                                        :device="device"
                                        @remove="() => removeDevice(device.id)"
                                        @view="navigateToDeviceView"
                                    />
                                </div>
                            </SidebarSection>

                            <!-- Linked Tickets (drop zone) - hidden on print when no linked tickets -->
                            <div
                                @dragenter.prevent="setDropTargetActive"
                                @dragover.prevent="setDropTargetActive"
                                @dragleave.prevent="setDropTargetInactive"
                                @drop.prevent="handleLinkDrop"
                                class="flex flex-col gap-2"
                                :class="{ 'print:hidden': !ticket.linkedTickets?.length }"
                            >
                                <!-- Header (only when has tickets) -->
                                <div v-if="ticket.linkedTickets?.length" class="flex items-center justify-between">
                                    <h3 class="text-sm font-medium text-secondary">Linked Tickets</h3>
                                    <button
                                        @click="showLinkedTicketModal = true"
                                        class="print:hidden flex items-center gap-1 text-xs font-medium text-tertiary hover:text-accent transition-colors"
                                    >
                                        <Icon name="add" />
                                        Add
                                    </button>
                                </div>

                                <!-- Drop zone (single instance, shown when dragging) - hidden on print -->
                                <div
                                    v-if="showDropAffordance || isLinkDropTarget"
                                    class="print:hidden rounded-lg border-2 border-dashed p-3 text-center text-sm transition-colors"
                                    :class="isLinkDropTarget
                                        ? 'border-accent bg-accent/10 text-accent'
                                        : 'border-accent/40 text-accent/70'"
                                >
                                    <template v-if="isLinkDropTarget && dragState.ticket">
                                        <span class="font-mono">#{{ dragState.ticket.id }}</span>
                                        {{ dragState.ticket.title }}
                                    </template>
                                    <template v-else>Drop to link ticket</template>
                                </div>

                                <!-- Existing linked tickets -->
                                <LinkedTicketPreview
                                    v-for="linkedId in ticket.linkedTickets"
                                    :key="linkedId"
                                    :linked-ticket-id="linkedId"
                                    :current-ticket-id="ticket.id"
                                    @unlink="() => unlinkTicket(linkedId)"
                                    @view="() => {}"
                                />

                            </div>

                            <!-- Projects -->
                            <SidebarSection
                                title="Projects"
                                add-label="Add to project"
                                :has-items="!!ticket.projects?.length"
                                hide-empty-state
                                @add="showProjectModal = true"
                            >
                                <div class="flex flex-col gap-2">
                                    <ProjectInfo
                                        v-for="projectId in ticket.projects as string[] | undefined"
                                        :key="projectId"
                                        :project-id="projectId"
                                        @view="viewProject(projectId)"
                                        @remove="() => removeFromProject(projectId)"
                                    />
                                </div>
                            </SidebarSection>

                            <!-- Knowledge gap flag (only renders when flagged) -->
                            <TicketGapFlag :ticket-id="ticket.id" />

                            <!-- Documentation links -->
                            <TicketLinkedDocs
                                :ticket-id="ticket.id"
                                @add="handleSaveAsDoc"
                            />

                            <!-- Plugin Components -->
                            <PluginSlot slot-name="ticket-sidebar" :ticket="ticket" :actionActivatedMap="pluginActionActivatedMap" />
                        </template>
                        </div>
                    </div>

                    <!--
                      Wide content column: ticket article on top, then
                      the comment timeline. The article is the original
                      message a customer sent (when the ticket was
                      created from email) or the running notes for the
                      ticket; visually it's the first entry in the
                      conversation.
                    -->
                    <div class="ticket-content-column">
                        <!-- Article -->
                        <!-- Skeleton: Article (matches CollaborativeTicketArticle) -->
                        <div v-if="!ticket" class="ticket-article rounded-xl print:hidden">
                            <SectionCard content-padding="p-4">
                                <template #title>Ticket Notes</template>
                                <template #headerActions>
                                    <div class="w-5 h-5 rounded bg-surface-hover animate-pulse"></div>
                                    <div class="w-5 h-5 rounded bg-surface-hover animate-pulse"></div>
                                    <div class="w-5 h-5 rounded bg-surface-hover animate-pulse"></div>
                                </template>
                                <!-- Content area (matches min-h-[300px]) -->
                                <div class="flex-grow min-h-[300px] flex flex-col gap-3">
                                    <div class="h-4 w-full bg-surface-hover rounded animate-pulse"></div>
                                    <div class="h-4 w-5/6 bg-surface-hover rounded animate-pulse"></div>
                                    <div class="h-4 w-full bg-surface-hover rounded animate-pulse"></div>
                                    <div class="h-4 w-4/6 bg-surface-hover rounded animate-pulse"></div>
                                    <div class="h-4 w-0"></div>
                                    <div class="h-4 w-full bg-surface-hover rounded animate-pulse"></div>
                                    <div class="h-4 w-3/4 bg-surface-hover rounded animate-pulse"></div>
                                    <div class="h-4 w-full bg-surface-hover rounded animate-pulse"></div>
                                    <div class="h-4 w-2/3 bg-surface-hover rounded animate-pulse"></div>
                                </div>
                            </SectionCard>
                        </div>
                        <div
                            v-else
                            class="ticket-article rounded-xl"
                            :class="{ 'print:hidden': !hasArticleContent }"
                        >
                            <CollaborativeTicketArticle
                                :key="`article-${ticket.id}`"
                                :initial-content="ticket.article_content || ''"
                                :ticket-id="ticket.id"
                            />
                        </div>

                        <!-- Comments timeline -->
                        <!-- Skeleton: Comments (matches CommentsAndAttachments / SectionCard) -->
                        <div v-if="!ticket" class="ticket-comments rounded-xl print:hidden">
                            <SectionCard content-padding="p-3">
                                <template #title>Comments and Attachments</template>
                                <!-- Content -->
                                <div class="flex flex-col gap-3">
                                    <!-- Comment input (matches SimpleEditor min-height: 60px) -->
                                    <div class="bg-surface rounded-lg" style="min-height: 60px;">
                                        <div class="p-3">
                                            <div class="h-4 w-48 bg-surface-hover rounded animate-pulse"></div>
                                        </div>
                                    </div>
                                    <!-- Button row (matches Add + voice + file buttons) -->
                                    <div class="flex gap-2">
                                        <div class="flex-1 h-10 bg-surface-hover rounded-md animate-pulse"></div>
                                        <div class="h-10 w-11 bg-surface-alt border border-default rounded-md animate-pulse"></div>
                                        <div class="h-10 w-11 bg-surface-alt border border-default rounded-md animate-pulse"></div>
                                    </div>
                                </div>
                            </SectionCard>
                        </div>
                        <!-- Hidden on print if no comments exist -->
                        <div
                            v-else
                            class="ticket-comments rounded-xl"
                            :class="{ 'print:hidden': !hasCommentsWithContent }"
                        >
                            <CommentsAndAttachments
                                :ticket-id="ticketId"
                                :comments="comments"
                                :current-user="
                                    authStore.user?.uuid || 'Unknown User'
                                "
                                :recently-added-comment-ids="
                                    recentlyAddedCommentIds
                                "
                                @add-comment="addComment"
                                @delete-attachment="deleteAttachment"
                                @delete-comment="deleteComment"
                            />
                        </div>
                    </div>
                </div>
            </div>
        </div>

        <!-- Modals (hidden on print) -->
        <div class="print:hidden">
            <DeviceSelectionModal
                v-if="ticket"
                :show="showDeviceModal"
                :current-ticket-id="ticket.id"
                :existing-device-ids="devices.map((d) => d.id)"
                :requester-uuid="ticket.requester"
                @close="showDeviceModal = false"
                @select-device="addDevice"
            />

            <LinkedTicketModal
                v-if="ticket"
                :show="showLinkedTicketModal"
                :current-ticket-id="ticket.id"
                :existing-linked-tickets="ticket.linkedTickets"
                @close="showLinkedTicketModal = false"
                @select-ticket="linkTicket"
            />

            <ProjectSelectionModal
                v-if="ticket"
                :show="showProjectModal"
                :existing-project-ids="
                    ticket.projects?.map(id => Number(id)) || []
                "
                @close="showProjectModal = false"
                @select-project="addToProject"
            />
        </div>
    </div>
</template>

<style scoped>
/*
 * Three breakpoints, three layouts:
 *
 *   Mobile (<1024px)       — single column, details quick-scan first.
 *   Tablet  (1024-1535px)  — two columns: details left, conversation
 *                            (article + comments folded) right wide.
 *   Desktop (≥1536px)      — three columns: details | article | comments
 *                            with comments taking the wide track. The
 *                            content-column wrapper dissolves via
 *                            `display: contents` so article and comments
 *                            become first-class grid items again.
 *
 * At every breakpoint the conversation gets the widest available
 * track. The earlier 3-column layout split it across two narrow
 * columns; this one keeps the metadata slim on the left and lets
 * the content stretch.
 */
.ticket-grid {
    display: flex;
    flex-direction: column;
    gap: 1.5rem;
    width: 100%;
}

.ticket-content-column,
.ticket-details-column {
    display: flex;
    flex-direction: column;
    gap: 1.5rem;
    min-width: 0;
    width: 100%;
}

/* Mobile order: details quick-scan first, then the conversation. */
.ticket-details-column { order: 1; }
.ticket-content-column { order: 2; }

/* Tablet (lg): 2 columns — details left, conversation right. */
@media (min-width: 1024px) {
    .ticket-grid {
        flex-direction: row;
        align-items: flex-start;
    }

    /* Narrow sidebar column on the left. Fixed-width, not a
     * fraction — metadata fields look cramped when scaled wide, and
     * pinning the width keeps the content column's growth
     * predictable. */
    .ticket-details-column {
        flex: 0 0 360px;
        max-width: 360px;
        min-width: 320px;
        order: 1;
    }

    /* Wide content column on the right, takes whatever's left after
     * the sidebar. `min-width: 0` so children that overflow (an
     * email body with a wide table, a `<pre>` with a long line)
     * scroll inside their container instead of pushing the column. */
    .ticket-content-column {
        flex: 1 1 0;
        min-width: 0;
        order: 2;
    }
}

/* Desktop (xl): 3 columns — details | article | comments. The two
 * column wrappers dissolve via `display: contents` so the article
 * and comments inside the content column become direct grid items
 * with their own tracks. */
@media (min-width: 1536px) {
    .ticket-grid {
        display: grid;
        grid-template-columns: 360px minmax(420px, 1fr) minmax(0, 1.5fr);
        gap: 1.5rem;
    }

    .ticket-content-column,
    .ticket-details-column {
        display: contents;
    }

    .ticket-details,
    .ticket-article,
    .ticket-comments {
        width: auto; /* override the 100% from the flex layout */
        min-width: 0;
    }

    .ticket-details {
        grid-column: 1;
        grid-row: 1;
    }
    .ticket-article {
        grid-column: 2;
        grid-row: 1;
    }
    .ticket-comments {
        grid-column: 3;
        grid-row: 1;
    }
}
</style>
