<script setup lang="ts">
import { formatDate, formatDateTime } from '@/utils/dateUtils';
import { ref, onMounted, onBeforeUnmount, computed } from "vue";
import { useRouter } from "vue-router";
import type { TicketStatus, TicketPriority } from "@/constants/ticketOptions";
import StatusBadge from "@/components/StatusBadge.vue";
import UserAvatar from "@/components/UserAvatar.vue";
import ticketService from "@/services/ticketService";
import type { Ticket, Device } from "@/services/ticketService";

const props = defineProps<{
  linkedTicketId: number;
  currentTicketId?: number;
}>();

const emit = defineEmits<{
  (e: "unlink"): void;
  (e: "view"): void;
}>();

const router = useRouter();
const linkedTicket = ref<Ticket | null>(null);
const isNavigating = ref(false);

const isSameAsCurrentTicket = computed(() => {
  return props.currentTicketId && props.linkedTicketId === props.currentTicketId;
});

// Computed property to get status colors for the ticket badge
const ticketBadgeColors = computed(() => {
  if (!linkedTicket.value) return 'bg-surface-alt text-secondary border-default';

  switch (linkedTicket.value.status) {
    case 'open':
      return 'bg-status-warning/20 text-status-warning border-status-warning/30';
    case 'in-progress':
      return 'bg-accent/15 dark:bg-accent/20 [color:#1e3a8a] dark:text-accent border-accent/30 dark:border-accent/30';
    case 'closed':
      return 'bg-status-success/20 text-status-success border-status-success/30';
    default:
      return 'bg-surface-alt text-secondary border-default';
  }
});

const fetchLinkedTicket = async () => {
  if (isSameAsCurrentTicket.value) {
    if (import.meta.env.DEV) {
      console.log(`Skipping fetch for ticket #${props.linkedTicketId} as it's the same as the current ticket #${props.currentTicketId}`);
    }
    return;
  }
  
  try {
    if (import.meta.env.DEV) {
      console.log(`Fetching linked ticket #${props.linkedTicketId}`);
    }
    const fetchedTicket = await ticketService.getTicketById(props.linkedTicketId);
    
    if (fetchedTicket) {
      linkedTicket.value = fetchedTicket;
      if (import.meta.env.DEV) {
        console.log(`Successfully fetched linked ticket #${props.linkedTicketId}:`, fetchedTicket);
      }
    }
  } catch (error) {
    console.error(`Error fetching linked ticket #${props.linkedTicketId}:`, error);
  }
};

const viewTicket = async () => {
  emit("view");

  if (isNavigating.value || !props.linkedTicketId) return;

  try {
    isNavigating.value = true;
    await router.push(`/tickets/${props.linkedTicketId}`);
  } catch (error) {
    console.error("Navigation error:", error);
    isNavigating.value = false;
  }
};

onMounted(() => {
  if (isSameAsCurrentTicket.value) {
    if (import.meta.env.DEV) {
      console.log(`Skipping fetch for ticket #${props.linkedTicketId} as it's the same as the current ticket #${props.currentTicketId}`);
    }
    return;
  }
  fetchLinkedTicket();
});

onBeforeUnmount(() => {
  linkedTicket.value = null;
  isNavigating.value = false;
});

const formattedDate = (dateString: string) => {
  const date = new Date(dateString);
  return formatDate(dateString, "MMM d, yyyy");
};
</script>

<template>
  <div
    v-if="linkedTicket && !isSameAsCurrentTicket"
    class="group bg-surface rounded-xl border border-default overflow-hidden hover:border-strong transition-colors"
  >
    <!-- Header with status and actions -->
    <div class="px-4 py-3 bg-surface-alt border-b border-default flex items-center">
      <div class="flex items-center gap-3 min-w-0 flex-1">
        <!-- Ticket Number Badge -->
        <div class="flex-shrink-0">
          <span class="inline-flex items-center px-2.5 py-1.5 rounded-md text-xs font-semibold" :class="ticketBadgeColors">
            #{{ linkedTicket.id }}
          </span>
        </div>
        
        <!-- Title - clickable to navigate to ticket -->
        <div class="min-w-0 flex-1">
          <h3
            @click="viewTicket"
            class="text-primary font-medium truncate text-md cursor-pointer hover:text-accent transition-colors"
          >
            {{ linkedTicket.title }}
          </h3>
        </div>
      </div>
      
      <!-- Action button -->
      <button
        @click="emit('unlink')"
        :disabled="isNavigating"
        class="p-1.5 ml-3 flex-shrink-0 text-tertiary hover:text-status-error hover:bg-status-error/20 rounded-md transition-colors disabled:opacity-50"
        title="Unlink ticket"
      >
        <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
          <path
            fill-rule="evenodd"
            d="M4.293 4.293a1 1 0 011.414 0L10 8.586l4.293-4.293a1 1 0 111.414 1.414L11.414 10l4.293 4.293a1 1 0 01-1.414 1.414L10 11.414l-4.293 4.293a1 1 0 01-1.414-1.414L8.586 10 4.293 5.707a1 1 0 010-1.414z"
            clip-rule="evenodd"
          />
        </svg>
      </button>
    </div>

    <!-- Ticket content -->
    <div class="p-4">
      <div class="flex flex-col gap-3">
        <!-- Details grid -->
        <div class="grid grid-cols-2 gap-3 text-sm">
          <div class="flex flex-col gap-1 items-start">
            <span class="text-xs text-tertiary uppercase tracking-wide">Priority</span>
            <StatusBadge type="priority" :value="linkedTicket.priority" short />
          </div>
          <div class="flex flex-col gap-1">
            <span class="text-xs text-tertiary uppercase tracking-wide">Created</span>
            <span class="text-secondary">{{
              formattedDate(linkedTicket.created)
            }}</span>
          </div>
          <div class="flex flex-col gap-1">
            <span class="text-xs text-tertiary uppercase tracking-wide">Requester</span>
            <UserAvatar
              v-if="linkedTicket.requester_user || linkedTicket.requester"
              :name="linkedTicket.requester_user?.uuid || linkedTicket.requester"
              :userName="linkedTicket.requester_user?.name"
              :avatar="linkedTicket.requester_user?.avatar_thumb"
              size="xs"
              :showName="true"
            />
            <span v-else class="text-tertiary text-sm">Unassigned</span>
          </div>
          <div class="flex flex-col gap-1">
            <span class="text-xs text-tertiary uppercase tracking-wide">Assignee</span>
            <UserAvatar
              v-if="linkedTicket.assignee_user || linkedTicket.assignee"
              :name="linkedTicket.assignee_user?.uuid || linkedTicket.assignee"
              :userName="linkedTicket.assignee_user?.name"
              :avatar="linkedTicket.assignee_user?.avatar_thumb"
              size="xs"
              :showName="true"
            />
            <span v-else class="text-tertiary text-sm">Unassigned</span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
