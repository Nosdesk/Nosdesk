<script setup lang="ts">
/// <reference types="node" />
import { computed, onMounted, onUnmounted, watch, ref } from "vue";
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
import BackButton from "@/components/common/BackButton.vue";
import DeleteButton from "@/components/common/DeleteButton.vue";
import NotFoundIllustration from "@/components/common/NotFoundIllustration.vue";
import PluginSlot from "@/plugins/components/PluginSlot.vue";

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
const { isConnected, recentlyAddedCommentIds, activeViewerCount } = useTicketSSE(
    ticket,
    ticketId,
    selectedStatus,
    selectedPriority,
);

// Device management
const { showDeviceModal, addDevice, removeDevice, updateDeviceField } =
    useTicketDevices(ticket, refreshTicket);

// Relationships (linked tickets & projects)
const {
    showLinkedTicketModal,
    showProjectModal,
    linkTicket,
    unlinkTicket,
    addToProject,
    removeFromProject,
} = useTicketRelationships(ticket, refreshTicket);

// Drag-to-link state
const { dragState } = useTicketDrag();
const isLinkDropTarget = ref(false);
let inactiveTimeout: ReturnType<typeof setTimeout> | null = null;

// Show drop zone affordance when dragging a ticket (that isn't this ticket)
const showDropAffordance = computed(() => {
    return dragState.value.isDragging &&
           dragState.value.ticket?.id !== ticket.value?.id;
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
});

onUnmounted(() => {
    document.removeEventListener('dragend', handleDocumentDragEnd);
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
        const fromRecent = route.query.fromRecent === "true";
        await fetchTicket(route.params.id, fromRecent);
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
        <!-- Loading state -->
        <div v-if="loading" class="flex items-center justify-center h-64">
            <div class="animate-spin rounded-full h-8 w-8 border-2 border-accent border-t-transparent"></div>
        </div>

        <!-- Error state -->
        <div v-else-if="error" class="flex flex-col items-center justify-center min-h-[calc(100vh-8rem)] px-4 gap-4">
            <NotFoundIllustration />
            <router-link
                to="/tickets"
                class="px-4 py-2 bg-accent text-white rounded-lg hover:bg-accent-hover transition-colors"
            >
                Back to Tickets
            </router-link>
        </div>

        <!-- Ticket content -->
        <div v-else-if="ticket" class="flex flex-col">
            <!-- Navigation and actions bar -->
            <div class="pt-4 px-4 sm:px-6 flex justify-between items-center">
                <div class="flex items-center gap-4">
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
                </div>

                <DeleteButton
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
                        <div class="ticket-details flex flex-col gap-6">
                        <TicketDetails
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
                        />

                        <!-- Devices -->
                        <div v-if="devices.length" class="flex flex-col gap-2">
                            <div class="flex items-center justify-between">
                                <h3 class="text-sm font-medium text-secondary">
                                    Devices
                                </h3>
                                <a
                                    href="#"
                                    @click.prevent="showDeviceModal = true"
                                    class="text-accent hover:text-accent/80 text-sm hover:underline"
                                >
                                    + Add device
                                </a>
                            </div>
                            <div class="flex flex-col gap-2">
                                <DeviceDetails
                                    v-for="device in devices"
                                    :key="device.id"
                                    :device="device"
                                    @remove="() => removeDevice(device.id)"
                                    @view="navigateToDeviceView"
                                    @update:name="
                                        (value) =>
                                            updateDeviceField(
                                                device.id,
                                                'name',
                                                value,
                                            )
                                    "
                                    @update:hostname="
                                        (value) =>
                                            updateDeviceField(
                                                device.id,
                                                'hostname',
                                                value,
                                            )
                                    "
                                    @update:serial_number="
                                        (value) =>
                                            updateDeviceField(
                                                device.id,
                                                'serial_number',
                                                value,
                                            )
                                    "
                                    @update:model="
                                        (value) =>
                                            updateDeviceField(
                                                device.id,
                                                'model',
                                                value,
                                            )
                                    "
                                    @update:manufacturer="
                                        (value) =>
                                            updateDeviceField(
                                                device.id,
                                                'manufacturer',
                                                value,
                                            )
                                    "
                                    @update:warranty_status="
                                        (value) =>
                                            updateDeviceField(
                                                device.id,
                                                'warranty_status',
                                                value,
                                            )
                                    "
                                />
                            </div>
                        </div>
                        <div v-else>
                            <a
                                href="#"
                                @click.prevent="showDeviceModal = true"
                                class="block text-accent hover:underline"
                            >
                                + Add device
                            </a>
                        </div>

                        <!-- Linked Tickets (drop zone) -->
                        <div
                            @dragenter.prevent="setDropTargetActive"
                            @dragover.prevent="setDropTargetActive"
                            @dragleave.prevent="setDropTargetInactive"
                            @drop.prevent="handleLinkDrop"
                            class="flex flex-col gap-2"
                        >
                            <!-- Header (only when has tickets) -->
                            <div v-if="ticket.linkedTickets?.length" class="flex items-center justify-between">
                                <h3 class="text-sm font-medium text-secondary">Linked Tickets</h3>
                                <a
                                    href="#"
                                    @click.prevent="showLinkedTicketModal = true"
                                    class="text-accent hover:text-accent/80 text-sm hover:underline"
                                >
                                    + Add
                                </a>
                            </div>

                            <!-- Drop zone (single instance, shown when dragging) -->
                            <div
                                v-if="showDropAffordance || isLinkDropTarget"
                                class="rounded-lg border-2 border-dashed p-3 text-center text-sm transition-colors"
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

                            <!-- Add link (only when no tickets and not dragging) -->
                            <a
                                v-if="!ticket.linkedTickets?.length && !showDropAffordance && !isLinkDropTarget"
                                href="#"
                                @click.prevent="showLinkedTicketModal = true"
                                class="text-accent hover:underline"
                            >
                                + Add linked ticket
                            </a>
                        </div>

                        <!-- Projects -->
                        <div
                            v-if="ticket.projects?.length"
                            class="flex flex-col gap-2"
                        >
                            <div class="flex items-center justify-between">
                                <h3 class="text-sm font-medium text-secondary">
                                    Projects
                                </h3>
                                <a
                                    href="#"
                                    @click.prevent="showProjectModal = true"
                                    class="text-accent hover:text-accent/80 text-sm hover:underline"
                                >
                                    + Add to project
                                </a>
                            </div>
                            <div class="flex flex-col gap-2">
                                <ProjectInfo
                                    v-for="projectId in ticket.projects"
                                    :key="projectId"
                                    :project-id="projectId"
                                    @view="viewProject(projectId)"
                                    @remove="() => removeFromProject(projectId)"
                                />
                            </div>
                        </div>
                        <div v-else>
                            <a
                                href="#"
                                @click.prevent="showProjectModal = true"
                                class="block text-accent hover:underline"
                            >
                                + Add to project
                            </a>
                        </div>

                        <!-- Plugin Components -->
                        <PluginSlot slot-name="ticket-sidebar" :ticket="ticket" />
                        </div>

                        <!-- Comments (inside left-column for tablet 2-col layout) -->
                        <div class="ticket-comments rounded-xl">
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
                    <div class="ticket-article rounded-xl">
                        <CollaborativeTicketArticle
                            :key="`article-${ticket.id}`"
                            :initial-content="ticket.article_content || ''"
                            :ticket-id="ticket.id"
                        />
                    </div>
                </div>
            </div>
        </div>

        <div v-else class="p-4 sm:p-6 text-center text-secondary">
            Loading ticket...
        </div>

        <!-- Modals -->
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
