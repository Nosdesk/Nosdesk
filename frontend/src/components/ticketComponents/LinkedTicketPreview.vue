<script setup lang="ts">
import { formatDate } from '@/utils/dateUtils';
import { ref, onMounted, onBeforeUnmount, computed } from "vue";
import { useRouter } from "vue-router";
import { useFluent } from 'fluent-vue';
import StatusBadge from "@/components/StatusBadge.vue";
import UserAvatar from "@/components/UserAvatar.vue";
import SidebarCard from "@/components/ticketComponents/SidebarCard.vue";
import ticketService from "@/services/ticketService";
import type { Ticket } from "@/services/ticketService";

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

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
  if (isSameAsCurrentTicket.value) return;
  try {
    const fetchedTicket = await ticketService.getTicketById(props.linkedTicketId);
    if (fetchedTicket) {
      linkedTicket.value = fetchedTicket;
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
  if (!isSameAsCurrentTicket.value) fetchLinkedTicket();
});

onBeforeUnmount(() => {
  linkedTicket.value = null;
  isNavigating.value = false;
});

const formattedDate = (dateString: string) => formatDate(dateString, "MMM d, yyyy");
</script>

<template>
  <SidebarCard
    v-if="linkedTicket && !isSameAsCurrentTicket"
    :remove-title="t('ticket-chip-preview-unlink')"
    :remove-disabled="isNavigating"
    clickable
    @click="viewTicket"
    @remove="emit('unlink')"
  >
    <template #header>
      <span
        class="flex-shrink-0 inline-flex items-center px-2.5 py-1.5 rounded-md text-xs font-semibold"
        :class="ticketBadgeColors"
      >
        #{{ linkedTicket.id }}
      </span>
      <h3 class="truncate group-hover:text-accent transition-colors min-w-0 flex-1">
        {{ linkedTicket.title }}
      </h3>
    </template>

    <div class="grid grid-cols-2 gap-3 text-sm">
      <div class="flex flex-col gap-1 items-start">
        <span class="text-xs text-tertiary uppercase tracking-wide">{{ t('ticket-chip-preview-priority') }}</span>
        <StatusBadge type="priority" :value="linkedTicket.priority" short />
      </div>
      <div class="flex flex-col gap-1">
        <span class="text-xs text-tertiary uppercase tracking-wide">{{ t('ticket-chip-preview-created') }}</span>
        <span class="text-secondary">{{ formattedDate(linkedTicket.created) }}</span>
      </div>
      <div class="flex flex-col gap-1">
        <span class="text-xs text-tertiary uppercase tracking-wide">{{ t('ticket-chip-preview-requester') }}</span>
        <div
          v-if="linkedTicket.requester_user || linkedTicket.requester"
          @click.stop="router.push(`/users/${linkedTicket.requester_user?.uuid || linkedTicket.requester}`)"
          class="cursor-pointer hover:opacity-80 transition-opacity"
        >
          <UserAvatar
            :uuid="linkedTicket.requester_user?.uuid || linkedTicket.requester"
            :fallbackName="linkedTicket.requester_user?.name"
            :fallbackAvatar="linkedTicket.requester_user?.avatar_thumb"
            size="xs"
            :showName="true"
          />
        </div>
        <span v-else class="text-tertiary text-sm">{{ t('ticket-chip-preview-unassigned') }}</span>
      </div>
      <div class="flex flex-col gap-1">
        <span class="text-xs text-tertiary uppercase tracking-wide">{{ t('ticket-chip-preview-assignee') }}</span>
        <div
          v-if="linkedTicket.assignee_user || linkedTicket.assignee"
          @click.stop="router.push(`/users/${linkedTicket.assignee_user?.uuid || linkedTicket.assignee}`)"
          class="cursor-pointer hover:opacity-80 transition-opacity"
        >
          <UserAvatar
            :uuid="linkedTicket.assignee_user?.uuid || linkedTicket.assignee"
            :fallbackName="linkedTicket.assignee_user?.name"
            :fallbackAvatar="linkedTicket.assignee_user?.avatar_thumb"
            size="xs"
            :showName="true"
          />
        </div>
        <span v-else class="text-tertiary text-sm">{{ t('ticket-chip-preview-unassigned') }}</span>
      </div>
    </div>

    <template #print>
      <div class="hidden print:block print-linked-ticket">
        <span class="print-ticket-badge" :class="`print-status-${linkedTicket.status}`">#{{ linkedTicket.id }}</span>
        <span class="print-ticket-title">{{ linkedTicket.title }}</span>
        <span class="print-ticket-meta">
          <span class="print-priority" :class="`print-priority-${linkedTicket.priority}`">{{ linkedTicket.priority }}</span>
          <span v-if="linkedTicket.requester_user" class="print-ticket-user">{{ linkedTicket.requester_user.name }}</span>
        </span>
      </div>
    </template>
  </SidebarCard>
</template>

<style scoped>
@media print {
  .print-linked-ticket {
    border: 1px solid #ccc;
    padding: 6pt 8pt;
    margin-bottom: 4pt;
    background: #fafafa;
    font-size: 9pt;
    display: flex;
    align-items: center;
    gap: 8pt;
    flex-wrap: wrap;
  }

  .print-ticket-badge {
    font-family: ui-monospace, monospace;
    font-weight: 600;
    padding: 1pt 4pt;
    border: 1px solid currentColor;
    border-radius: 2pt;
    font-size: 8pt;
  }

  .print-status-open { color: #b45309; }
  .print-status-in-progress { color: #1d4ed8; }
  .print-status-closed { color: #047857; }

  .print-ticket-title {
    font-weight: 500;
    color: #000;
    flex: 1;
    min-width: 0;
  }

  .print-ticket-meta {
    display: flex;
    align-items: center;
    gap: 8pt;
    color: #666;
    font-size: 8pt;
  }

  .print-priority {
    padding: 1pt 4pt;
    border: 1px solid currentColor;
    border-radius: 2pt;
    text-transform: capitalize;
  }

  .print-priority-high { color: #dc2626; }
  .print-priority-medium { color: #b45309; }
  .print-priority-low { color: #047857; }
  .print-ticket-user { color: #333; }
}
</style>
