<script setup lang="ts">
import { ref, onMounted, computed, watch } from "vue";
import { useRouter } from "vue-router";
import { useAuthStore } from "@/stores/auth";
import UserAvatar from "@/components/UserAvatar.vue";
import ticketService, { type Ticket } from "@/services/ticketService";

const props = defineProps({
    limit: {
        type: Number,
        default: 5,
    },
    showTitle: {
        type: Boolean,
        default: true,
    },
    filterStatus: {
        type: String,
        default: "", // empty string means show all
    },
});

const router = useRouter();
const authStore = useAuthStore();

const tickets = ref<Ticket[]>([]);
const loading = ref(true);
const error = ref<string | null>(null);
const selectedStatus = ref(props.filterStatus || "active"); // Default to active tickets
const sortBy = ref("date"); // default sort by date

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

// Priority mapping for sorting (higher number = higher priority)
const priorityOrder = {
    critical: 4,
    high: 3,
    medium: 2,
    low: 1,
    default: 0,
};

// Format relative time
const formatRelativeTime = (dateString: string) => {
    const date = new Date(dateString);
    const now = new Date();
    const diffInSeconds = Math.floor((now.getTime() - date.getTime()) / 1000);

    if (diffInSeconds < 60) {
        return "just now";
    } else if (diffInSeconds < 3600) {
        const minutes = Math.floor(diffInSeconds / 60);
        return `${minutes}m ago`;
    } else if (diffInSeconds < 86400) {
        const hours = Math.floor(diffInSeconds / 3600);
        return `${hours}h ago`;
    } else {
        const days = Math.floor(diffInSeconds / 86400);
        if (days < 7) {
            return `${days}d ago`;
        } else if (days < 30) {
            const weeks = Math.floor(days / 7);
            return `${weeks}w ago`;
        } else {
            return date.toLocaleDateString("en-US", {
                month: "short",
                day: "numeric",
                year:
                    date.getFullYear() !== now.getFullYear()
                        ? "numeric"
                        : undefined,
            });
        }
    }
};

// Get class for priority badge - matching LinkedTicketPreview style
const getPriorityClass = (priority: string) => {
    switch (priority) {
        case "critical":
            return "bg-red-500/20 text-red-700 dark:text-red-300 border-red-500/30";
        case "high":
            return "bg-red-500/20 text-red-700 dark:text-red-300 border-red-500/30";
        case "medium":
            return "bg-yellow-500/20 text-yellow-700 dark:text-yellow-300 border-yellow-500/30";
        case "low":
            return "bg-green-500/20 text-green-700 dark:text-green-300 border-green-500/30";
        default:
            return "bg-slate-600/20 text-slate-700 dark:text-slate-300 border-slate-500/30";
    }
};

// Get class for status badge - matching LinkedTicketPreview style
const getStatusClass = (status: string) => {
    switch (status) {
        case "open":
            return "bg-yellow-500/20 text-yellow-700 dark:text-yellow-300 border-yellow-500/30";
        case "in-progress":
            return "bg-blue-500/20 text-blue-700 dark:text-blue-300 border-blue-500/30";
        case "closed":
            return "bg-green-500/20 text-green-700 dark:text-green-300 border-green-500/30";
        default:
            return "bg-slate-600/20 text-slate-700 dark:text-slate-300 border-slate-500/30";
    }
};

// Get formatted status for display
const getFormattedStatus = (status: string) => {
    switch (status) {
        case "in-progress":
            return "In Progress";
        default:
            return status.charAt(0).toUpperCase() + status.slice(1);
    }
};

// Get tickets assigned to the current user
const fetchAssignedTickets = async () => {
    if (!authStore.user?.uuid) return;

    loading.value = true;
    error.value = null;

    try {
        // Handle the "active" filter by fetching open and in-progress tickets
        let statusFilter: string | undefined = selectedStatus.value;
        if (statusFilter === "active") {
            statusFilter = undefined; // We'll filter client-side for active tickets
        } else if (statusFilter === "") {
            statusFilter = undefined; // All tickets
        }

        const response = await ticketService.getPaginatedTickets({
            page: 1,
            pageSize: props.limit * 2, // Fetch more to account for client-side filtering
            sortField: sortBy.value === "priority" ? "priority" : "modified",
            sortDirection: "desc",
            status: statusFilter,
            assignee: authStore.user.uuid,
        });

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
        console.error("Error fetching assigned tickets:", err);
        error.value = "Failed to load your assigned tickets";
    } finally {
        loading.value = false;
    }
};

const navigateToTicket = (ticketId: number) => {
    router.push(`/tickets/${ticketId}`);
};

// Simple watcher that triggers when user becomes available or filters change
watch(
    [
        () => authStore.user?.uuid,
        () => props.filterStatus,
        selectedStatus,
        sortBy,
    ],
    ([userUuid, newPropStatus]) => {
        if (newPropStatus) selectedStatus.value = newPropStatus;
        if (userUuid) {
            fetchAssignedTickets();
        }
    },
    { immediate: true },
);

// Get status counts for the tickets
const statusCounts = computed(() => {
    if (!authStore.user) return { total: 0, open: 0, inProgress: 0, closed: 0 };

    // This would normally be done on the backend with a dedicated API
    // Here we're just counting the tickets we already have
    return {
        total: tickets.value.length,
        open: tickets.value.filter((t) => t.status === "open").length,
        inProgress: tickets.value.filter((t) => t.status === "in-progress")
            .length,
        closed: tickets.value.filter((t) => t.status === "closed").length,
    };
});
</script>

<template>
    <div
        class="bg-surface rounded-xl border border-default hover:border-strong transition-colors overflow-hidden"
    >
        <!-- Header with title and filter -->
        <div
            class="px-4 py-3 bg-surface-hover border-b border-default flex flex-col sm:flex-row sm:justify-between sm:items-center gap-3"
        >
            <div v-if="showTitle" class="flex items-center gap-3">
                <h2 class="text-lg font-medium text-primary">
                    Your Assigned Tickets
                </h2>
                <router-link
                    to="/tickets?assignee=current"
                    class="text-xs px-3 py-1.5 bg-blue-600 text-primary rounded-lg hover:bg-blue-700 transition-colors font-medium"
                >
                    See All
                </router-link>
            </div>

            <div class="flex flex-col sm:flex-row gap-2">
                <!-- Sort dropdown -->
                <div class="relative">
                    <select
                        v-model="sortBy"
                        class="bg-surface border border-default text-sm rounded-lg focus:ring-2 focus:ring-blue-500/50 focus:border-blue-500 focus:outline-none block w-full px-3 py-2 text-primary transition-colors hover:bg-surface-hover"
                    >
                        <option
                            v-for="option in sortOptions"
                            :key="option.value"
                            :value="option.value"
                        >
                            {{ option.label }}
                        </option>
                    </select>
                </div>

                <!-- Filter dropdown -->
                <div class="relative">
                    <select
                        v-model="selectedStatus"
                        class="bg-surface border border-default text-sm rounded-lg focus:ring-2 focus:ring-blue-500/50 focus:border-blue-500 focus:outline-none block w-full px-3 py-2 text-primary transition-colors hover:bg-surface-hover"
                    >
                        <option
                            v-for="option in statusOptions"
                            :key="option.value"
                            :value="option.value"
                        >
                            {{ option.label }}
                        </option>
                    </select>
                </div>
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
                    class="w-10 h-10 text-red-400"
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
                <p class="text-red-400 font-medium">{{ error }}</p>
                <button
                    @click="fetchAssignedTickets"
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
                        No assigned tickets found
                    </p>
                    <p class="text-tertiary text-sm mt-1">
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
                                class="text-primary font-medium group-hover:text-blue-400 transition-colors flex-1 leading-snug"
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
                            <span
                                class="inline-flex items-center px-2 py-0.5 rounded-md font-medium border"
                                :class="getStatusClass(ticket.status)"
                            >
                                {{ getFormattedStatus(ticket.status) }}
                            </span>

                            <span
                                class="inline-flex items-center px-2 py-0.5 rounded-md font-medium border"
                                :class="getPriorityClass(ticket.priority)"
                            >
                                {{
                                    ticket.priority.charAt(0).toUpperCase() +
                                    ticket.priority.slice(1)
                                }}
                            </span>

                            <!-- Requester info -->
                            <div
                                v-if="ticket.requester_user"
                                class="flex items-center gap-1.5"
                            >
                                <span class="text-tertiary">From:</span>
                                <UserAvatar
                                    :name="ticket.requester_user.name"
                                    :avatarUrl="
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
