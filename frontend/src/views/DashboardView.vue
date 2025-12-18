<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import TicketHeatmap from '@/components/TicketHeatmap.vue'
import UserAssignedTickets from '@/components/UserAssignedTickets.vue'
import { getTickets } from '@/services/ticketService'
import type { Ticket } from '@/services/ticketService'
import { useAuthStore } from '@/stores/auth'
import { useBrandingStore } from '@/stores/branding'

// Initialize stores
const authStore = useAuthStore()
const brandingStore = useBrandingStore()

// Get current time-based greeting with helpdesk-themed messages
const getGreeting = () => {
  const hour = new Date().getHours();
  const greetings = {
    morning: [
      { message: "Good morning, {0}.", weight: 1 },
      { message: "Morning, {0}.", weight: 1 },
      { message: "Hey {0}, hope you're having a nice day.", weight: 1 }
    ],
    afternoon: [
      { message: "Good afternoon, {0}.", weight: 1 },
      { message: "Hi {0}, nice to see you.", weight: 1 },
      { message: "Afternoon, {0}.", weight: 1 }
    ],
    evening: [
      { message: "Good evening, {0}.", weight: 1 },
      { message: "Evening, {0}.", weight: 1 },
      { message: "Hi {0}, hope your day went well.", weight: 1 }
    ],
    lateNight: [
      { message: "Good night, {0}.", weight: 1 },
      { message: "Hello {0}, it's getting late.", weight: 1 },
      { message: "Evening, {0}. Remember to rest.", weight: 1 }
    ]
  };

  // Determine time period
  let period;
  if (hour < 12) period = 'morning';
  else if (hour < 18) period = 'afternoon';
  else if (hour < 22) period = 'evening';
  else period = 'lateNight';

  // Select a random greeting from the period's pool
  const periodGreetings = greetings[period as keyof typeof greetings];
  const totalWeight = periodGreetings.reduce((sum, g) => sum + g.weight, 0);
  let random = Math.random() * totalWeight;
  for (const greeting of periodGreetings) {
    random -= greeting.weight;
    if (random <= 0) {
      return greeting.message;
    }
  }
  return periodGreetings[0].message; // Fallback
};

// Compute the formatted greeting with username
const formattedGreeting = computed(() => {
  const greetingTemplate = getGreeting();
  const userName = username.value;
  return greetingTemplate.replace('{0}', userName);
});

// Get username from auth store
const username = computed(() => {
  if (!authStore.user?.name) return 'Guest';
  
  // Split the full name and take the first part as the first name
  const firstName = authStore.user.name.split(' ')[0];
  return firstName;
});

// Initialize ticket stats with zeros
const ticketStats = ref({
  total: 0,
  open: 0,
  inProgress: 0,
  closed: 0
});

// Fetch tickets and update stats
const fetchTicketStats = async () => {
  try {
    const tickets = await getTickets();
    
    // Calculate stats
    ticketStats.value = {
      total: tickets.length,
      open: tickets.filter(ticket => ticket.status === 'open').length,
      inProgress: tickets.filter(ticket => ticket.status === 'in-progress').length,
      closed: tickets.filter(ticket => ticket.status === 'closed').length
    };
  } catch (error) {
    console.error('Error fetching ticket stats:', error);
  }
};

// Fetch data when component mounts
onMounted(() => {
  fetchTicketStats();
});
</script>

<template>
  <div class="flex flex-col h-full">
    <!-- Content -->
    <div class="flex flex-col gap-4 p-6">
      <!-- Greeting Card -->
      <div class="mb-2">
        <h2 class="text-3xl font-medium text-primary">
          {{ formattedGreeting }}
        </h2>
        <p class="text-secondary mt-2">
          Welcome to your {{ brandingStore.appName }} dashboard
        </p>
      </div>

      <!-- Ticket Activity Heatmap - Full Width at Top -->
      <div class="w-full">
        <TicketHeatmap ticketStatus="closed" />
      </div>

      <!-- Condensed Stats Row -->
      <div class="grid grid-cols-2 md:grid-cols-4 gap-3">
        <!-- Total Tickets -->
        <router-link to="/tickets" class="bg-surface rounded-lg border border-default hover:border-strong transition-colors p-4 cursor-pointer group">
          <h3 class="text-tertiary text-xs font-medium uppercase tracking-wide">Total</h3>
          <p class="text-xl font-semibold text-primary mt-1 group-hover:text-blue-400 transition-colors">{{ ticketStats.total }}</p>
        </router-link>

        <!-- Open Tickets -->
        <router-link to="/tickets?status=open" class="bg-surface rounded-lg border border-default hover:border-strong transition-colors p-4 cursor-pointer group">
          <h3 class="text-tertiary text-xs font-medium uppercase tracking-wide">Open</h3>
          <p class="text-xl font-semibold text-status-success mt-1 group-hover:text-blue-400 transition-colors">{{ ticketStats.open }}</p>
        </router-link>

        <!-- In Progress -->
        <router-link to="/tickets?status=in-progress" class="bg-surface rounded-lg border border-default hover:border-strong transition-colors p-4 cursor-pointer group">
          <h3 class="text-tertiary text-xs font-medium uppercase tracking-wide">In Progress</h3>
          <p class="text-xl font-semibold text-brand-blue mt-1 group-hover:text-blue-400 transition-colors">{{ ticketStats.inProgress }}</p>
        </router-link>

        <!-- Closed -->
        <router-link to="/tickets?status=closed" class="bg-surface rounded-lg border border-default hover:border-strong transition-colors p-4 cursor-pointer group">
          <h3 class="text-tertiary text-xs font-medium uppercase tracking-wide">Closed</h3>
          <p class="text-xl font-semibold text-tertiary mt-1 group-hover:text-blue-400 transition-colors">{{ ticketStats.closed }}</p>
        </router-link>
      </div>

      <!-- Assigned Tickets - Full Width -->
      <div class="w-full">
        <UserAssignedTickets :limit="8" />
      </div>
    </div>
  </div>
</template>