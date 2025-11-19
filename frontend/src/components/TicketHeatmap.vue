<script setup lang="ts">
import { formatDate, formatDateTime } from '@/utils/dateUtils';
import { ref, onMounted, computed } from "vue";
import { useRouter } from "vue-router";
import { getTickets } from "@/services/ticketService";
import type { Ticket } from "@/services/ticketService";
import HeatmapTooltip from "@/components/HeatmapTooltip.vue";
import { useThemeStore } from "@/stores/theme";

interface Props {
    ticketStatus?: "open" | "in-progress" | "closed";
}

const props = withDefaults(defineProps<Props>(), {
    ticketStatus: "closed",
});

interface DayData {
    date: string;
    count: number;
    tickets: { id: number; title: string }[];
}

const router = useRouter();
const themeStore = useThemeStore();
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

// Get color based on activity count and theme
const getColor = (count: number) => {
    const isDark = themeStore.isDarkMode;

    if (isDark) {
        // Dark mode - darker greens with good contrast
        if (count === 0) return "#334155"; // slate-700 - No activity (slightly lighter than bg)
        if (count <= 1) return "#14532d"; // green-950 - Very low
        if (count <= 2) return "#166534"; // green-800 - Low activity
        if (count <= 3) return "#15803d"; // green-700 - Medium activity
        return "#16a34a"; // green-600 - High activity
    } else {
        // Light mode - lighter greens with good contrast
        if (count === 0) return "#f3f4f6"; // gray-100 - No activity (subtle)
        if (count <= 1) return "#dcfce7"; // green-100 - Very low
        if (count <= 2) return "#86efac"; // green-300 - Low activity
        if (count <= 3) return "#4ade80"; // green-400 - Medium activity
        return "#22c55e"; // green-500 - High activity
    }
};

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
</script>

<template>
    <div class="bg-surface rounded-lg p-6 w-full">
        <!-- Header -->
        <div class="flex justify-between items-center mb-4">
            <h3 class="text-secondary text-sm font-medium">
                {{
                    props.ticketStatus === "closed"
                        ? "Closed Tickets"
                        : "Ticket Activity"
                }}
                Heatmap
            </h3>
            <button
                @click="fetchTicketData"
                class="text-xs text-secondary hover:text-primary transition-colors disabled:opacity-50"
                :disabled="isLoading"
            >
                <span v-if="isLoading">Loading...</span>
                <span v-else>Refresh</span>
            </button>
        </div>

        <!-- Error State -->
        <div v-if="error" class="text-red-400 text-sm mb-4">
            {{ error }}
        </div>

        <!-- Loading State -->
        <div
            v-else-if="isLoading"
            class="flex justify-center items-center h-32 text-secondary"
        >
            <div class="flex flex-col items-center gap-2">
                <svg
                    class="w-6 h-6 animate-spin"
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
                <span class="text-sm">Loading ticket data...</span>
            </div>
        </div>

        <!-- Heatmap -->
        <div v-else class="w-full">
            <!-- Responsive container with flexible layout -->
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

                    <!-- Heatmap container that fills remaining space -->
                    <div class="flex-1 min-w-0">
                        <div
                            class="grid w-full"
                            :style="{
                                gridTemplateColumns: `repeat(${weeklyData.length}, minmax(0, 1fr))`,
                            }"
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
                                >
                                    <div class="p-[1px]">
                                        <div
                                            class="w-full h-2.5 rounded-[1px] transition-transform duration-75 hover:scale-110 hover:z-10 border border-subtle"
                                            :class="{
                                                'cursor-pointer hover:border-default':
                                                    day.count > 0,
                                                'cursor-default': day.count === 0,
                                            }"
                                            :style="{
                                                backgroundColor: getColor(
                                                    day.count,
                                                ),
                                            }"
                                            @click="handleDayClick(day)"
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
                        {{ heatmapData.filter((d) => d.count > 0).length }} days
                        with activity
                    </div>

                    <!-- Legend -->
                    <div
                        class="flex items-center gap-2 text-[10px] text-secondary"
                    >
                        <span>Less</span>
                        <div class="flex gap-0.5">
                            <div
                                v-for="i in 6"
                                :key="i"
                                class="w-2.5 h-2.5 rounded-[1px] border border-subtle"
                                :style="{
                                    backgroundColor: getColor(i - 1),
                                }"
                            />
                        </div>
                        <span>More</span>
                    </div>
                </div>
            </div>
        </div>
    </div>
</template>
