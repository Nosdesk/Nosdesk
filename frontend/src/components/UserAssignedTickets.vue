<script setup lang="ts">
import { formatRelativeTime } from '@/utils/dateUtils';
import { ref, computed, watch } from "vue";
import { useRouter } from "vue-router";
import { useAuthStore } from "@/stores/auth";
import UserAvatar from "@/components/UserAvatar.vue";
import StatusBadge from "@/components/StatusBadge.vue";
import BaseDropdown from "@/components/common/BaseDropdown.vue";
import ticketService, { type Ticket } from "@/services/ticketService";

const props = withDefaults(defineProps<{
    limit?: number;
    showTitle?: boolean;
    filterStatus?: string;
    userUuid?: string;
    ticketType?: 'assigned' | 'requested';
    title?: string;
    showFilters?: boolean;
}>(), {
    limit: 5,
    showTitle: true,
    filterStatus: "",
    userUuid: "",
    ticketType: 'assigned',
    title: "",
    showFilters: true,
});

const router = useRouter();
const authStore = useAuthStore();

const tickets = ref<Ticket[]>([]);
const loading = ref(true);
const error = ref<string | null>(null);
// When filters are hidden, default to showing all tickets; otherwise default to active
const selectedStatus = ref(props.filterStatus || (props.showFilters ? "active" : ""));
const sortBy = ref("date"); // default sort by date

// Computed: target user UUID (prop or current user)
const targetUserUuid = computed(() => props.userUuid || authStore.user?.uuid || "");

// Computed: whether we're showing data for the current user
const isCurrentUser = computed(() => !props.userUuid || props.userUuid === authStore.user?.uuid);

// Computed: display title
const displayTitle = computed(() => {
    if (props.title) return props.title;
    return props.ticketType === 'requested' ? 'Requested Tickets' : 'Assigned Tickets';
});

// Computed: "See All" link
const seeAllLink = computed(() => {
    const baseLink = '/tickets';
    const paramKey = props.ticketType === 'requested' ? 'requester' : 'assignee';
    const userParam = props.userUuid || 'current';
    return `${baseLink}?${paramKey}=${userParam}`;
});

// Status options for the filter
const statusOptions = [
    { value: "active", label: "Active" }, // Default: open + in-progress
    { value: "", label: "All" },
    { value: "open", label: "Open" },
    { value: "in-progress", label: "In Progress" },
    { value: "closed", label: "Closed" },
];

// Sort options
const sortOptions = [
    { value: "date", label: "Latest Modified" },
    { value: "priority", label: "Highest Priority" },
];

// Get tickets for the target user (assigned or requested based on ticketType)
const fetchTickets = async () => {
    if (!targetUserUuid.value) return;

    loading.value = true;
    error.value = null;

    try {
        // "active" and "" both mean fetch all (active filters client-side)
        const statusFilter = selectedStatus.value && selectedStatus.value !== "active"
            ? selectedStatus.value
            : undefined;

        // Build query params based on ticket type
        const queryParams: Parameters<typeof ticketService.getPaginatedTickets>[0] = {
            page: 1,
            pageSize: props.limit * 2, // Fetch more to account for client-side filtering
            sortField: sortBy.value === "priority" ? "priority" : "modified",
            sortDirection: "desc",
            status: statusFilter,
        };

        // Set assignee or requester based on ticket type
        if (props.ticketType === 'requested') {
            queryParams.requester = targetUserUuid.value;
        } else {
            queryParams.assignee = targetUserUuid.value;
        }

        // Use a unique request key to prevent race conditions when multiple instances exist
        const requestKey = `user-tickets-${props.ticketType}-${targetUserUuid.value}`;
        const response = await ticketService.getPaginatedTickets(queryParams, requestKey);

        // Client-side filter for "active" status (open + in-progress)
        let filteredTickets = response.data;
        if (selectedStatus.value === "active") {
            filteredTickets = response.data.filter(
                (ticket) =>
                    ticket.status === "open" || ticket.status === "in-progress",
            );
        }

        // Limit to the requested number
        tickets.value = filteredTickets.slice(0, props.limit);
    } catch (err) {
        console.error(`Error fetching ${props.ticketType} tickets:`, err);
        error.value = `Failed to load ${props.ticketType} tickets`;
    } finally {
        loading.value = false;
    }
};

const navigateToTicket = (ticketId: number) => {
    router.push(`/tickets/${ticketId}`);
};

// Watch for changes and fetch - uses immediate:true to handle initial load
// This is the Vue 3 recommended pattern for data fetching that depends on reactive state
watch(
    [
        targetUserUuid,
        () => props.filterStatus,
        () => props.ticketType,
        selectedStatus,
        sortBy,
    ],
    ([userUuid, newPropStatus]) => {
        if (newPropStatus) selectedStatus.value = newPropStatus;
        // Fetch when we have a valid userUuid
        if (userUuid) {
            fetchTickets();
        }
    },
    { immediate: true }
);

</script>

<template>
    <div
        class="bg-surface rounded-xl border border-default hover:border-strong transition-colors overflow-hidden"
    >
        <!-- Header with title and filter -->
        <div
            class="px-4 py-3 bg-surface-alt border-b border-default flex flex-col sm:flex-row sm:justify-between sm:items-center gap-3"
        >
            <div v-if="showTitle" class="flex items-center gap-3">
                <h2 class="text-lg font-medium text-primary">
                    {{ displayTitle }}
                </h2>
                <router-link
                    :to="seeAllLink"
                    class="text-xs px-3 py-1.5 bg-accent text-white rounded-lg hover:opacity-90 transition-colors font-medium"
                >
                    See All
                </router-link>
            </div>

            <div v-if="showFilters" class="flex flex-col sm:flex-row gap-2">
                <!-- Sort dropdown -->
                <BaseDropdown
                    v-model="sortBy"
                    :options="sortOptions"
                    size="sm"
                />

                <!-- Filter dropdown -->
                <BaseDropdown
                    v-model="selectedStatus"
                    :options="statusOptions"
                    size="sm"
                />
            </div>
        </div>

        <!-- Loading state -->
        <div v-if="loading" class="px-4 py-12 flex justify-center items-center">
            <div class="flex items-center gap-3 text-secondary">
                <svg
                    class="w-5 h-5 animate-spin"
                    fill="none"
                    viewBox="0 0 24 24"
                >
                    <circle
                        class="opacity-25"
                        cx="12"
                        cy="12"
                        r="10"
                        stroke="currentColor"
                        stroke-width="4"
                    ></circle>
                    <path
                        class="opacity-75"
                        fill="currentColor"
                        d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
                    ></path>
                </svg>
                <span class="text-sm font-medium">Loading tickets...</span>
            </div>
        </div>

        <!-- Error state -->
        <div v-else-if="error" class="px-4 py-8 text-center">
            <div class="flex flex-col items-center gap-3">
                <svg
                    class="w-10 h-10 text-status-error"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                >
                    <path
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        stroke-width="2"
                        d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
                    ></path>
                </svg>
                <p class="text-status-error font-medium">{{ error }}</p>
                <button
                    @click="fetchTickets"
                    class="px-4 py-2 bg-surface-alt border border-default rounded-lg text-primary hover:bg-surface-hover transition-colors text-sm font-medium"
                >
                    Try Again
                </button>
            </div>
        </div>

        <!-- Empty state -->
        <div v-else-if="tickets.length === 0" class="px-4 py-8 text-center">
            <div class="flex flex-col items-center gap-3">
                <svg
                    xmlns="http://www.w3.org/2000/svg"
                    class="h-10 w-10 text-tertiary"
                    fill="none"
                    viewBox="0 0 24 24"
                    stroke="currentColor"
                >
                    <path
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        stroke-width="1.5"
                        d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2m-6 9l2 2 4-4"
                    />
                </svg>
                <div>
                    <p class="text-secondary font-medium">
                        No {{ props.ticketType === 'requested' ? 'requested' : 'assigned' }} tickets
                    </p>
                    <p v-if="isCurrentUser" class="text-tertiary text-sm mt-1">
                        You're all caught up!
                    </p>
                </div>
            </div>
        </div>

        <!-- Ticket list -->
        <div v-else class="divide-y divide-default">
            <div
                v-for="ticket in tickets"
                :key="ticket.id"
                @click="navigateToTicket(ticket.id)"
                class="px-4 py-4 hover:bg-surface-hover transition-all duration-200 cursor-pointer group"
            >
                <div class="flex gap-4">
                    <!-- Ticket content -->
                    <div class="flex flex-col gap-1 flex-1 min-w-0 space-y-2">
                        <!-- Title and ID -->
                        <div class="flex items-start gap-2">
                            <h3
                                class="text-primary font-medium group-hover:text-accent transition-colors flex-1 leading-snug"
                            >
                                {{ ticket.title }}
                            </h3>
                        </div>

                        <!-- Metadata row -->
                        <div
                            class="flex flex-wrap items-center gap-x-3 gap-y-1.5 text-xs text-secondary"
                        >
                            <span class="font-mono text-tertiary"
                                >#{{ ticket.id }}</span
                            >

                            <!-- Status and Priority badges -->
                            <StatusBadge type="status" :value="ticket.status" :compact="true" />
                            <StatusBadge type="priority" :value="ticket.priority" :short="true" :compact="true" />

                            <!-- Requester info -->
                            <div
                                v-if="ticket.requester_user"
                                class="flex items-center gap-1.5"
                            >
                                <span class="text-tertiary">From:</span>
                                <UserAvatar
                                    :name="ticket.requester_user.name"
                                    :avatar="
                                        ticket.requester_user.avatar_thumb
                                    "
                                    :userUuid="ticket.requester_user.uuid"
                                    size="xs"
                                    :showName="true"
                                    class="text-secondary"
                                />
                            </div>

                            <!-- Last updated -->
                            <div class="flex items-center gap-1">
                                <svg
                                    class="w-3 h-3 text-tertiary"
                                    fill="none"
                                    stroke="currentColor"
                                    viewBox="0 0 24 24"
                                >
                                    <path
                                        stroke-linecap="round"
                                        stroke-linejoin="round"
                                        stroke-width="2"
                                        d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z"
                                    />
                                </svg>
                                <span class="text-tertiary">{{
                                    formatRelativeTime(ticket.modified)
                                }}</span>
                            </div>
                        </div>
                    </div>

                    <!-- Arrow indicator -->
                    <div class="flex-shrink-0 flex items-center">
                        <svg
                            class="w-5 h-5 text-tertiary group-hover:text-primary group-hover:translate-x-1 transition-all"
                            fill="none"
                            stroke="currentColor"
                            viewBox="0 0 24 24"
                        >
                            <path
                                stroke-linecap="round"
                                stroke-linejoin="round"
                                stroke-width="2"
                                d="M9 5l7 7-7 7"
                            />
                        </svg>
                    </div>
                </div>
            </div>
        </div>
    </div>
</template>
