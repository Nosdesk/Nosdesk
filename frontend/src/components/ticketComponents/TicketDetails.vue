<script setup lang="ts">
import { computed, ref } from 'vue';
import UserAutocomplete from "@/components/ticketComponents/UserSelection.vue";
import CustomDropdown from "@/components/ticketComponents/CustomDropdown.vue";
import ContentEditable from "@/components/ticketComponents/ContentEditable.vue";
import SectionCard from "@/components/common/SectionCard.vue";

// Refs for user autocomplete components
const requesterRef = ref<InstanceType<typeof UserAutocomplete> | null>(null);
const assigneeRef = ref<InstanceType<typeof UserAutocomplete> | null>(null);

interface UserInfo {
  uuid: string;
  name: string;
  avatar_url?: string | null;
  avatar_thumb?: string | null;
}

const props = defineProps<{
  ticket: {
    id: number;
    title: string;
    status: string;
    priority: string;
    created?: string;
    modified?: string;
    assignee?: string;
    requester?: string;
    requester_user?: UserInfo | null;
    assignee_user?: UserInfo | null;
  };
  createdDate: string;
  modifiedDate: string;
  selectedStatus: string;
  selectedPriority: string;
  statusOptions: { value: string; label: string }[];
  priorityOptions: { value: string; label: string }[];
}>();

const emit = defineEmits<{
  (e: "update:selectedStatus", value: string): void;
  (e: "update:selectedPriority", value: string): void;
  (e: "update:requester", value: string): void;
  (e: "update:assignee", value: string): void;
  (e: "update:title", value: string): void;
}>();

// Computed values - single source of truth from props
const selectedRequester = computed(() =>
  props.ticket.requester_user?.uuid || props.ticket.requester || ""
);

const selectedAssignee = computed(() =>
  props.ticket.assignee_user?.uuid || props.ticket.assignee || ""
);

// Handle title update
const handleTitleUpdate = (newTitle: string) => {
  emit('update:title', newTitle);
};
</script>

<template>
  <div class="w-full">
    <SectionCard>
      <template #title>Ticket Details</template>

      <template #default>
        <div class="flex flex-col gap-3">
          <!-- Title Section -->
          <div class="flex flex-col gap-1.5">
            <h3 class="text-xs font-medium text-tertiary uppercase tracking-wide">Title</h3>
            <div class="bg-surface-alt rounded-lg border border-subtle hover:border-default transition-colors">
              <ContentEditable
                :modelValue="ticket.title || ''"
                @update:modelValue="handleTitleUpdate"
              />
            </div>
          </div>

          <!-- Assignment Section -->
          <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <!-- Requester -->
            <div class="flex flex-col gap-1.5">
              <div class="flex items-center justify-between">
                <h3 class="text-xs font-medium text-tertiary uppercase tracking-wide">Requester</h3>
                <div class="flex items-center gap-0.5">
                  <button
                    v-if="selectedRequester"
                    @click="emit('update:requester', '')"
                    class="p-1 text-tertiary hover:text-status-error hover:bg-status-error-muted rounded transition-colors"
                    type="button"
                    title="Clear requester"
                  >
                    <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
                    </svg>
                  </button>
                  <button
                    @click="requesterRef?.focus()"
                    class="p-1 text-tertiary hover:text-accent hover:bg-accent-muted rounded transition-colors"
                    type="button"
                    title="Add requester"
                  >
                    <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
                    </svg>
                  </button>
                </div>
              </div>
              <div class="bg-surface-alt rounded-lg border border-subtle hover:border-default transition-colors">
                <UserAutocomplete
                  ref="requesterRef"
                  :modelValue="selectedRequester"
                  @update:modelValue="emit('update:requester', $event)"
                  :currentUser="ticket.requester_user"
                  placeholder="Search or select requester..."
                  type="requester"
                  :hideInlineClear="true"
                  class="w-full"
                />
              </div>
            </div>

            <!-- Assignee -->
            <div class="flex flex-col gap-1.5">
              <div class="flex items-center justify-between">
                <h3 class="text-xs font-medium text-tertiary uppercase tracking-wide">Assignee</h3>
                <div class="flex items-center gap-0.5">
                  <button
                    v-if="selectedAssignee"
                    @click="emit('update:assignee', '')"
                    class="p-1 text-tertiary hover:text-status-error hover:bg-status-error-muted rounded transition-colors"
                    type="button"
                    title="Clear assignee"
                  >
                    <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
                    </svg>
                  </button>
                  <button
                    @click="assigneeRef?.focus()"
                    class="p-1 text-tertiary hover:text-accent hover:bg-accent-muted rounded transition-colors"
                    type="button"
                    title="Add assignee"
                  >
                    <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
                    </svg>
                  </button>
                </div>
              </div>
              <div class="bg-surface-alt rounded-lg border border-subtle hover:border-default transition-colors">
                <UserAutocomplete
                  ref="assigneeRef"
                  :modelValue="selectedAssignee"
                  @update:modelValue="emit('update:assignee', $event)"
                  :currentUser="ticket.assignee_user"
                  placeholder="Search or select assignee..."
                  type="assignee"
                  :hideInlineClear="true"
                  class="w-full"
                />
              </div>
            </div>
          </div>

          <!-- Status and Priority Section -->
          <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <!-- Status -->
            <div class="flex flex-col gap-1.5">
              <h3 class="text-xs font-medium text-tertiary uppercase tracking-wide">Status</h3>
              <div class="bg-surface-alt rounded-lg border border-subtle hover:border-default transition-colors">
                <CustomDropdown
                  :value="selectedStatus"
                  :options="statusOptions"
                  type="status"
                  @update:value="emit('update:selectedStatus', $event)"
                  class="w-full"
                />
              </div>
            </div>

            <!-- Priority -->
            <div class="flex flex-col gap-1.5">
              <h3 class="text-xs font-medium text-tertiary uppercase tracking-wide">Priority</h3>
              <div class="bg-surface-alt rounded-lg border border-subtle hover:border-default transition-colors">
                <CustomDropdown
                  :value="selectedPriority"
                  :options="priorityOptions"
                  type="priority"
                  @update:value="emit('update:selectedPriority', $event)"
                  class="w-full"
                />
              </div>
            </div>
          </div>

          <!-- Timestamps Section -->
          <div class="pt-2 border-t border-default">
            <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
              <!-- Created Date -->
              <div class="flex flex-col gap-1">
                <span class="text-xs text-tertiary uppercase tracking-wide font-medium">Created</span>
                <span class="text-secondary text-sm font-medium">{{ createdDate }}</span>
              </div>

              <!-- Modified Date -->
              <div class="flex flex-col gap-1">
                <span class="text-xs text-tertiary uppercase tracking-wide font-medium">Last Modified</span>
                <span class="text-secondary text-sm font-medium">{{ modifiedDate }}</span>
              </div>
            </div>
          </div>
        </div>
      </template>
    </SectionCard>
  </div>
</template>