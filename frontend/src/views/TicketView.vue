<script setup lang="ts">
import { computed, onMounted, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useAuthStore } from "@/stores/auth";
import { STATUS_OPTIONS, PRIORITY_OPTIONS } from "@/constants/ticketOptions";

// Composables
import { useTicketData } from "@/composables/useTicketData";
import { useTicketSSE } from "@/composables/useTicketSSE";
import { useTicketDevices } from "@/composables/useTicketDevices";
import { useTicketRelationships } from "@/composables/useTicketRelationships";
import { useTicketComments } from "@/composables/useTicketComments";

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
    deleteTicket,
} = useTicketData();

// SSE real-time updates
const ticketId = computed(() =>
    route.params.id ? Number(route.params.id) : undefined,
);
const { isConnected, recentlyAddedCommentIds } = useTicketSSE(
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
    projectDetails,
    linkTicket,
    unlinkTicket,
    addToProject,
    removeFromProject,
    fetchProjectDetails,
} = useTicketRelationships(ticket, refreshTicket);

// Comments
const { addComment, deleteAttachment, deleteComment } = useTicketComments(
    ticket,
    refreshTicket,
);

// Emit ticket updates
const emit = defineEmits<{
    (e: "update:ticket", ticket: { id: number; title: string } | null): void;
}>();

watch(
    ticket,
    (newTicket) => {
        if (newTicket) {
            emit("update:ticket", { id: newTicket.id, title: newTicket.title });
        } else {
            emit("update:ticket", null);
        }
    },
    { immediate: true },
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

        // Load project details if exists
        if (ticket.value?.project) {
            await fetchProjectDetails(ticket.value.project);
        }
    }
});

watch(
    () => route.params.id,
    async (newId) => {
        if (newId) {
            await fetchTicket(newId);
            if (ticket.value?.project) {
                await fetchProjectDetails(ticket.value.project);
            }
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
                                'bg-green-400': isConnected,
                                'bg-yellow-400 animate-pulse': !isConnected,
                            }"
                        ></div>
                        <span class="text-slate-400">
                            {{ isConnected ? "Live updates" : "Connecting..." }}
                        </span>
                    </div>
                </div>

                <DeleteButton
                    fallbackRoute="/tickets"
                    itemName="Ticket"
                    @delete="deleteTicket"
                />
            </div>

            <div class="flex flex-col gap-3 px-6 py-3 mx-auto w-full max-w-8xl">
                <!-- Grid Container -->
                <div class="grid-container">
                    <!-- Details Sidebar -->
                    <div class="details-area flex flex-col gap-4">
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
                        />

                        <!-- Devices -->
                        <div v-if="devices.length" class="flex flex-col gap-2">
                            <div class="flex items-center justify-between">
                                <h3 class="text-sm font-medium text-slate-300">
                                    Devices
                                </h3>
                                <a
                                    href="#"
                                    @click.prevent="showDeviceModal = true"
                                    class="text-blue-500 hover:text-blue-400 text-sm hover:underline"
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
                                class="block text-blue-500 hover:underline"
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
                                <h3 class="text-sm font-medium text-slate-300">
                                    Linked Tickets
                                </h3>
                                <a
                                    href="#"
                                    @click.prevent="
                                        showLinkedTicketModal = true
                                    "
                                    class="text-blue-500 hover:text-blue-400 text-sm hover:underline"
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
                                class="block text-blue-500 hover:underline"
                            >
                                + Add linked ticket
                            </a>
                        </div>

                        <!-- Project -->
                        <div class="flex flex-col gap-2">
                            <div
                                v-if="ticket.project"
                                class="flex items-center justify-between"
                            >
                                <h3 class="text-sm font-medium text-slate-300">
                                    Project
                                </h3>
                                <a
                                    href="#"
                                    @click.prevent="showProjectModal = true"
                                    class="text-blue-500 hover:text-blue-400 text-sm hover:underline"
                                >
                                    Change project
                                </a>
                            </div>

                            <div v-if="ticket.project && projectDetails">
                                <ProjectInfo
                                    :project="projectDetails"
                                    :project-id="ticket.project"
                                    @view="viewProject(ticket.project!)"
                                    @remove="removeFromProject"
                                />
                            </div>

                            <div v-else>
                                <a
                                    href="#"
                                    @click.prevent="showProjectModal = true"
                                    class="block text-blue-500 hover:underline"
                                >
                                    + Add to project
                                </a>
                            </div>
                        </div>
                    </div>

                    <!-- Article -->
                    <div class="article-area rounded-xl">
                        <CollaborativeTicketArticle
                            :initial-content="ticket.article_content || ''"
                            :ticket-id="ticket.id"
                        />
                    </div>

                    <!-- Comments -->
                    <div class="comments-area rounded-xl">
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

        <div v-else class="p-6 text-center text-gray-400">
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
            :current-project-id="
                ticket.project ? Number(ticket.project) : undefined
            "
            @close="showProjectModal = false"
            @select-project="addToProject"
        />
    </div>
</template>

<style scoped>
.grid-container {
    display: grid;
    grid-template-columns: 1fr;
    grid-template-rows: auto auto 1fr;
    grid-template-areas: "details" "article" "comments";
    gap: 1rem;
    min-height: calc(100vh - 140px);

    @media (min-width: 1280px) {
        grid-template-columns: minmax(400px, 1fr) minmax(0, 2fr);
        grid-template-rows: auto 1fr;
        grid-template-areas:
            "details article"
            "comments article";
    }

    @media (min-width: 1860px) {
        grid-template-columns: minmax(400px, 1fr) minmax(0, 2fr) minmax(
                400px,
                1fr
            );
        grid-template-rows: 1fr;
        grid-template-areas: "details article comments";
    }
}

.details-area {
    grid-area: details;
    max-height: 100%;
    overflow-y: auto;
}

.article-area {
    grid-area: article;
    display: flex;
    flex-direction: column;
    overflow: visible;
    min-height: fit-content;
}

.comments-area {
    grid-area: comments;
    max-height: 100%;
    overflow-y: auto;
}
</style>
