<script setup lang="ts">
import { ref, computed, watch, onMounted } from "vue";
import { useAuthStore } from "@/stores/auth";
import { useSSEListeners } from "@/composables/useSSEListeners";
import { useWidgetConfigState } from "@/composables/useWidgetConfigState";
import TicketRow from "@/components/TicketRow.vue";
import TicketRowSkeleton from "@/components/TicketRowSkeleton.vue";
import BaseDropdown, { type DropdownOption } from "@/components/common/BaseDropdown.vue";
import FilterToggle from "@/components/common/FilterToggle.vue";
import DashboardWidgetShell from "@/views/dashboard/DashboardWidgetShell.vue";
import ticketService, { getRecentTickets, type Ticket } from "@/services/ticketService";

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

const auth = useAuthStore();

// When this widget is rendered on the dashboard (current user's own
// view), filter + sort choices persist to the widget's config so a
// reload returns the user to where they were. When it is rendered on
// another user's profile page, `userUuid` is set and the widget-id
// resolves to null, so the config composable becomes a no-op for
// persistence — two instances can't fight for the same config slot.
const dashboardWidgetId = computed<string | null>(() => {
    if (props.userUuid) return null;
    return props.ticketType === 'requested' ? 'requested-tickets' : 'assigned-tickets';
});

const config = useWidgetConfigState(dashboardWidgetId, {
    status: props.filterStatus || (props.showFilters ? "active" : ""),
    sort: "priority-date",
    highPriority: false,
    newActivity: false,
});

const targetUserUuid = computed(() => props.userUuid || auth.user?.uuid || "");
const isCurrentUser = computed(() => !props.userUuid || props.userUuid === auth.user?.uuid);

// `You're` in a template string needs escaping, so keep it in script.
const emptyDescription = computed(() => (isCurrentUser.value ? "You're all caught up!" : ''));

const displayTitle = computed(() => {
    if (props.title) return props.title;
    return props.ticketType === 'requested' ? 'Requested Tickets' : 'Assigned Tickets';
});

const seeAllLink = computed(() => {
    const paramKey = props.ticketType === 'requested' ? 'requester' : 'assignee';
    const userParam = props.userUuid || 'current';
    return `/tickets?${paramKey}=${userParam}`;
});

// Status filter options. `tones` tags each option with the status
// colours it represents. Single statuses get one dot, meta options
// that span multiple get a cluster (Active unions open + in-progress,
// All unions all three), so the gutter always carries information.
const statusOptions: DropdownOption[] = [
    { value: "active", label: "Active", description: "Open + In Progress", tones: ["bg-status-open", "bg-status-in-progress"] },
    { value: "open", label: "Open", tones: ["bg-status-open"] },
    { value: "in-progress", label: "In Progress", tones: ["bg-status-in-progress"] },
    { value: "closed", label: "Closed", tones: ["bg-status-closed"] },
    { value: "", label: "All", description: "Every status", tones: ["bg-status-open", "bg-status-in-progress", "bg-status-closed"] },
];

// Sort options. Strictly-by-priority was dropped intentionally:
// without a date tiebreak the intra-tier order is arbitrary.
const sortOptions: DropdownOption[] = [
    { value: "priority-date", label: "Priority", description: "Priority, then recent" },
    { value: "date", label: "Recent", description: "Most recently modified" },
    { value: "oldest", label: "Oldest", description: "Oldest first, for triage" },
];

const PRIORITY_ORDER: Record<string, number> = {
    'critical': 0,
    'high': 1,
    'medium': 2,
    'low': 3,
};

// -- New-activity cross-reference -------------------------------------

// Map of ticket id → ISO timestamp of the current user's last view of
// that ticket, populated from /tickets/recent. A ticket has "new
// activity" when its `modified` is after the user's last view.
// Tickets never viewed don't appear here and are treated as no new
// activity — the indicator is about updates since you last looked,
// not about tickets you haven't touched yet.
const lastViewedById = ref<Map<number, string>>(new Map());

async function refreshRecentViews() {
    try {
        const views = await getRecentTickets();
        lastViewedById.value = new Map(views.map((v) => [v.id, v.last_viewed_at]));
    } catch (err) {
        // Non-fatal — the new-activity signal just won't be available.
        console.error('Failed to fetch recent ticket views:', err);
    }
}

function hasNewActivity(ticket: Ticket): boolean {
    const lastViewed = lastViewedById.value.get(ticket.id);
    if (!lastViewed) return false;
    return new Date(ticket.modified).getTime() > new Date(lastViewed).getTime();
}

function isHighPriority(ticket: Ticket): boolean {
    const p = ticket.priority as string;
    return p === 'high' || p === 'critical';
}

// -- Fetch --------------------------------------------------------------

// `rawTickets` is the server response; `tickets` is the displayed
// list after client-side filters. Splitting them means toggles that
// only narrow the visible set (high-priority, new-activity) update
// instantly without a round-trip — only state that changes what the
// server returns (status, sort, target user) refetches.
const rawTickets = ref<Ticket[]>([]);
const loading = ref(true);
const refreshing = ref(false);
const hasLoadedOnce = ref(false);
const error = ref<string | null>(null);

const tickets = computed<Ticket[]>(() => {
    let out = rawTickets.value;
    if (config.highPriority) out = out.filter(isHighPriority);
    if (config.newActivity) out = out.filter(hasNewActivity);
    return out.slice(0, props.limit);
});

async function fetchTickets() {
    if (!targetUserUuid.value) return;

    if (hasLoadedOnce.value) {
        refreshing.value = true;
    } else {
        loading.value = true;
    }
    error.value = null;

    try {
        // "active" collapses open + in-progress client-side (the server
        // status filter only matches a single status at a time).
        const statusFilter = config.status && config.status !== "active"
            ? config.status
            : undefined;

        // Sort field + direction per sortBy:
        //  - priority-date: priority desc (client re-sorts with date tiebreak)
        //  - date:          modified desc
        //  - oldest:        created_at asc
        let sortField: 'priority' | 'modified' | 'created_at' = 'priority';
        let sortDirection: 'asc' | 'desc' = 'desc';
        if (config.sort === 'date') {
            sortField = 'modified';
        } else if (config.sort === 'oldest') {
            sortField = 'created_at';
            sortDirection = 'asc';
        }

        // Fetch more than the display limit so the "active" client
        // filter still leaves enough rows to fill the widget.
        const queryParams: Parameters<typeof ticketService.getPaginatedTickets>[0] = {
            page: 1,
            pageSize: props.limit * 3,
            sortField,
            sortDirection,
            status: statusFilter,
        };
        if (props.ticketType === 'requested') {
            queryParams.requester = targetUserUuid.value;
        } else {
            queryParams.assignee = targetUserUuid.value;
        }

        const requestKey = `user-tickets-${props.ticketType}-${targetUserUuid.value}`;
        const response = await ticketService.getPaginatedTickets(queryParams, requestKey);

        let data = response.data;
        if (config.status === "active") {
            data = data.filter((t) => t.status === "open" || t.status === "in-progress");
        }
        if (config.sort === "priority-date") {
            data = data.slice().sort((a, b) => {
                const priorityA = PRIORITY_ORDER[a.priority] ?? 4;
                const priorityB = PRIORITY_ORDER[b.priority] ?? 4;
                if (priorityA !== priorityB) return priorityA - priorityB;
                return new Date(b.modified).getTime() - new Date(a.modified).getTime();
            });
        }

        rawTickets.value = data;
    } catch (err) {
        console.error(`Error fetching ${props.ticketType} tickets:`, err);
        error.value = `Failed to load ${props.ticketType} tickets`;
    } finally {
        loading.value = false;
        refreshing.value = false;
        hasLoadedOnce.value = true;
    }
}

onMounted(refreshRecentViews);

// Refetch only when inputs that change what the server returns change.
// Client-only filters (high-priority, new-activity) are computed from
// `rawTickets` and don't need a round-trip.
watch(
    [
        targetUserUuid,
        () => props.filterStatus,
        () => props.ticketType,
        () => config.status,
        () => config.sort,
    ],
    ([userUuid, newPropStatus]) => {
        if (newPropStatus) config.status = newPropStatus;
        if (userUuid) fetchTickets();
    },
    { immediate: true },
);

// -- SSE live updates ---------------------------------------------------

const { on } = useSSEListeners();

function statusMatchesFilter(status: string): boolean {
    if (!config.status) return true;
    if (config.status === 'active') return status === 'open' || status === 'in-progress';
    return status === config.status;
}

on('ticket-updated', (data) => {
    const event = data as { ticket_id: number; field: string; value: unknown };
    const idx = rawTickets.value.findIndex(t => t.id === event.ticket_id);
    if (idx === -1) return;

    const ticket = rawTickets.value[idx];
    const field = event.field as keyof Ticket;
    if (field in ticket) {
        (ticket as Record<string, unknown>)[field] = event.value;
    }

    if (field === 'status' && !statusMatchesFilter(String(event.value))) {
        rawTickets.value.splice(idx, 1);
        return;
    }
    if (field === 'assignee' && props.ticketType === 'assigned') {
        const assigneeVal = event.value as { uuid?: string } | string | null;
        const assigneeUuid = typeof assigneeVal === 'object' && assigneeVal ? assigneeVal.uuid : assigneeVal;
        if (assigneeUuid !== targetUserUuid.value) {
            rawTickets.value.splice(idx, 1);
        }
    }
});

on('ticket-created', (data) => {
    const event = data as { ticket: Record<string, unknown> };
    if (!event.ticket) return;
    const ticket = event.ticket as unknown as Ticket;

    const matchesUser = props.ticketType === 'assigned'
        ? ticket.assignee_uuid === targetUserUuid.value
        : ticket.requester_uuid === targetUserUuid.value;
    if (!matchesUser) return;
    if (!statusMatchesFilter(ticket.status)) return;

    rawTickets.value.unshift(ticket);
});

on('ticket-deleted', (data) => {
    const event = data as { ticket_id: number };
    rawTickets.value = rawTickets.value.filter(t => t.id !== event.ticket_id);
});
</script>

<template>
    <DashboardWidgetShell
        :title="showTitle ? displayTitle : ''"
        :action-to="seeAllLink"
        :loading="loading"
        :refreshing="refreshing"
        :error="error"
        :empty="!loading && !error && tickets.length === 0"
        :empty-title="`No ${props.ticketType === 'requested' ? 'requested' : 'assigned'} tickets`"
        :empty-description="emptyDescription"
        min-body-height="200px"
    >
        <template #skeleton>
            <TicketRowSkeleton :count="5" />
        </template>

        <template v-if="showFilters" #subheader>
            <div class="flex items-center gap-2 px-3 py-1.5 border-b border-default bg-surface-alt/40">
                <BaseDropdown
                    v-model="config.status"
                    :options="statusOptions"
                    size="xs"
                    class="flex-shrink-0"
                    :aria-label="`${displayTitle} status filter`"
                />

                <div class="flex-1 min-w-0" />

                <div class="flex items-center gap-1 flex-shrink-0">
                    <FilterToggle
                        v-model="config.highPriority"
                        label="High priority only"
                        active-class="bg-priority-high/15 text-priority-high"
                    >
                        <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" stroke-width="2.5" viewBox="0 0 24 24" aria-hidden="true">
                            <path stroke-linecap="round" stroke-linejoin="round" d="M5 10l7-7m0 0l7 7m-7-7v18" />
                        </svg>
                    </FilterToggle>
                    <FilterToggle
                        v-model="config.newActivity"
                        label="New activity only"
                        active-class="bg-accent/15 text-accent"
                    >
                        <svg class="w-3.5 h-3.5" fill="currentColor" viewBox="0 0 24 24" aria-hidden="true">
                            <circle cx="12" cy="12" r="4" />
                            <circle cx="12" cy="12" r="8" fill="none" stroke="currentColor" stroke-width="1.5" stroke-opacity="0.4" />
                        </svg>
                    </FilterToggle>
                </div>

                <BaseDropdown
                    v-model="config.sort"
                    :options="sortOptions"
                    size="xs"
                    class="flex-shrink-0"
                />
            </div>
        </template>

        <ul class="divide-y divide-default">
            <li v-for="ticket in tickets" :key="ticket.id">
                <TicketRow
                    :id="ticket.id"
                    :title="ticket.title"
                    :status="ticket.status"
                    :priority="ticket.priority"
                    :timestamp="ticket.modified"
                    :requester="ticket.requester_user"
                    :new-activity="hasNewActivity(ticket)"
                    :to="`/tickets/${ticket.id}`"
                />
            </li>
        </ul>
    </DashboardWidgetShell>
</template>
