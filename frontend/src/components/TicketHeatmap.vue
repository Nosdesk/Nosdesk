<script setup lang="ts">
import { formatDate, formatDateTime } from '@/utils/dateUtils';
import { ref, onMounted, computed, onActivated } from "vue";
import { useRouter } from "vue-router";
import { getTickets } from "@/services/ticketService";
import type { Ticket } from "@/services/ticketService";
import HeatmapTooltip from "@/components/HeatmapTooltip.vue";

interface Props {
    ticketStatus?: "open" | "in-progress" | "closed";
    userUuid?: string;
    title?: string;
}

const props = withDefaults(defineProps<Props>(), {
    ticketStatus: "closed",
    userUuid: "",
    title: "",
});

interface DayData {
    date: string;
    count: number;
    tickets: { id: number; title: string }[];
}

const router = useRouter();
const heatmapData = ref<DayData[]>([]);
const isLoading = ref(true);
const error = ref<string | null>(null);

// Generate 365 days of data ending today
const generateDateRange = () => {
    const dates: DayData[] = [];
    const today = new Date();

    // Generate 365 days ending today (including today)
    for (let i = 364; i >= 0; i--) {
        const date = new Date(
            today.getFullYear(),
            today.getMonth(),
            today.getDate() - i,
        );
        // Use local date formatting to avoid timezone issues
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

// Generate skeleton grid structure (53 weeks x 7 days)
const skeletonWeeks = Array.from({ length: 53 }, () => Array(7).fill(null));

// Fetch ticket data and populate the heatmap
const fetchTicketData = async () => {
    isLoading.value = true;
    error.value = null;

    try {
        const emptyDates = generateDateRange();
        const dateMap = new Map<
            string,
            { count: number; tickets: { id: number; title: string }[] }
        >();

        // Initialize map
        emptyDates.forEach((day) => {
            dateMap.set(day.date, { count: 0, tickets: [] });
        });

        // Fetch and process tickets
        const tickets = await getTickets();

        tickets.forEach((ticket) => {
            if (ticket.status === props.ticketStatus) {
                // Filter by user if specified (match assignee for closed tickets)
                if (props.userUuid && ticket.assignee !== props.userUuid) {
                    return;
                }

                const dateStr =
                    ticket.status === "closed" && ticket.closed_at
                        ? ticket.closed_at.split("T")[0]
                        : ticket.modified.split("T")[0];

                if (dateMap.has(dateStr)) {
                    const dayData = dateMap.get(dateStr)!;
                    dayData.count++;
                    dayData.tickets.push({
                        id: ticket.id,
                        title: ticket.title,
                    });
                }
            }
        });

        // Convert back to array
        heatmapData.value = emptyDates.map((day) => ({
            date: day.date,
            count: dateMap.get(day.date)?.count || 0,
            tickets: dateMap.get(day.date)?.tickets || [],
        }));
    } catch (err: any) {
        console.error("Error fetching ticket data for heatmap:", err);
        error.value = "Failed to load ticket data. Please try again.";
    } finally {
        isLoading.value = false;
    }
};

// Get CSS class based on activity count
const getColorClass = (count: number) => {
    if (count === 0) return "heatmap-level-0";
    if (count <= 1) return "heatmap-level-1";
    if (count <= 2) return "heatmap-level-2";
    if (count <= 3) return "heatmap-level-3";
    return "heatmap-level-4";
};

// Check if date is in the future
const todayStr = new Date().toISOString().split("T")[0];
const isFutureDate = (dateStr: string) => dateStr > todayStr;

// Format date for tooltip
const formatHeatmapDate = (date: string) => {
    return formatDate(date, "MMM d, yyyy");
};

// Create tooltip content
const getTooltipDetails = (day: DayData) => {
    if (day.count === 0) {
        return {
            title: "No tickets",
            date: formatHeatmapDate(day.date),
        };
    }

    const ticketWord = day.count === 1 ? "ticket" : "tickets";
    return {
        title: `${day.count} ${ticketWord}`,
        date: formatHeatmapDate(day.date),
        tickets: day.tickets.slice(0, 5).map((ticket) => ({
            id: ticket.id,
            title: ticket.title,
        })),
        totalTickets: day.tickets.length,
    };
};

// Group data into proper weeks (starting Sunday)
const weeklyData = computed(() => {
    if (heatmapData.value.length === 0) return [];

    const weeks: DayData[][] = [];
    const data = [...heatmapData.value];

    // Find the first Sunday to start the calendar properly
    const firstDate = new Date(data[0].date);
    const firstDayOfWeek = firstDate.getDay(); // 0 = Sunday

    // Pad the beginning if we don't start on Sunday
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

    // Group into weeks of 7 days (including incomplete weeks)
    for (let i = 0; i < data.length; i += 7) {
        const week = data.slice(i, i + 7);
        // Include all weeks, even if incomplete
        if (week.length > 0) {
            // Pad incomplete weeks to 7 days for consistent display
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

// Handle day click navigation
const handleDayClick = (day: DayData) => {
    if (day.count === 0) return;

    const query: Record<string, string> = {
        status: props.ticketStatus,
    };

    if (props.ticketStatus === "closed") {
        query.closedOn = day.date;
    } else {
        query.createdOn = day.date;
    }

    router.push({
        path: "/tickets",
        query,
    });
};

onMounted(() => {
    fetchTicketData();
});

// Refetch data when component is activated (e.g., navigating back to dashboard)
onActivated(() => {
    fetchTicketData();
});
</script>

<template>
    <div class="bg-surface rounded-lg px-3 py-4 sm:p-6 w-full">
        <!-- Header -->
        <div class="mb-4">
            <h3 class="text-secondary text-sm font-medium">
                {{ props.title || (props.ticketStatus === "closed" ? "Closed Tickets" : "Ticket Activity") }}
            </h3>
        </div>

        <!-- Error State -->
        <div v-if="error" class="text-status-error text-sm mb-4">
            {{ error }}
        </div>

        <!-- Heatmap Container -->
        <div class="w-full">
            <div class="flex flex-col gap-2 w-full">
                <!-- Main heatmap grid -->
                <div class="flex gap-1.5 w-full">
                    <!-- Days of week labels -->
                    <div
                        class="flex flex-col gap-0.5 text-[10px] text-secondary justify-around flex-shrink-0"
                    >
                        <span class="h-2.5 flex items-center">Sun</span>
                        <span class="h-2.5 flex items-center">Mon</span>
                        <span class="h-2.5 flex items-center">Tue</span>
                        <span class="h-2.5 flex items-center">Wed</span>
                        <span class="h-2.5 flex items-center">Thu</span>
                        <span class="h-2.5 flex items-center">Fri</span>
                        <span class="h-2.5 flex items-center">Sat</span>
                    </div>

                    <!-- Skeleton grid while loading -->
                    <div v-if="isLoading" class="flex-1 min-w-0">
                        <div
                            class="grid w-full animate-pulse"
                            :style="{ gridTemplateColumns: `repeat(${skeletonWeeks.length}, minmax(0, 1fr))` }"
                        >
                            <div
                                v-for="(week, weekIndex) in skeletonWeeks"
                                :key="weekIndex"
                                class="flex flex-col"
                            >
                                <div v-for="dayIndex in 7" :key="dayIndex" class="p-[1px]">
                                    <div class="w-full h-2.5 rounded-[1px] bg-surface-alt border border-subtle" />
                                </div>
                            </div>
                        </div>
                    </div>

                    <!-- Real heatmap data -->
                    <div v-else class="flex-1 min-w-0">
                        <div
                            class="grid w-full"
                            :style="{ gridTemplateColumns: `repeat(${weeklyData.length}, minmax(0, 1fr))` }"
                        >
                            <div
                                v-for="(week, weekIndex) in weeklyData"
                                :key="weekIndex"
                                class="flex flex-col"
                            >
                                <HeatmapTooltip
                                    v-for="(day, dayIndex) in week"
                                    :key="`${weekIndex}-${dayIndex}`"
                                    :text="day.count.toString()"
                                    :details="getTooltipDetails(day)"
                                    :disabled="isFutureDate(day.date)"
                                >
                                    <div class="p-[1px]">
                                        <div
                                            class="w-full h-2.5 rounded-[1px] transition-colors duration-300 hover:scale-110 hover:z-10 border"
                                            :class="[
                                                isFutureDate(day.date) ? 'invisible' : getColorClass(day.count),
                                                isFutureDate(day.date) ? 'border-transparent' : 'border-subtle',
                                                day.count > 0 && !isFutureDate(day.date) ? 'cursor-pointer hover:border-default' : 'cursor-default'
                                            ]"
                                            @click="!isFutureDate(day.date) && handleDayClick(day)"
                                        />
                                    </div>
                                </HeatmapTooltip>
                            </div>
                        </div>
                    </div>
                </div>

                <!-- Legend and info -->
                <div class="flex justify-between items-center">
                    <div class="text-[10px] text-secondary">
                        <template v-if="!isLoading">
                            {{ heatmapData.filter((d) => d.count > 0).length }} days with activity
                        </template>
                        <span v-else class="invisible">0 days with activity</span>
                    </div>

                    <!-- Legend -->
                    <div class="flex items-center gap-2 text-[10px] text-secondary">
                        <span>Less</span>
                        <div class="flex gap-0.5">
                            <div
                                v-for="i in 5"
                                :key="i"
                                class="w-2.5 h-2.5 rounded-[1px] border border-subtle"
                                :class="getColorClass(i - 1)"
                            />
                        </div>
                        <span>More</span>
                    </div>
                </div>
            </div>
        </div>
    </div>
</template>

<style scoped>
/* Heatmap color levels using semantic CSS variables */
/* Level 0: No activity - uses surface-alt for empty state */
.heatmap-level-0 {
    background-color: var(--color-bg-surface-alt);
}

/* Levels 1-4: Gradient from muted to full status-success color */
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
