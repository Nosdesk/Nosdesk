<script setup lang="ts">
import { formatRelativeTime } from '@/utils/dateUtils';
import { ref, computed, watch } from "vue";
import { useRouter } from "vue-router";
import { useAuthStore } from "@/stores/auth";
import { useSSEListeners } from "@/composables/useSSEListeners";
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
const sortBy = ref("priority-date"); // default sort by priority first, then date

// Computed: target user UUID (prop or current user)
const targetUserUuid = computed(() => props.userUuid || authStore.user?.uuid || "");

// Computed: whether showing data for the current user
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
    { value: "priority-date", label: "Priority, then Date" },
    { value: "priority", label: "Highest Priority" },
    { value: "date", label: "Latest Modified" },
];


// Priority order for sorting (higher priority = lower number)
const PRIORITY_ORDER: Record<string, number> = {
    'critical': 0,
    'high': 1,
    'medium': 2,
    'low': 3,
};

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

        // For multi-level sort (priority-date), fetch by priority first
        // For single-level sorts, use the appropriate field
        const sortField = sortBy.value === "date" ? "modified" : "priority";

        // Build query params based on ticket type
        const queryParams: Parameters<typeof ticketService.getPaginatedTickets>[0] = {
            page: 1,
            pageSize: props.limit * 3, // Fetch more to account for client-side filtering/sorting
            sortField,
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

        // Apply client-side sorting for multi-level sort (priority, then date)
        if (sortBy.value === "priority-date") {
            filteredTickets.sort((a, b) => {
                // First sort by priority (critical > high > medium > low)
                const priorityA = PRIORITY_ORDER[a.priority] ?? 4;
                const priorityB = PRIORITY_ORDER[b.priority] ?? 4;
                if (priorityA !== priorityB) {
                    return priorityA - priorityB;
                }
                // Then sort by modified date (newest first)
                return new Date(b.modified).getTime() - new Date(a.modified).getTime();
            });
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
        // Fetch when a valid userUuid is available
        if (userUuid) {
            fetchTickets();
        }
    },
    { immediate: true }
);

// SSE integration for real-time ticket updates
const { on } = useSSEListeners();

/** Check if a status value passes the current filter */
const statusMatchesFilter = (status: string): boolean => {
    if (!selectedStatus.value || selectedStatus.value === '') return true;
    if (selectedStatus.value === 'active') return status === 'open' || status === 'in-progress';
    return status === selectedStatus.value;
};

on('ticket-updated', (data) => {
    const event = data as { ticket_id: number; field: string; value: unknown };
    const idx = tickets.value.findIndex(t => t.id === event.ticket_id);
    if (idx === -1) return;

    const ticket = tickets.value[idx];
    const field = event.field as keyof Ticket;

    // Update the field in place
    if (field in ticket) {
        (ticket as Record<string, unknown>)[field] = event.value;
    }

    // If status changed and no longer matches filter, remove
    if (field === 'status' && !statusMatchesFilter(String(event.value))) {
        tickets.value.splice(idx, 1);
        return;
    }

    // If assignee changed away from target user (for assigned view), remove
    if (field === 'assignee' && props.ticketType === 'assigned') {
        const assigneeVal = event.value as { uuid?: string } | string | null;
        const assigneeUuid = typeof assigneeVal === 'object' && assigneeVal ? assigneeVal.uuid : assigneeVal;
        if (assigneeUuid !== targetUserUuid.value) {
            tickets.value.splice(idx, 1);
        }
    }
});

on('ticket-created', (data) => {
    const event = data as { ticket: Record<string, unknown> };
    if (!event.ticket) return;
    const ticket = event.ticket as unknown as Ticket;

    // Check if the ticket matches our target user
    const matchesUser = props.ticketType === 'assigned'
        ? ticket.assignee_uuid === targetUserUuid.value
        : ticket.requester_uuid === targetUserUuid.value;
    if (!matchesUser) return;

    // Check status filter
    if (!statusMatchesFilter(ticket.status)) return;

    // Prepend and enforce limit
    tickets.value.unshift(ticket);
    if (tickets.value.length > props.limit) {
        tickets.value.pop();
    }
});

on('ticket-deleted', (data) => {
    const event = data as { ticket_id: number };
    tickets.value = tickets.value.filter(t => t.id !== event.ticket_id);
});
</script>

<template>
    <div
        class="bg-surface rounded-xl border border-default hover:border-strong transition-colors overflow-hidden"
    >
        <!-- Header with title and filter -->
        <div
            class="px-3 sm:px-4 py-3 bg-surface-alt border-b border-default flex items-center justify-between gap-2"
        >
            <div v-if="showTitle" class="flex items-center gap-2 min-w-0 flex-shrink">
                <h2 class="text-base sm:text-lg font-medium text-primary truncate">
                    {{ displayTitle }}
                </h2>
                <router-link
                    :to="seeAllLink"
                    class="text-xs px-2 py-1 sm:px-3 sm:py-1.5 bg-accent text-white rounded-lg hover:opacity-90 transition-colors font-medium flex-shrink-0"
                >
                    All
                </router-link>
            </div>

            <div v-if="showFilters" class="flex gap-1 flex-shrink-0">
                <BaseDropdown
                    v-model="sortBy"
                    :options="sortOptions"
                    size="xs"
                />
                <BaseDropdown
                    v-model="selectedStatus"
                    :options="statusOptions"
                    size="xs"
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

                        <!-- Metadata: ID, Status, Priority -->
                        <div class="flex items-center gap-3 text-xs">
                            <span class="font-mono text-tertiary">#{{ ticket.id }}</span>
                            <StatusBadge type="status" :value="ticket.status" :compact="true" />
                            <StatusBadge type="priority" :value="ticket.priority" :short="true" :compact="true" />
                        </div>

                        <!-- From and Time (always on bottom) -->
                        <div class="flex items-center gap-3 text-xs text-tertiary">
                            <div v-if="ticket.requester_user" class="flex items-center gap-1.5">
                                <span>From:</span>
                                <UserAvatar
                                    :name="ticket.requester_user.name"
                                    :avatar="ticket.requester_user.avatar_thumb"
                                    :userUuid="ticket.requester_user.uuid"
                                    size="xs"
                                    :showName="true"
                                    class="text-secondary"
                                />
                            </div>
                            <div class="flex items-center gap-1">
                                <svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
                                </svg>
                                <span>{{ formatRelativeTime(ticket.modified) }}</span>
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
