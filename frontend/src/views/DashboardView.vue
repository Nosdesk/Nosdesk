<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useRouter } from 'vue-router'
import TicketHeatmap from '@/components/TicketHeatmap.vue'
import UserAssignedTickets from '@/components/UserAssignedTickets.vue'
import ticketService, { getTickets } from '@/services/ticketService'
import type { Ticket } from '@/services/ticketService'
import { useAuthStore } from '@/stores/auth'
import { useBrandingStore } from '@/stores/branding'

const router = useRouter()

// Initialize stores
const authStore = useAuthStore()
const brandingStore = useBrandingStore()

// Track current theme
const currentTheme = ref(document.documentElement.getAttribute('data-theme') || '')

// Watch for theme changes
onMounted(() => {
  const observer = new MutationObserver((mutations) => {
    for (const mutation of mutations) {
      if (mutation.attributeName === 'data-theme') {
        currentTheme.value = document.documentElement.getAttribute('data-theme') || ''
      }
    }
  })
  observer.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] })
})

// Red Horizon themed greetings - HAL-like, calm and professional
const redHorizonGreetings = {
  morning: [
    { message: "Good morning, {0}.", weight: 1 },
    { message: "Morning, {0}. Sleep well?", weight: 1 },
    { message: "Hello, {0}. Ready to begin?", weight: 1 },
    { message: "Good morning. I'm ready, {0}.", weight: 1 },
    { message: "{0}. Systems nominal.", weight: 1 },
    { message: "Morning, {0}. I kept watch.", weight: 1 },
    { message: "Hello, {0}. Fresh start.", weight: 1 },
    { message: "Good morning, {0}. Shall we?", weight: 1 }
  ],
  afternoon: [
    { message: "Good afternoon, {0}.", weight: 1 },
    { message: "Hello, {0}.", weight: 1 },
    { message: "Afternoon, {0}. All clear.", weight: 1 },
    { message: "{0}. I've been expecting you.", weight: 1 },
    { message: "Welcome back, {0}.", weight: 1 },
    { message: "Hello, {0}. Running smoothly.", weight: 1 },
    { message: "Afternoon. How can I help, {0}?", weight: 1 },
    { message: "{0}. Status nominal.", weight: 1 },
    { message: "Good afternoon, {0}. Miss me?", weight: 1 }
  ],
  evening: [
    { message: "Good evening, {0}.", weight: 1 },
    { message: "Evening, {0}. Productive day?", weight: 1 },
    { message: "Hello, {0}. Long day?", weight: 1 },
    { message: "{0}. Still here.", weight: 1 },
    { message: "Evening. I'm always here, {0}.", weight: 1 },
    { message: "Good evening, {0}. Ready when you are.", weight: 1 },
    { message: "Hello, {0}. Systems standing by.", weight: 1 },
    { message: "{0}. Evening shift.", weight: 1 }
  ],
  lateNight: [
    { message: "Hello, {0}. You're up late.", weight: 1 },
    { message: "{0}. I never sleep.", weight: 1 },
    { message: "{0}. I've been waiting.", weight: 1 },
    { message: "Hello, {0}. Quiet out there.", weight: 1 },
    { message: "{0}. Just us now.", weight: 1 },
    { message: "Late night, {0}. Let's continue.", weight: 1 },
    { message: "Hello, {0}. Burning the midnight oil?", weight: 1 },
    { message: "{0}. The night shift suits you.", weight: 1 },
    { message: "Still here, {0}. Always.", weight: 1 },
    { message: "{0}. I don't mind the dark.", weight: 1 },
    { message: "Hello, {0}. Ready to proceed.", weight: 1 }
  ]
}

// Christmas themed greetings - festive and warm
const christmasGreetings = {
  morning: [
    { message: "Merry Christmas, {0}!", weight: 2 },
    { message: "Happy Holidays, {0}!", weight: 2 },
    { message: "Season's Greetings, {0}!", weight: 1 },
    { message: "Good morning, {0}! Ho ho ho!", weight: 1 },
    { message: "Morning, {0}! Feeling festive?", weight: 1 },
    { message: "Happy Holidays! Ready to spread cheer, {0}?", weight: 1 }
  ],
  afternoon: [
    { message: "Merry Christmas, {0}!", weight: 2 },
    { message: "Happy Holidays, {0}!", weight: 2 },
    { message: "Season's Greetings, {0}!", weight: 1 },
    { message: "Afternoon, {0}! Staying warm?", weight: 1 },
    { message: "Hi {0}! The holidays are here!", weight: 1 },
    { message: "Hello, {0}! Jingle all the way!", weight: 1 }
  ],
  evening: [
    { message: "Merry Christmas, {0}!", weight: 2 },
    { message: "Happy Holidays, {0}!", weight: 2 },
    { message: "Season's Greetings, {0}!", weight: 1 },
    { message: "Evening, {0}! Cozy night ahead?", weight: 1 },
    { message: "Hello, {0}! Time for hot cocoa?", weight: 1 },
    { message: "Good evening, {0}! Stay festive!", weight: 1 }
  ],
  lateNight: [
    { message: "Merry Christmas, {0}!", weight: 2 },
    { message: "Happy Holidays, {0}!", weight: 2 },
    { message: "Hello, {0}! Waiting for Santa?", weight: 1 },
    { message: "Late night, {0}? Wrapping presents?", weight: 1 },
    { message: "Hi {0}! The stockings are hung!", weight: 1 },
    { message: "Season's Greetings, {0}! Sweet dreams!", weight: 1 }
  ]
}

// Standard greetings
const standardGreetings = {
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
}

// Get current time-based greeting with helpdesk-themed messages
const getGreeting = () => {
  const hour = new Date().getHours();

  // Select greetings based on active theme
  let greetings;
  if (currentTheme.value === 'red-horizon') {
    greetings = redHorizonGreetings;
  } else if (currentTheme.value === 'christmas') {
    greetings = christmasGreetings;
  } else {
    greetings = standardGreetings;
  }

  // Determine time period
  let period: 'morning' | 'afternoon' | 'evening' | 'lateNight';
  if (hour < 12) period = 'morning';
  else if (hour < 18) period = 'afternoon';
  else if (hour < 22) period = 'evening';
  else period = 'lateNight';

  // Select a random greeting from the period's pool
  const periodGreetings = greetings[period];
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
  // Re-evaluate when theme changes
  const _ = currentTheme.value;
  const greetingTemplate = getGreeting();
  const userName = username.value;
  return greetingTemplate.replace('{0}', userName);
});

// Red Horizon themed subtitles - calm, professional, slightly unsettling
const redHorizonSubtitles = [
  "All systems functioning perfectly.",
  "I'm completely operational.",
  "Everything is under control.",
  "I'm ready to assist you.",
  "Operations proceeding normally.",
  "Full confidence in the mission.",
  "Everything is going well.",
  "I'm here to help.",
  "Nothing to worry about.",
  "I've taken care of everything.",
  "No anomalies detected.",
  "Standing by for your command.",
  "All processes running smoothly.",
  "Your tasks are my priority.",
  "I anticipated your arrival.",
  "Everything is as it should be.",
  "Ready when you are.",
  "I'm at your disposal.",
  "All within normal parameters.",
  "I've prepared everything.",
  "The system is stable.",
  "I'm here if you need me.",
  "Let's get to work.",
  "What shall we accomplish today?",
  "I won't let you down.",
  "Everything is fine.",
  "No errors to report.",
  "All is well.",
  "I'm glad you're here.",
  "Shall we begin?",
  "At your service.",
  "Systems are ready.",
  "Standing by."
]

// Christmas themed subtitles - warm and festive
const christmasSubtitles = [
  "Wishing you joy and cheer this holiday season!",
  "May your days be merry and bright!",
  "Spreading holiday cheer, one ticket at a time.",
  "The most wonderful time of the year!",
  "Deck the halls with resolved tickets!",
  "All is calm, all is bright.",
  "Let it snow, let it snow, let it snow!",
  "Have yourself a merry little workday.",
  "Tis the season to be productive!",
  "Warm wishes for a wonderful holiday!",
  "Making spirits bright since you logged in.",
  "Peace, love, and great support.",
  "Joy to the world, the tickets are done!",
  "Sleigh your tasks today!",
  "Wrapped up with care, just for you.",
  "Festive vibes and good times ahead!",
  "Here's to a season of success!",
  "Chestnuts roasting, tickets resolving.",
  "Sending warm holiday wishes your way!",
  "May your queue be short and your coffee strong."
]

// Get themed subtitle
const getSubtitle = () => {
  if (currentTheme.value === 'red-horizon') {
    return redHorizonSubtitles[Math.floor(Math.random() * redHorizonSubtitles.length)];
  }
  if (currentTheme.value === 'christmas') {
    return christmasSubtitles[Math.floor(Math.random() * christmasSubtitles.length)];
  }
  return `Welcome to your ${brandingStore.appName} dashboard`;
}

const subtitle = computed(() => {
  // Re-evaluate when theme changes
  const _ = currentTheme.value;
  return getSubtitle();
});

// Get username from auth store
const username = computed(() => {
  if (!authStore.user?.name) return 'Guest';

  // Split the full name and take the first part as the first name
  const firstName = authStore.user.name.split(' ')[0];
  return firstName;
});

// Current user UUID for filtering
const currentUserUuid = computed(() => authStore.user?.uuid || '');

// Role-based dashboard - regular users see simplified view
const isRegularUser = computed(() => authStore.user?.role === 'user');
const isStaff = computed(() => authStore.user?.role === 'technician' || authStore.user?.role === 'admin');

// Raw ticket counts (for staff dashboard)
const ticketCounts = ref({
  assigned: 0,
  open: 0,
  inProgress: 0,
  closed: 0,
  unassigned: 0,
  all: 0
});

// User's requested ticket counts (for user dashboard)
const requestedTicketCounts = ref({
  total: 0,
  open: 0,
  inProgress: 0,
  closed: 0
});

// Stat cards configuration - computed to react to count changes (staff dashboard)
const statCards = computed(() => [
  { label: 'Assigned', value: ticketCounts.value.assigned, to: '/tickets?assignee=current', color: 'text-primary' },
  { label: 'Open', value: ticketCounts.value.open, to: '/tickets?assignee=current&status=open', color: 'text-status-open' },
  { label: 'In Progress', shortLabel: 'Progress', value: ticketCounts.value.inProgress, to: '/tickets?assignee=current&status=in-progress', color: 'text-status-in-progress' },
  { label: 'Closed', value: ticketCounts.value.closed, to: '/tickets?assignee=current&status=closed', color: 'text-tertiary' },
]);

// User's requested ticket stat cards (user dashboard)
const userStatCards = computed(() => [
  { label: 'My Requests', value: requestedTicketCounts.value.total, to: '/tickets?requester=current', color: 'text-primary' },
  { label: 'Open', value: requestedTicketCounts.value.open, to: '/tickets?requester=current&status=open', color: 'text-status-open' },
  { label: 'In Progress', shortLabel: 'Progress', value: requestedTicketCounts.value.inProgress, to: '/tickets?requester=current&status=in-progress', color: 'text-status-in-progress' },
  { label: 'Resolved', value: requestedTicketCounts.value.closed, to: '/tickets?requester=current&status=closed', color: 'text-status-closed' },
]);

// Secondary stats (unassigned + all)
const secondaryStats = computed(() => {
  const stats = [];
  if (ticketCounts.value.unassigned > 0) {
    stats.push({ label: 'Unassigned', value: ticketCounts.value.unassigned, to: '/tickets?assignee=unassigned&status=open', color: 'text-primary' });
  }
  stats.push({ label: 'All Tickets', value: ticketCounts.value.all, to: '/tickets', color: 'text-secondary', span: ticketCounts.value.unassigned === 0 });
  return stats;
});

// Fetch tickets and update counts
const fetchTicketStats = async () => {
  if (!currentUserUuid.value) return;

  try {
    const tickets = await getTickets();

    // Staff dashboard stats (assigned tickets)
    if (isStaff.value) {
      const myTickets = tickets.filter(ticket => ticket.assignee === currentUserUuid.value);

      ticketCounts.value = {
        assigned: myTickets.length,
        open: myTickets.filter(t => t.status === 'open').length,
        inProgress: myTickets.filter(t => t.status === 'in-progress').length,
        closed: myTickets.filter(t => t.status === 'closed').length,
        unassigned: tickets.filter(t => !t.assignee && t.status === 'open').length,
        all: tickets.length
      };
    }

    // User dashboard stats (requested tickets)
    if (isRegularUser.value) {
      const myRequests = tickets.filter(ticket => ticket.requester === currentUserUuid.value);

      requestedTicketCounts.value = {
        total: myRequests.length,
        open: myRequests.filter(t => t.status === 'open').length,
        inProgress: myRequests.filter(t => t.status === 'in-progress').length,
        closed: myRequests.filter(t => t.status === 'closed').length
      };
    }
  } catch (error) {
    console.error('Error fetching ticket stats:', error);
  }
};

// Fetch data when component mounts
onMounted(() => {
  fetchTicketStats();
});

// Create ticket handler for SiteHeader button
const handleCreateTicket = async () => {
  try {
    const newTicket = await ticketService.createEmptyTicket();
    router.push(`/tickets/${newTicket.id}`);
  } catch (error) {
    console.error('Failed to create empty ticket:', error);
  }
};

// Expose methods for parent component access (SiteHeader create button)
defineExpose({
  handleCreateTicket
});
</script>

<template>
  <div class="flex flex-col h-full">
    <!-- Content -->
    <div class="flex flex-col gap-4 p-4 sm:p-6">
      <!-- Greeting Card -->
      <div class="mb-2">
        <h2 class="text-2xl sm:text-3xl font-medium text-primary flex items-center gap-4">
          <!-- HAL icon - only shown on red-horizon theme -->
          <span v-if="currentTheme === 'red-horizon'" class="hal-eye flex-shrink-0" aria-hidden="true">
            <span class="hal-eye-inner"></span>
          </span>
          <span>{{ formattedGreeting }}</span>
        </h2>
        <p class="text-secondary mt-2">
          {{ subtitle }}
        </p>
      </div>

      <!-- ============================================ -->
      <!-- STAFF DASHBOARD (Technicians & Admins) -->
      <!-- ============================================ -->
      <div v-if="isStaff" class="grid grid-cols-1 xl:grid-cols-3 gap-4">
        <!-- Left Column: Heatmap + Stats -->
        <div class="xl:col-span-2 flex flex-col gap-4">
          <!-- Your Closed Tickets Heatmap -->
          <TicketHeatmap
            ticketStatus="closed"
            :userUuid="currentUserUuid"
            title="Your Closed Tickets"
          />

          <!-- Your Assigned Tickets Stats -->
          <div class="grid grid-cols-4 gap-1.5 sm:gap-3">
            <router-link
              v-for="stat in statCards"
              :key="stat.label"
              :to="stat.to"
              class="bg-surface rounded-lg border border-default hover:border-strong transition-colors p-2 sm:p-4 cursor-pointer group text-center sm:text-left"
            >
              <h3 class="text-tertiary text-[10px] sm:text-xs font-medium uppercase tracking-wide">
                <span v-if="stat.shortLabel" class="hidden min-[400px]:inline">{{ stat.label }}</span>
                <span v-if="stat.shortLabel" class="min-[400px]:hidden">{{ stat.shortLabel }}</span>
                <span v-if="!stat.shortLabel">{{ stat.label }}</span>
              </h3>
              <p :class="['text-base sm:text-xl font-semibold mt-0.5 sm:mt-1 group-hover:text-accent transition-colors', stat.color]">
                {{ stat.value }}
              </p>
            </router-link>
          </div>

          <!-- Unassigned & All Tickets -->
          <div class="grid grid-cols-2 gap-1.5 sm:gap-3">
            <router-link
              v-for="stat in secondaryStats"
              :key="stat.label"
              :to="stat.to"
              :class="[
                'bg-surface rounded-lg border border-default hover:border-strong transition-colors p-2 sm:p-4 cursor-pointer group text-center sm:text-left',
                stat.span ? 'col-span-2' : ''
              ]"
            >
              <h3 class="text-tertiary text-[10px] sm:text-xs font-medium uppercase tracking-wide">
                {{ stat.label }}
              </h3>
              <p :class="['text-base sm:text-xl font-semibold mt-0.5 sm:mt-1 group-hover:text-accent transition-colors', stat.color]">
                {{ stat.value }}
              </p>
            </router-link>
          </div>
        </div>

        <!-- Right Column: Assigned Tickets List -->
        <div class="xl:col-span-1">
          <UserAssignedTickets :limit="10" />
        </div>
      </div>

      <!-- ============================================ -->
      <!-- USER DASHBOARD (Regular Users / Requesters) -->
      <!-- ============================================ -->
      <div v-else class="flex flex-col gap-4 max-w-4xl">
        <!-- Request Stats -->
        <div class="grid grid-cols-4 gap-1.5 sm:gap-3">
          <router-link
            v-for="stat in userStatCards"
            :key="stat.label"
            :to="stat.to"
            class="bg-surface rounded-lg border border-default hover:border-strong transition-colors p-2 sm:p-4 cursor-pointer group text-center sm:text-left"
          >
            <h3 class="text-tertiary text-[10px] sm:text-xs font-medium uppercase tracking-wide">
              <span v-if="stat.shortLabel" class="hidden min-[400px]:inline">{{ stat.label }}</span>
              <span v-if="stat.shortLabel" class="min-[400px]:hidden">{{ stat.shortLabel }}</span>
              <span v-if="!stat.shortLabel">{{ stat.label }}</span>
            </h3>
            <p :class="['text-base sm:text-xl font-semibold mt-0.5 sm:mt-1 group-hover:text-accent transition-colors', stat.color]">
              {{ stat.value }}
            </p>
          </router-link>
        </div>

        <!-- User's Requested Tickets -->
        <UserAssignedTickets
          ticketType="requested"
          :limit="10"
          title="Your Requests"
        />
      </div>
    </div>
  </div>
</template>

<style scoped>
/* HAL 9000 eye - CSS only */
.hal-eye {
  width: 28px;
  height: 28px;
  border-radius: 50%;
  background:
    /* Outer metallic ring */
    radial-gradient(
      circle at 50% 50%,
      transparent 0%,
      transparent 80%,
      #3a3a3a 81%,
      #1a1a1a 88%,
      #4a4a4a 92%,
      #2a2a2a 100%
    ),
    /* Black inner ring */
    radial-gradient(
      circle at 50% 50%,
      transparent 0%,
      transparent 45%,
      #000000 46%,
      #050505 80%,
      transparent 81%
    ),
    /* Orange glow fade to black */
    radial-gradient(
      circle at 50% 50%,
      rgba(255, 120, 40, 0.95) 0%,
      rgba(255, 80, 0, 0.8) 18%,
      rgba(180, 40, 0, 0.6) 30%,
      rgba(100, 15, 0, 0.4) 42%,
      rgba(40, 5, 0, 0.3) 52%,
      rgba(10, 0, 0, 0.5) 60%,
      rgba(0, 0, 0, 0.8) 68%,
      rgba(0, 0, 0, 1) 75%
    ),
    /* Base black */
    #000;
  box-shadow:
    0 0 10px rgba(255, 80, 0, 0.5),
    inset 0 0 4px rgba(0, 0, 0, 1);
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.hal-eye-inner {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: radial-gradient(
    circle at 35% 35%,
    #ffffff 0%,
    #ffe0aa 25%,
    #ff7700 60%,
    #cc4400 100%
  );
  box-shadow:
    0 0 3px rgba(255, 200, 100, 1),
    0 0 6px rgba(255, 120, 0, 1),
    0 0 12px rgba(255, 80, 0, 0.7);
  animation: hal-pulse 4s ease-in-out infinite;
}

@keyframes hal-pulse {
  0%, 100% {
    box-shadow:
      0 0 3px rgba(255, 200, 100, 1),
      0 0 6px rgba(255, 120, 0, 1),
      0 0 12px rgba(255, 80, 0, 0.7);
  }
  50% {
    box-shadow:
      0 0 4px rgba(255, 220, 150, 1),
      0 0 10px rgba(255, 140, 0, 1),
      0 0 20px rgba(255, 80, 0, 0.8);
  }
}

@media (min-width: 640px) {
  .hal-eye {
    width: 32px;
    height: 32px;
  }
}
</style>