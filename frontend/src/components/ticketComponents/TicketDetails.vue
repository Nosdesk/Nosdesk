<script setup lang="ts">
import { computed } from 'vue';
import UserAutocomplete from "@/components/ticketComponents/UserSelection.vue";
import CustomDropdown from "@/components/ticketComponents/CustomDropdown.vue";
import ContentEditable from "@/components/ticketComponents/ContentEditable.vue";

interface UserInfo {
  uuid: string;
  name: string;
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
    <div class="bg-slate-800 rounded-xl border border-slate-700/50 hover:border-slate-600/50 transition-colors">
      <!-- Header -->
      <div class="px-4 py-3 bg-slate-700/30 border-b border-slate-700/50">
        <h2 class="text-lg font-medium text-white">Ticket Details</h2>
      </div>
      
      <!-- Content -->
      <div class="p-3">
        <div class="flex flex-col gap-3">
          <!-- Title Section -->
          <div class="flex flex-col gap-1.5">
            <h3 class="text-xs font-medium text-slate-400 uppercase tracking-wide">Title</h3>
            <div class="bg-slate-700/50 rounded-lg border border-slate-600/30 hover:border-slate-500/50 transition-colors">
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
              <h3 class="text-xs font-medium text-slate-400 uppercase tracking-wide">Requester</h3>
              <div class="bg-slate-700/50 rounded-lg border border-slate-600/30 hover:border-slate-500/50 transition-colors">
                <UserAutocomplete
                  :modelValue="selectedRequester"
                  @update:modelValue="emit('update:requester', $event)"
                  :currentUser="ticket.requester_user"
                  placeholder="Search or select requester..."
                  type="requester"
                  class="w-full"
                />
              </div>
            </div>

            <!-- Assignee -->
            <div class="flex flex-col gap-1.5">
              <h3 class="text-xs font-medium text-slate-400 uppercase tracking-wide">Assignee</h3>
              <div class="bg-slate-700/50 rounded-lg border border-slate-600/30 hover:border-slate-500/50 transition-colors">
                <UserAutocomplete
                  :modelValue="selectedAssignee"
                  @update:modelValue="emit('update:assignee', $event)"
                  :currentUser="ticket.assignee_user"
                  placeholder="Search or select assignee..."
                  type="assignee"
                  class="w-full"
                />
              </div>
            </div>
          </div>

          <!-- Status and Priority Section -->
          <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <!-- Status -->
            <div class="flex flex-col gap-1.5">
              <h3 class="text-xs font-medium text-slate-400 uppercase tracking-wide">Status</h3>
              <div class="bg-slate-700/50 rounded-lg border border-slate-600/30 hover:border-slate-500/50 transition-colors">
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
              <h3 class="text-xs font-medium text-slate-400 uppercase tracking-wide">Priority</h3>
              <div class="bg-slate-700/50 rounded-lg border border-slate-600/30 hover:border-slate-500/50 transition-colors">
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
          <div class="pt-2 border-t border-slate-700/50">
            <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
              <!-- Created Date -->
              <div class="flex flex-col gap-1">
                <span class="text-xs text-slate-400 uppercase tracking-wide font-medium">Created</span>
                <span class="text-slate-200 text-sm font-medium">{{ createdDate }}</span>
              </div>

              <!-- Modified Date -->
              <div class="flex flex-col gap-1">
                <span class="text-xs text-slate-400 uppercase tracking-wide font-medium">Last Modified</span>
                <span class="text-slate-200 text-sm font-medium">{{ modifiedDate }}</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>