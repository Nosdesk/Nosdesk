<script setup lang="ts">
import { ref, onMounted, computed, onActivated } from "vue";
import { useRouter } from "vue-router";
import { useFluent } from 'fluent-vue';
import { getTickets } from "@nosdesk/core/services/ticketService";
import { useWorkflowStatesStore } from "@nosdesk/core/stores/workflowStates";
import { TERMINAL_CATEGORIES } from "@nosdesk/core/types/workflow";
import DashboardWidgetShell from "@/views/dashboard/DashboardWidgetShell.vue";
import ContributionHeatmapPlot, {
    type ContributionDay,
} from "@/views/dashboard/charts/ContributionHeatmapPlot.vue";

const fluent = useFluent();
const wf = useWorkflowStatesStore();

interface Props {
    mode?: "completed" | "active";
    userUuid?: string;
    title?: string;
    /** FTL key used in place of `title` so callers passing a registry
     *  entry can stay locale-aware. Wins over `title` when both set. */
    titleKey?: string;
}

const props = withDefaults(defineProps<Props>(), {
    mode: "completed",
    userUuid: "",
    title: "",
    titleKey: "",
});

const router = useRouter();
const heatmapData = ref<ContributionDay[]>([]);
const isLoading = ref(true);
const error = ref<string | null>(null);

const shellTitle = computed(() =>
    props.titleKey
        ? fluent.$t(props.titleKey)
        : (props.title || (props.mode === 'completed'
            ? fluent.$t('ticket-heatmap-title-closed')
            : fluent.$t('ticket-heatmap-title-activity'))),
);

const daysWithActivity = computed(
    () => heatmapData.value.filter((d) => d.count > 0).length,
);

const todayStr = new Date().toISOString().split("T")[0];

// Generate 365 days of data ending today
const generateDateRange = (): ContributionDay[] => {
    const dates: ContributionDay[] = [];
    const today = new Date();

    for (let i = 364; i >= 0; i--) {
        const date = new Date(
            today.getFullYear(),
            today.getMonth(),
            today.getDate() - i,
        );
        const year = date.getFullYear();
        const month = String(date.getMonth() + 1).padStart(2, "0");
        const day = String(date.getDate()).padStart(2, "0");
        const dateStr = `${year}-${month}-${day}`;

        dates.push({
            date: dateStr,
            count: 0,
            tickets: [],
        });
    }

    return dates;
};

const fetchTicketData = async () => {
    isLoading.value = true;
    error.value = null;

    try {
        const emptyDates = generateDateRange();
        const dateMap = new Map<
            string,
            { count: number; tickets: { id: number; title: string }[] }
        >();

        emptyDates.forEach((day) => {
            dateMap.set(day.date, { count: 0, tickets: [] });
        });

        await wf.load();
        const tickets = await getTickets();

        tickets.forEach((ticket) => {
            const cat = ticket.workflow_state_id != null ? wf.findById(ticket.workflow_state_id)?.category : undefined;
            if (!cat) return;
            const isTerminal = TERMINAL_CATEGORIES.has(cat);
            const matches = props.mode === 'completed' ? isTerminal : !isTerminal;
            if (!matches) return;
            if (props.userUuid && ticket.assignee !== props.userUuid) return;
            const dateStr = props.mode === 'completed' && ticket.closed_at
                ? ticket.closed_at.split('T')[0]
                : ticket.modified.split('T')[0];
            if (dateMap.has(dateStr)) {
                const dayData = dateMap.get(dateStr)!;
                dayData.count++;
                dayData.tickets.push({ id: ticket.id, title: ticket.title });
            }
        });

        heatmapData.value = emptyDates.map((day) => ({
            date: day.date,
            count: dateMap.get(day.date)?.count || 0,
            tickets: dateMap.get(day.date)?.tickets || [],
        }));
    } catch (err) {
        console.error("Error fetching ticket data for heatmap:", err);
        error.value = fluent.$t('ticket-heatmap-error-load');
    } finally {
        isLoading.value = false;
    }
};

const weeklyData = computed(() => {
    if (heatmapData.value.length === 0) return [];

    const weeks: ContributionDay[][] = [];
    const data = [...heatmapData.value];

    const firstDate = new Date(data[0].date);
    const firstDayOfWeek = firstDate.getDay();

    for (let i = 0; i < firstDayOfWeek; i++) {
        const paddingDate = new Date(firstDate);
        paddingDate.setDate(paddingDate.getDate() - (firstDayOfWeek - i));
        const year = paddingDate.getFullYear();
        const month = String(paddingDate.getMonth() + 1).padStart(2, "0");
        const day = String(paddingDate.getDate()).padStart(2, "0");
        const dateStr = `${year}-${month}-${day}`;

        data.unshift({
            date: dateStr,
            count: 0,
            tickets: [],
        });
    }

    for (let i = 0; i < data.length; i += 7) {
        const week = data.slice(i, i + 7);
        if (week.length > 0) {
            while (week.length < 7) {
                const lastDate = new Date(week[week.length - 1].date);
                lastDate.setDate(lastDate.getDate() + 1);
                const year = lastDate.getFullYear();
                const month = String(lastDate.getMonth() + 1).padStart(2, "0");
                const day = String(lastDate.getDate()).padStart(2, "0");
                const dateStr = `${year}-${month}-${day}`;

                week.push({
                    date: dateStr,
                    count: 0,
                    tickets: [],
                });
            }
            weeks.push(week);
        }
    }

    return weeks;
});

function handleDayClick(day: ContributionDay) {
    const query: Record<string, string> = {};
    if (props.mode === 'completed') {
        query.closedOn = day.date;
    } else {
        query.createdOn = day.date;
    }

    router.push({
        path: "/tickets",
        query,
    });
}

function legendClass(level: number): string {
    if (level === 0) return "heatmap-level-0";
    if (level === 1) return "heatmap-level-1";
    if (level === 2) return "heatmap-level-2";
    if (level === 3) return "heatmap-level-3";
    return "heatmap-level-4";
}

onMounted(() => {
    fetchTicketData();
});

onActivated(() => {
    fetchTicketData();
});
</script>

<template>
    <!--
      Plot + footer split follows the dashboard fluid-widget contract:
      the shell body slot is `flex-1 min-h-0` (see DashboardWidgetShell)
      and the optional `#footer` slot is pinned chrome. ContributionHeatmapPlot
      fills the body with relative flex sizing — no pixel budgets.
    -->
    <DashboardWidgetShell
        :title="shellTitle"
        :loading="isLoading"
        :error="error"
    >
        <template #skeleton>
            <ContributionHeatmapPlot
                :weeks="[]"
                :today-str="todayStr"
                loading
            />
        </template>

        <ContributionHeatmapPlot
            :weeks="weeklyData"
            :today-str="todayStr"
            @day-click="handleDayClick"
        />

        <template #footer>
            <div v-if="isLoading" class="flex flex-1 min-w-0 items-center">
                <span class="inline-block h-2.5 w-28 rounded bg-surface-alt animate-pulse" />
            </div>
            <div v-else class="flex flex-1 min-w-0 items-center justify-between gap-2">
                <p class="text-3xs text-tertiary tabular-nums truncate min-w-0">
                    {{ $t('ticket-heatmap-days-with-activity', { count: daysWithActivity }) }}
                </p>

                <div class="flex items-center gap-1.5 text-4xs text-tertiary shrink-0">
                    <span class="sr-only">{{ $t('ticket-heatmap-legend-less') }}</span>
                    <div class="flex gap-0.5" aria-hidden="true">
                        <div
                            v-for="i in 5"
                            :key="i"
                            class="size-2 rounded-sm border border-subtle"
                            :class="legendClass(i - 1)"
                        />
                    </div>
                    <span class="sr-only">{{ $t('ticket-heatmap-legend-more') }}</span>
                </div>
            </div>
        </template>
    </DashboardWidgetShell>
</template>

<style scoped>
.heatmap-level-0 {
    background-color: var(--color-bg-surface-alt);
}

.heatmap-level-1 {
    background-color: color-mix(in srgb, var(--color-status-success) 25%, var(--color-bg-surface-alt));
}

.heatmap-level-2 {
    background-color: color-mix(in srgb, var(--color-status-success) 50%, var(--color-bg-surface-alt));
}

.heatmap-level-3 {
    background-color: color-mix(in srgb, var(--color-status-success) 75%, var(--color-bg-surface-alt));
}

.heatmap-level-4 {
    background-color: var(--color-status-success);
}
</style>
