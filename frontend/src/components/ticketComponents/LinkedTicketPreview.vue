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
const isHovered = ref(false);

const isSameAsCurrentTicket = computed(() => {
  return props.currentTicketId && props.linkedTicketId === props.currentTicketId;
});

// Computed property to get status colors for the ticket badge
const ticketBadgeColors = computed(() => {
  if (!linkedTicket.value) return 'bg-slate-600/20 text-slate-700 dark:text-slate-300 border-slate-500/30';

  switch (linkedTicket.value.status) {
    case 'open':
      return 'bg-yellow-500/20 text-yellow-700 dark:text-yellow-300 border-yellow-500/30';
    case 'in-progress':
      return 'bg-blue-500/20 text-blue-700 dark:text-blue-300 border-blue-500/30';
    case 'closed':
      return 'bg-green-500/20 text-green-700 dark:text-green-300 border-green-500/30';
    default:
      return 'bg-slate-600/20 text-slate-700 dark:text-slate-300 border-slate-500/30';
  }
});

const handleMouseEnter = () => {
  isHovered.value = true;
};

const handleMouseLeave = () => {
  isHovered.value = false;
};

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
    class="bg-surface rounded-xl border border-default overflow-hidden hover:border-strong transition-colors"
    @mouseenter="handleMouseEnter"
    @mouseleave="handleMouseLeave"
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
        
        <!-- Title - extends to full width when buttons not shown -->
        <div class="min-w-0 flex-1">
          <h3 class="text-primary font-medium truncate text-md">
            {{ linkedTicket.title }}
          </h3>
        </div>
      </div>
      
      <!-- Action buttons - only render when hovering -->
      <div 
        v-if="isHovered"
        class="flex items-center gap-1 ml-3 flex-shrink-0 animate-in fade-in duration-200"
      >
        <button
          @click="viewTicket"
          :disabled="isNavigating"
          class="p-1.5 text-tertiary hover:text-primary hover:bg-surface-hover rounded-md transition-colors disabled:opacity-50"
          title="View ticket"
        >
          <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
            <path d="M10 12a2 2 0 100-4 2 2 0 000 4z" />
            <path
              fill-rule="evenodd"
              d="M.458 10C1.732 5.943 5.522 3 10 3s8.268 2.943 9.542 7c-1.274 4.057-5.064 7-9.542 7S1.732 14.057.458 10zM14 10a4 4 0 11-8 0 4 4 0 018 0z"
              clip-rule="evenodd"
            />
          </svg>
        </button>
        <button
          @click="emit('unlink')"
          :disabled="isNavigating"
          class="p-1.5 text-tertiary hover:text-red-400 hover:bg-red-900/20 rounded-md transition-colors disabled:opacity-50"
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
    </div>

    <!-- Ticket content -->
    <div class="p-4">
      <div class="flex flex-col gap-3">
        <!-- Details grid -->
        <div class="grid grid-cols-2 gap-3 text-sm">
          <div class="flex flex-col gap-1">
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
            <span class="text-xs text-tertiary uppercase tracking-wide">Assignee</span>
            <div class="flex items-center gap-2">
              <UserAvatar
                v-if="linkedTicket.assignee_user || linkedTicket.assignee"
                :name="linkedTicket.assignee_user?.name || linkedTicket.assignee"
                :avatarUrl="linkedTicket.assignee_user?.avatar_thumb"
                :userUuid="linkedTicket.assignee_user?.uuid"
                size="xs"
                :showName="true"
              />
              <span v-else class="text-secondary">Unassigned</span>
            </div>
          </div>
          <div class="flex flex-col gap-1">
            <span class="text-xs text-tertiary uppercase tracking-wide">Requester</span>
            <div class="flex items-center gap-2">
              <UserAvatar
                v-if="linkedTicket.requester_user || linkedTicket.requester"
                :name="linkedTicket.requester_user?.name || linkedTicket.requester"
                :avatarUrl="linkedTicket.requester_user?.avatar_thumb"
                :userUuid="linkedTicket.requester_user?.uuid"
                size="xs"
                :showName="true"
              />
              <span v-else class="text-secondary">None</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
