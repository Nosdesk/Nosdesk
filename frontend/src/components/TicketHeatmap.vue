<script setup lang="ts">
import { ref, computed, onActivated, watch } from "vue";
import { useRouter } from "vue-router";
import { useFluent } from 'fluent-vue';
import { useDateStore } from "@nosdesk/core/stores/dateStore";
import { useAuthStore } from "@/stores/auth";
import { useSyncTicketsStore } from "@/sync/stores/tickets";
import {
    buildCompletedWork,
    hasAssignedWork,
    heatmapDayRoute,
    shiftDayKey,
    weekdayOfDayKey,
    type HeatmapDay,
    type HeatmapScope,
} from "@/utils/ticketHeatmap";
import SegmentedControl from "@/components/common/SegmentedControl.vue";
import DashboardWidgetShell from "@/views/dashboard/DashboardWidgetShell.vue";
import ContributionHeatmapPlot from "@/views/dashboard/charts/ContributionHeatmapPlot.vue";

const fluent = useFluent();
const dateStore = useDateStore();
const authStore = useAuthStore();
const router = useRouter();

interface Props {
    title?: string;
    /** FTL key used in place of `title` so callers passing a registry
     *  entry can stay locale-aware. Wins over `title` when both set. */
    titleKey?: string;
}

const props = withDefaults(defineProps<Props>(), {
    title: "",
    titleKey: "",
});

const shellTitle = computed(() =>
    props.titleKey
        ? fluent.$t(props.titleKey)
        : (props.title || fluent.$t('dashboard-widget-activity-heatmap-prop-title')),
);

// Derived from the sync pool, which holds every visible ticket with its
// closed_at and closed_by, so the grid paints from cache.
const pool = useSyncTicketsStore().all();
const me = computed(() => authStore.user?.uuid ?? null);

// Mine by default for someone with assigned work, until they pick.
const pickedScope = ref<HeatmapScope | null>(null);
const scope = computed<HeatmapScope>(() =>
    pickedScope.value ?? (hasAssignedWork(pool.value, me.value) ? "mine" : "everyone"),
);
const scopeOptions = computed(() => [
    { value: "mine" as const, label: fluent.$t('ticket-heatmap-scope-mine') },
    { value: "everyone" as const, label: fluent.$t('ticket-heatmap-scope-everyone') },
]);
function setScope(next: HeatmapScope) {
    pickedScope.value = next;
}

// Today moves on when the dashboard is shown again after midnight.
const now = ref(new Date());
onActivated(() => {
    now.value = new Date();
});
watch(() => dateStore.effectiveTimezone, () => {
    now.value = new Date();
});

const heatmapData = computed<HeatmapDay[]>(() =>
    buildCompletedWork(pool.value, {
        now: now.value,
        tz: dateStore.effectiveTimezone,
        scope: scope.value,
        me: me.value,
    }),
);

const todayStr = computed(() => heatmapData.value[heatmapData.value.length - 1]?.date ?? "");

const daysWithActivity = computed(
    () => heatmapData.value.filter((d) => d.count > 0).length,
);

const emptyDay = (date: string): HeatmapDay => ({ date, count: 0, weight: 0, level: 0, tickets: [] });

const weeklyData = computed(() => {
    if (heatmapData.value.length === 0) return [];

    const weeks: HeatmapDay[][] = [];
    const data = [...heatmapData.value];

    const firstDate = data[0].date;
    const firstDayOfWeek = weekdayOfDayKey(firstDate);

    for (let i = 1; i <= firstDayOfWeek; i++) {
        data.unshift(emptyDay(shiftDayKey(firstDate, -i)));
    }

    for (let i = 0; i < data.length; i += 7) {
        const week = data.slice(i, i + 7);
        if (week.length > 0) {
            while (week.length < 7) {
                week.push(emptyDay(shiftDayKey(week[week.length - 1].date, 1)));
            }
            weeks.push(week);
        }
    }

    return weeks;
});

function handleDayClick(day: { date: string }) {
    router.push(heatmapDayRoute(day.date, dateStore.effectiveTimezone, scope.value, me.value));
}

function legendClass(level: number): string {
    if (level === 0) return "heatmap-level-0";
    if (level === 1) return "heatmap-level-1";
    if (level === 2) return "heatmap-level-2";
    if (level === 3) return "heatmap-level-3";
    return "heatmap-level-4";
}
</script>

<template>
    <!--
      Plot + footer split follows the dashboard fluid-widget contract:
      the shell body slot is `flex-1 min-h-0` (see DashboardWidgetShell)
      and the optional `#footer` slot is pinned chrome. ContributionHeatmapPlot
      fills the body with relative flex sizing — no pixel budgets.
    -->
    <DashboardWidgetShell :title="shellTitle">
        <template #headerActions>
            <SegmentedControl
                :model-value="scope"
                :options="scopeOptions"
                :aria-label="$t('ticket-heatmap-scope-label')"
                size="sm"
                @update:model-value="setScope"
            />
        </template>

        <ContributionHeatmapPlot
            :weeks="weeklyData"
            :today-str="todayStr"
            @day-click="handleDayClick"
        />

        <template #footer>
            <div class="flex flex-1 min-w-0 items-center justify-between gap-2">
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
