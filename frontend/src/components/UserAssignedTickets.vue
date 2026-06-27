<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { useFluent } from 'fluent-vue';
import { useQuery } from "@pinia/colada";
import { useAuthStore } from "@/stores/auth";
import { useWorkflowStatesStore } from "@/stores/workflowStates";
import { TERMINAL_CATEGORIES } from "@nosdesk/core/types/workflow";
import { useSyncActions } from "@/composables/useSyncActions";
import { useWidgetConfigState } from "@/composables/useWidgetConfigState";
import TicketRow from "@/components/TicketRow.vue";
import TicketRowSkeleton from "@/components/TicketRowSkeleton.vue";
import BaseDropdown, { type DropdownOption } from "@/components/common/BaseDropdown.vue";
import FilterToggle from "@/components/common/FilterToggle.vue";
import DashboardWidgetShell from "@/views/dashboard/DashboardWidgetShell.vue";
import ticketService, { getRecentTickets, type Ticket } from "@nosdesk/core/services/ticketService";

const props = withDefaults(defineProps<{
    limit?: number;
    showTitle?: boolean;
    filterStatus?: string;
    userUuid?: string;
    ticketType?: 'assigned' | 'requested';
    title?: string;
    /** FTL key used in place of `title` so callers passing a registry
     *  entry can stay locale-aware. Wins over `title` when both set. */
    titleKey?: string;
    showFilters?: boolean;
}>(), {
    limit: 5,
    showTitle: true,
    filterStatus: "",
    userUuid: "",
    ticketType: 'assigned',
    title: "",
    titleKey: "",
    showFilters: true,
});

const fluent = useFluent();
const auth = useAuthStore();
const wf = useWorkflowStatesStore();

// Client-side "is this ticket active" check. A ticket is active when
// its workflow-state category is non-terminal (not done/cancelled/
// merged). Relies on the workflow-states store being loaded.
const isActiveTicket = (t: Ticket) => {
    const c = t.workflow_state_id != null ? wf.findById(t.workflow_state_id)?.category : undefined;
    return !!c && !TERMINAL_CATEGORIES.has(c);
};

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

const emptyDescription = computed(() => (isCurrentUser.value ? fluent.$t('user-assigned-tickets-empty-current') : ''));

const displayTitle = computed(() => {
    if (props.titleKey) return fluent.$t(props.titleKey);
    if (props.title) return props.title;
    return props.ticketType === 'requested'
        ? fluent.$t('user-assigned-tickets-title-requested')
        : fluent.$t('user-assigned-tickets-title-assigned');
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
// Computed so labels follow the active locale; the `value` strings
// stay as canonical filter keys consumed by the API.
const statusOptions = computed<DropdownOption[]>(() => [
    { value: "active", label: fluent.$t('user-assigned-tickets-status-active'), description: fluent.$t('user-assigned-tickets-status-active-desc'), tones: ["bg-status-open", "bg-status-in-progress"] },
    { value: "open", label: fluent.$t('user-assigned-tickets-status-open'), tones: ["bg-status-open"] },
    { value: "in-progress", label: fluent.$t('user-assigned-tickets-status-in-progress'), tones: ["bg-status-in-progress"] },
    { value: "closed", label: fluent.$t('user-assigned-tickets-status-closed'), tones: ["bg-status-closed"] },
    { value: "", label: fluent.$t('user-assigned-tickets-status-all'), description: fluent.$t('user-assigned-tickets-status-all-desc'), tones: ["bg-status-open", "bg-status-in-progress", "bg-status-closed"] },
]);

// Sort options. Strictly-by-priority was dropped intentionally:
// without a date tiebreak the intra-tier order is arbitrary.
const sortOptions = computed<DropdownOption[]>(() => [
    { value: "priority-date", label: fluent.$t('user-assigned-tickets-sort-priority'), description: fluent.$t('user-assigned-tickets-sort-priority-desc') },
    { value: "date", label: fluent.$t('user-assigned-tickets-sort-recent'), description: fluent.$t('user-assigned-tickets-sort-recent-desc') },
    { value: "oldest", label: fluent.$t('user-assigned-tickets-sort-oldest'), description: fluent.$t('user-assigned-tickets-sort-oldest-desc') },
]);

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

// -- Query --------------------------------------------------------------
//
// Pinia Colada owns the server response; client-only filters
// (high-priority, new-activity) sit in the `tickets` computed below so
// they don't need a refetch. Switching anything in the query key
// (target user, ticket type, status, sort) triggers a refetch
// automatically — same shape as every other dashboard widget.
//
// `enabled` defends against the auth-not-ready window: the parent
// DashboardView already gates the grid on auth.user.uuid, but this
// stays as belt-and-braces so the component is correct even if a
// caller embeds it outside the dashboard (e.g. profile view).

const queryKey = computed(
    () => [
        'user-tickets',
        props.ticketType,
        targetUserUuid.value,
        config.status,
        config.sort,
        props.limit,
    ] as const,
);

const { data, isPending, isLoading, error, refetch } = useQuery({
    key: queryKey,
    enabled: () => !!targetUserUuid.value,
    query: async ({ signal }) => {
        // "active" collapses open + in-progress client-side (the
        // server status filter only matches a single status at a
        // time).
        const statusFilter =
            config.status && config.status !== "active" ? config.status : undefined;

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

        const response = await ticketService.getPaginatedTickets(queryParams, { signal });

        let rows = response.data;
        if (config.status === "active") {
            // The "active" client filter reads workflow-state categories,
            // so the store must be warm before we filter.
            await wf.load();
            rows = rows.filter(isActiveTicket);
        }
        if (config.sort === "priority-date") {
            rows = rows.slice().sort((a, b) => {
                const priorityA = PRIORITY_ORDER[a.priority] ?? 4;
                const priorityB = PRIORITY_ORDER[b.priority] ?? 4;
                if (priorityA !== priorityB) return priorityA - priorityB;
                return new Date(b.modified).getTime() - new Date(a.modified).getTime();
            });
        }
        return rows;
    },
});

const rawTickets = computed<Ticket[]>(() => data.value ?? []);
// `loading` is the first-paint signal (cache miss); `refreshing` is the
// background-refetch signal. Splitting them lets the shell keep cached
// content visible during a refetch instead of flashing back to skeleton.
const loading = computed(() => isPending.value && data.value === undefined);
const refreshing = computed(() => isLoading.value && data.value !== undefined);
const errorMessage = computed(() =>
    error.value
        ? props.ticketType === 'requested'
            ? fluent.$t('user-assigned-tickets-error-requested')
            : fluent.$t('user-assigned-tickets-error-assigned')
        : null,
);

const tickets = computed<Ticket[]>(() => {
    let out = rawTickets.value;
    if (config.highPriority) out = out.filter(isHighPriority);
    if (config.newActivity) out = out.filter(hasNewActivity);
    return out.slice(0, props.limit);
});

// Sync the explicit prop status (when this widget is embedded outside
// the dashboard, e.g. a profile view) into the persisted config so the
// query key picks it up.
if (props.filterStatus) {
    config.status = props.filterStatus;
}

onMounted(refreshRecentViews);

// -- Live updates -------------------------------------------------------
//
// Refetch when ticket sync actions land (delivered cross-machine via the
// sync stream). The server query owns the assignee/requester + status/
// sort filtering, so a refetch correctly handles every case the old
// discrete handlers did by hand — adds, deletes, reassigns, and
// workflow-state changes that move a ticket in or out of this view —
// without re-deriving that logic client-side. Debounced so a burst of
// changes coalesces into one refetch.
useSyncActions(() => void refetch(), { aggregates: ['ticket'], debounceMs: 300 });
</script>

<template>
    <DashboardWidgetShell
        :title="showTitle ? displayTitle : ''"
        :action-to="seeAllLink"
        :loading="loading"
        :refreshing="refreshing"
        :error="errorMessage"
        :empty="!loading && !errorMessage && tickets.length === 0"
        :empty-title="props.ticketType === 'requested' ? $t('user-assigned-tickets-empty-title-requested') : $t('user-assigned-tickets-empty-title-assigned')"
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
                    :aria-label="$t('user-assigned-tickets-status-filter-aria', { title: displayTitle })"
                />

                <div class="flex-1 min-w-0" />

                <div class="flex items-center gap-1 flex-shrink-0">
                    <FilterToggle
                        v-model="config.highPriority"
                        :label="$t('user-assigned-tickets-filter-high-priority')"
                        active-class="bg-priority-high/15 text-priority-high"
                    >
                        <svg class="w-4 h-4" fill="none" stroke="currentColor" stroke-width="2.5" viewBox="0 0 24 24" aria-hidden="true">
                            <path stroke-linecap="round" stroke-linejoin="round" d="M5 10l7-7m0 0l7 7m-7-7v18" />
                        </svg>
                    </FilterToggle>
                    <FilterToggle
                        v-model="config.newActivity"
                        :label="$t('user-assigned-tickets-filter-new-activity')"
                        active-class="bg-accent/15 text-accent"
                    >
                        <svg class="w-4 h-4" fill="currentColor" viewBox="0 0 24 24" aria-hidden="true">
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
                    :workflow-state-id="ticket.workflow_state_id"
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
