<script setup lang="ts">
/// <reference types="node" />
import { computed, onMounted, watch, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useAuthStore } from "@/stores/auth";
import { STATUS_OPTIONS, PRIORITY_OPTIONS } from "@/constants/ticketOptions";
import ticketService from "@/services/ticketService";

// Composables
import { useTicketData } from "@/composables/useTicketData";
import { useTicketSSE } from "@/composables/useTicketSSE";
import { useTicketDevices } from "@/composables/useTicketDevices";
import { useTicketRelationships } from "@/composables/useTicketRelationships";
import { useTicketComments } from "@/composables/useTicketComments";
import { useTitleManager } from "@/composables/useTitleManager";
import { useRecentTicketsStore } from "@/stores/recentTickets";

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
    formattedCreatedDate,
    formattedModifiedDate,
    comments,
    devices,
    fetchTicket,
    refreshTicket,
    updateStatus,
    updatePriority,
    updateRequester,
    updateAssignee,
    updateTitle,
    deleteTicket,
} = useTicketData();

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
                await ticketService.update(ticket.value.id, { title: newTitle });

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
    (e: "update:ticket", ticket: any | null): void;
}>();

watch(
    ticket,
    (newTicket) => {
        // Pass the actual reactive ticket object reference
        emit("update:ticket", newTicket);
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
</script>

<template>
    <div class="flex-1">
        <div v-if="ticket" class="flex flex-col">
            <!-- Navigation and actions bar -->
            <div class="pt-4 px-6 flex justify-between items-center">
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
                            <span class="text-brand-blue">{{ activeViewerCount }}</span> viewing
                        </span>
                    </div>
                </div>

                <DeleteButton
                    fallbackRoute="/tickets"
                    itemName="Ticket"
                    @delete="deleteTicket"
                />
            </div>

            <div class="flex flex-col gap-4 px-6 py-4 mx-auto w-full max-w-8xl">
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
                            :status-options="STATUS_OPTIONS"
                            :priority-options="PRIORITY_OPTIONS"
                            @update:selectedStatus="updateStatus"
                            @update:selectedPriority="updatePriority"
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
                                    class="text-brand-blue hover:text-brand-blue/80 text-sm hover:underline"
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
                                class="block text-brand-blue hover:underline"
                            >
                                + Add device
                            </a>
                        </div>

                        <!-- Linked Tickets -->
                        <div
                            v-if="ticket.linkedTickets?.length"
                            class="flex flex-col gap-2"
                        >
                            <div class="flex items-center justify-between">
                                <h3 class="text-sm font-medium text-secondary">
                                    Linked Tickets
                                </h3>
                                <a
                                    href="#"
                                    @click.prevent="
                                        showLinkedTicketModal = true
                                    "
                                    class="text-brand-blue hover:text-brand-blue/80 text-sm hover:underline"
                                >
                                    + Add linked ticket
                                </a>
                            </div>
                            <div class="flex flex-col gap-2">
                                <LinkedTicketPreview
                                    v-for="linkedId in ticket.linkedTickets"
                                    :key="linkedId"
                                    :linked-ticket-id="linkedId"
                                    :current-ticket-id="ticket.id"
                                    @unlink="() => unlinkTicket(linkedId)"
                                    @view="() => {}"
                                />
                            </div>
                        </div>
                        <div v-else>
                            <a
                                href="#"
                                @click.prevent="showLinkedTicketModal = true"
                                class="block text-brand-blue hover:underline"
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
                                    class="text-brand-blue hover:text-brand-blue/80 text-sm hover:underline"
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
                                class="block text-brand-blue hover:underline"
                            >
                                + Add to project
                            </a>
                        </div>
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

                    <!-- Comments -->
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
            </div>
        </div>

        <div v-else class="p-6 text-center text-secondary">
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
    display: grid;
    grid-template-columns: 1fr;
    gap: 1.5rem;
}

.ticket-left-column {
    display: contents; /* Dissolve wrapper on mobile */
}

.ticket-details,
.ticket-article,
.ticket-comments {
    min-width: 0; /* Prevent overflow */
}

/* Tablet (lg): 2 columns, details + comments in left, article in right */
@media (min-width: 1024px) {
    .ticket-grid {
        grid-template-columns: minmax(400px, 1.5fr) minmax(0, 1fr);
        grid-template-rows: auto auto;
        align-items: start;
    }

    .ticket-left-column {
        display: flex;
        flex-direction: column;
        gap: 1.5rem;
        grid-column: 1;
        grid-row: 1;
    }

    .ticket-article {
        grid-column: 2;
        grid-row: 1 / 3;
        align-self: start;
    }

    .ticket-comments {
        grid-column: 1;
        grid-row: 2;
    }
}

/* Desktop (xl): 3 columns, wrapper dissolves again */
@media (min-width: 1536px) {
    .ticket-grid {
        grid-template-columns: minmax(350px, 1fr) minmax(0, 1.5fr) minmax(350px, 1fr);
    }

    .ticket-left-column {
        display: contents; /* Dissolve wrapper so details and comments become separate grid items */
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
