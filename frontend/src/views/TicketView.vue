<script setup lang="ts">
/// <reference types="node" />
import { computed, onMounted, onUnmounted, watch, ref, reactive } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useAuthStore } from "@/stores/auth";
import { STATUS_OPTIONS, PRIORITY_OPTIONS } from "@/constants/ticketOptions";
import ticketService from "@/services/ticketService";
import { categoryService } from "@/services/categoryService";
import type { TicketCategory } from "@/types/category";
import type { Ticket } from "@/types/ticket";

// Composables
import { useTicketData } from "@/composables/useTicketData";
import { useTicketSSE } from "@/composables/useTicketSSE";
import { useTicketDevices } from "@/composables/useTicketDevices";
import { useTicketRelationships } from "@/composables/useTicketRelationships";
import { useTicketComments } from "@/composables/useTicketComments";
import { useTitleManager } from "@/composables/useTitleManager";
import { useRecentTicketsStore } from "@/stores/recentTickets";
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
import SidebarAddMenu from "@/components/ticketComponents/SidebarAddMenu.vue";
import type { SidebarAddMenuItem } from "@/components/ticketComponents/SidebarAddMenu.vue";
import BackButton from "@/components/common/BackButton.vue";
import DeleteButton from "@/components/common/DeleteButton.vue";
import NotFoundIllustration from "@/components/common/NotFoundIllustration.vue";
import PluginSlot from "@/plugins/components/PluginSlot.vue";
import { getActionRegistrations } from "@/plugins/loader";


const route = useRoute();
const router = useRouter();
const authStore = useAuthStore();
const titleManager = useTitleManager();

// Ticket data management
const {
    ticket,
    loading,
    error,
    selectedStatus,
    selectedPriority,
    selectedCategory,
    formattedCreatedDate,
    formattedModifiedDate,
    comments,
    devices,
    fetchTicket,
    refreshTicket,
    updateStatus,
    updatePriority,
    updateCategory,
    updateRequester,
    updateAssignee,
    updateTitle,
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

// Unified "+ Add" menu state
const pluginActionActivatedMap = reactive(new Map<string, number>());

const sidebarAddItems = computed<SidebarAddMenuItem[]>(() => {
    const items: SidebarAddMenuItem[] = [
        { id: 'device', label: 'Add device', type: 'native', icon: 'M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z' },
        { id: 'linked-ticket', label: 'Link ticket', type: 'native', icon: 'M13.828 10.172a4 4 0 00-5.656 0l-4 4a4 4 0 105.656 5.656l1.102-1.101m-.758-4.899a4 4 0 005.656 0l4-4a4 4 0 00-5.656-5.656l-1.1 1.1' },
        { id: 'project', label: 'Add to project', type: 'native', icon: 'M4 4h4v16H4V4zm6 0h4v12h-4V4zm6 0h4v8h-4V4z' },
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

const handleSidebarAddAction = (itemId: string) => {
    if (itemId === 'device') {
        showDeviceModal.value = true;
    } else if (itemId === 'linked-ticket') {
        showLinkedTicketModal.value = true;
    } else if (itemId === 'project') {
        showProjectModal.value = true;
    } else if (itemId.startsWith('plugin:')) {
        // Increment activation counter for the plugin component
        const key = itemId.replace('plugin:', '');
        const current = pluginActionActivatedMap.get(key) || 0;
        pluginActionActivatedMap.set(key, current + 1);
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
defineExpose({
    handleCreateTicket,
});
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
                        <BackButton
                            v-if="ticket.project"
                            context="project"
                            :contextId="ticket.project"
                            :fallbackRoute="'/tickets'"
                        />
                        <BackButton v-else fallbackRoute="/tickets" />

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
                <div class="ticket-grid gap-6 items-start">
                    <!-- Left Column Wrapper (for 2-column tablet layout) -->
                    <div class="ticket-left-column">
                        <!-- Details Sidebar -->
                        <div class="ticket-details flex flex-col gap-3">

                        <!-- Skeleton: Details -->
                        <div v-if="!ticket" class="bg-surface rounded-xl border border-default overflow-hidden">
                            <!-- Header (matches SectionCard) -->
                            <div class="bg-surface-alt border-b border-default px-4 py-3">
                                <h2 class="text-lg font-medium text-primary">Ticket Details</h2>
                            </div>
                            <!-- Content (matches TicketDetails inner layout) -->
                            <div class="p-3 flex flex-col gap-3">
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
                        </div>

                        <TicketDetails
                            v-else
                            :ticket="ticket"
                            :created-date="formattedCreatedDate"
                            :modified-date="formattedModifiedDate"
                            :selected-status="selectedStatus"
                            :selected-priority="selectedPriority"
                            :selected-category="selectedCategory"
                            :status-options="STATUS_OPTIONS"
                            :priority-options="PRIORITY_OPTIONS"
                            :category-options="categoryOptions"
                            @update:selectedStatus="updateStatus"
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
                                        <svg class="w-3.5 h-3.5" viewBox="0 0 20 20" fill="currentColor">
                                            <path d="M10 3a1 1 0 011 1v5h5a1 1 0 110 2h-5v5a1 1 0 11-2 0v-5H4a1 1 0 110-2h5V4a1 1 0 011-1z" />
                                        </svg>
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
                                        v-for="projectId in ticket.projects"
                                        :key="projectId"
                                        :project-id="projectId"
                                        @view="viewProject(projectId)"
                                        @remove="() => removeFromProject(projectId)"
                                    />
                                </div>
                            </SidebarSection>

                            <!-- Plugin Components -->
                            <PluginSlot slot-name="ticket-sidebar" :ticket="ticket" :actionActivatedMap="pluginActionActivatedMap" />
                        </template>
                        </div>

                        <!-- Comments (inside left-column for tablet 2-col layout) -->
                        <!-- Skeleton: Comments (matches CommentsAndAttachments / SectionCard) -->
                        <div v-if="!ticket" class="ticket-comments rounded-xl print:hidden">
                            <div class="bg-surface rounded-xl border border-default overflow-hidden">
                                <!-- Header (matches SectionCard) -->
                                <div class="bg-surface-alt border-b border-default px-4 py-3">
                                    <h2 class="text-lg font-medium text-primary">Comments and Attachments</h2>
                                </div>
                                <!-- Content -->
                                <div class="p-3 flex flex-col gap-3">
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
                            </div>
                        </div>
                        <!-- Hidden on print if no comments exist -->
                        <div
                            v-else
                            class="ticket-comments rounded-xl"
                            :class="{ 'print:hidden': !hasCommentsWithContent }"
                        >
                            <CommentsAndAttachments
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

                    <!-- Article -->
                    <!-- Skeleton: Article (matches CollaborativeTicketArticle) -->
                    <div v-if="!ticket" class="ticket-article rounded-xl print:hidden">
                        <div class="bg-surface rounded-xl border border-default flex flex-col w-full h-auto overflow-hidden">
                            <!-- Header -->
                            <div class="px-4 py-3 bg-surface-alt border-b border-default flex justify-between items-center">
                                <h2 class="text-lg font-medium text-primary">Ticket Notes</h2>
                                <div class="flex items-center gap-2">
                                    <div class="w-8 h-8 rounded-md bg-surface-hover animate-pulse"></div>
                                    <div class="w-8 h-8 rounded-md bg-surface-hover animate-pulse"></div>
                                    <div class="w-8 h-8 rounded-md bg-surface-hover animate-pulse"></div>
                                </div>
                            </div>
                            <!-- Content area (matches min-h-[300px]) -->
                            <div class="flex-grow min-h-[300px] p-4 flex flex-col gap-3">
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
                        </div>
                    </div>
                    <div v-else class="ticket-article rounded-xl">
                        <CollaborativeTicketArticle
                            :key="`article-${ticket.id}`"
                            :initial-content="ticket.article_content || ''"
                            :ticket-id="ticket.id"
                        />
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
/* Mobile: Single column, wrapper dissolves so items stack naturally */
.ticket-grid {
    display: flex;
    flex-direction: column;
    gap: 1.5rem;
    width: 100%;
}

.ticket-left-column {
    display: contents; /* Dissolve wrapper on mobile */
}

.ticket-details,
.ticket-article,
.ticket-comments {
    min-width: 0; /* Prevent overflow */
    width: 100%;
}

/* Mobile ordering: details → article → comments */
.ticket-details {
    order: 1;
}

.ticket-article {
    order: 2;
}

.ticket-comments {
    order: 3;
}

/* Tablet (lg): 2 columns using flexbox - no row alignment issues */
@media (min-width: 1024px) {
    .ticket-grid {
        flex-direction: row;
        align-items: flex-start;
    }

    .ticket-left-column {
        display: flex;
        flex-direction: column;
        gap: 1.5rem;
        flex: 1 1 0;
        max-width: 420px;
        min-width: 340px;
        order: 1; /* Left column first */
    }

    .ticket-details,
    .ticket-comments {
        width: 100%;
        order: unset; /* Reset mobile ordering */
    }

    .ticket-article {
        flex: 1.5 1 0;
        min-width: 0;
        order: 2; /* Article second */
    }
}

/* Desktop (xl): 3 columns with grid, wrapper dissolves */
@media (min-width: 1536px) {
    .ticket-grid {
        display: grid;
        grid-template-columns: minmax(350px, 1fr) minmax(0, 1.5fr) minmax(350px, 1fr);
    }

    .ticket-left-column {
        display: contents; /* Dissolve wrapper so details and comments become separate grid items */
    }

    .ticket-details,
    .ticket-article,
    .ticket-comments {
        width: auto; /* Reset width for grid */
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
