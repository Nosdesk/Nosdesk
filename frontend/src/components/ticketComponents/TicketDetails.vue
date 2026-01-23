<script setup lang="ts">
import { computed, ref, watchEffect } from 'vue';
import QRCode from 'qrcode';
import UserAutocomplete from "@/components/ticketComponents/UserSelection.vue";
import CustomDropdown from "@/components/ticketComponents/CustomDropdown.vue";
import ContentEditable from "@/components/ticketComponents/ContentEditable.vue";
import SectionCard from "@/components/common/SectionCard.vue";
import UserAvatar from "@/components/UserAvatar.vue";
import LogoIcon from "@/components/icons/LogoIcon.vue";
import { useBrandingStore } from "@/stores/branding";

// Refs for user autocomplete components
const requesterRef = ref<InstanceType<typeof UserAutocomplete> | null>(null);
const assigneeRef = ref<InstanceType<typeof UserAutocomplete> | null>(null);

// QR code for print
const qrCodeDataUrl = ref<string | null>(null);

// Branding for print header
const brandingStore = useBrandingStore();
const customLogoUrl = computed(() => brandingStore.getLogoUrl(false)); // Light mode logo for print

interface UserInfo {
  uuid: string;
  name: string;
  avatar_url?: string | null;
  avatar_thumb?: string | null;
}

interface CategoryInfo {
  id: number;
  name: string;
  color?: string | null;
  icon?: string | null;
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
    category_id?: number | null;
    category?: CategoryInfo | null;
  };
  createdDate: string;
  modifiedDate: string;
  selectedStatus: string;
  selectedPriority: string;
  selectedCategory?: number | null;
  statusOptions: { value: string; label: string }[];
  priorityOptions: { value: string; label: string }[];
  categoryOptions?: { value: string; label: string; color?: string }[];
}>();

const emit = defineEmits<{
  (e: "update:selectedStatus", value: string): void;
  (e: "update:selectedPriority", value: string): void;
  (e: "update:selectedCategory", value: string): void;
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

// Print-friendly display values
const statusLabel = computed(() => {
  const option = props.statusOptions.find(o => o.value === props.selectedStatus);
  return option?.label || props.selectedStatus || 'Unknown';
});

const priorityLabel = computed(() => {
  const option = props.priorityOptions.find(o => o.value === props.selectedPriority);
  return option?.label || props.selectedPriority || 'Unknown';
});

const categoryLabel = computed(() => {
  if (!props.selectedCategory) return null;
  const option = props.categoryOptions?.find(o => o.value === String(props.selectedCategory));
  return option?.label || props.ticket.category?.name || null;
});

// Generate QR code for ticket URL (for print)
const ticketUrl = computed(() => {
  if (typeof window === 'undefined') return '';
  return `${window.location.origin}/tickets/${props.ticket.id}`;
});

watchEffect(async () => {
  if (props.ticket.id) {
    try {
      qrCodeDataUrl.value = await QRCode.toDataURL(ticketUrl.value, {
        width: 80,
        margin: 1,
        color: {
          dark: '#000000',
          light: '#ffffff'
        }
      });
    } catch (err) {
      console.error('Failed to generate QR code:', err);
    }
  }
});
</script>

<template>
  <div class="w-full">
    <!-- Print-only branding header -->
    <div class="hidden print:block print-branding-header">
      <img v-if="customLogoUrl" :src="customLogoUrl" alt="Logo" class="print-logo-image" />
      <LogoIcon v-else class="print-logo-icon" />
    </div>

    <!-- Print-only compact layout -->
    <div class="hidden print:block print-ticket-details">
      <!-- Header with ID and Title (full width) -->
      <div class="print-ticket-header">
        <span class="print-ticket-id">#{{ ticket.id }}</span>
        <h1 class="print-ticket-title">{{ ticket.title }}</h1>
      </div>

      <!-- Metadata Grid -->
      <div class="print-ticket-meta">
        <!-- Status & Priority Row -->
        <div class="print-meta-row">
          <div class="print-meta-item">
            <span class="print-meta-label">Status</span>
            <span class="print-badge" :class="`print-badge-${selectedStatus}`">{{ statusLabel }}</span>
          </div>
          <div class="print-meta-item">
            <span class="print-meta-label">Priority</span>
            <span class="print-badge" :class="`print-badge-${selectedPriority}`">{{ priorityLabel }}</span>
          </div>
          <div v-if="categoryLabel" class="print-meta-item">
            <span class="print-meta-label">Category</span>
            <span class="print-badge">{{ categoryLabel }}</span>
          </div>
        </div>

        <!-- People Row -->
        <div class="print-meta-row print-people-row">
          <div class="print-meta-item print-person">
            <span class="print-meta-label">Requester</span>
            <div v-if="ticket.requester_user" class="print-user">
              <UserAvatar
                :name="ticket.requester_user.uuid"
                :userName="ticket.requester_user.name"
                :avatar="ticket.requester_user.avatar_thumb || ticket.requester_user.avatar_url"
                size="sm"
                :showName="false"
                :clickable="false"
              />
              <span class="print-user-name">{{ ticket.requester_user.name }}</span>
            </div>
            <span v-else class="print-meta-empty">Unassigned</span>
          </div>
          <div class="print-meta-item print-person">
            <span class="print-meta-label">Assignee</span>
            <div v-if="ticket.assignee_user" class="print-user">
              <UserAvatar
                :name="ticket.assignee_user.uuid"
                :userName="ticket.assignee_user.name"
                :avatar="ticket.assignee_user.avatar_thumb || ticket.assignee_user.avatar_url"
                size="sm"
                :showName="false"
                :clickable="false"
              />
              <span class="print-user-name">{{ ticket.assignee_user.name }}</span>
            </div>
            <span v-else class="print-meta-empty">Unassigned</span>
          </div>
        </div>

        <!-- Dates Row -->
        <div class="print-meta-row print-dates-row">
          <div class="print-meta-item">
            <span class="print-meta-label">Created</span>
            <span class="print-meta-value">{{ createdDate }}</span>
          </div>
          <div class="print-meta-item">
            <span class="print-meta-label">Modified</span>
            <span class="print-meta-value">{{ modifiedDate }}</span>
          </div>
        </div>
      </div>

      <!-- QR Code (bottom right) -->
      <div v-if="qrCodeDataUrl" class="print-qr-code">
        <span class="print-qr-label">Scan to open</span>
        <img :src="qrCodeDataUrl" alt="Ticket QR Code" />
      </div>
    </div>

    <!-- Screen-only interactive layout -->
    <SectionCard class="print:hidden">
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
                <div class="print:hidden flex items-center gap-0.5">
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
                <div class="print:hidden flex items-center gap-0.5">
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

          <!-- Category Section -->
          <div v-if="categoryOptions && categoryOptions.length > 0" class="flex flex-col gap-1.5">
            <h3 class="text-xs font-medium text-tertiary uppercase tracking-wide">Category</h3>
            <div class="bg-surface-alt rounded-lg border border-subtle hover:border-default transition-colors">
              <CustomDropdown
                :value="selectedCategory?.toString() || ''"
                :options="categoryOptions"
                type="category"
                @update:value="emit('update:selectedCategory', $event)"
                class="w-full"
                placeholder="Select category..."
              />
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

<style scoped>
/* Print-specific ticket details layout */
@media print {
  /* Branding header above ticket details */
  .print-branding-header {
    margin-bottom: 12pt;
    display: flex;
    align-items: center;
  }

  .print-logo-image {
    height: 24pt !important;
    width: auto !important;
    max-height: 24pt !important;
  }

  .print-logo-icon {
    height: 20pt !important;
    width: auto !important;
    color: #000 !important;
  }

  .print-ticket-details {
    border: 1px solid #ccc;
    padding: 12pt;
    margin-bottom: 12pt;
    background: #fafafa;
  }

  .print-ticket-header {
    display: flex;
    align-items: baseline;
    gap: 8pt;
    margin-bottom: 10pt;
    padding-bottom: 8pt;
    border-bottom: 1px solid #ddd;
  }

  .print-ticket-id {
    font-family: ui-monospace, monospace;
    font-size: 11pt;
    font-weight: 600;
    color: #666;
  }

  .print-ticket-title {
    font-size: 14pt;
    font-weight: 600;
    color: #000;
    margin: 0;
    flex: 1;
  }

  .print-ticket-meta {
    display: flex;
    flex-direction: column;
    gap: 8pt;
  }

  .print-meta-row {
    display: flex;
    flex-wrap: wrap;
    gap: 16pt;
  }

  .print-meta-item {
    display: flex;
    flex-direction: column;
    gap: 2pt;
    min-width: 80pt;
  }

  .print-meta-label {
    font-size: 8pt;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5pt;
    color: #666;
  }

  .print-meta-value {
    font-size: 10pt;
    color: #333;
  }

  .print-meta-empty {
    font-size: 10pt;
    color: #999;
    font-style: italic;
  }

  .print-badge {
    display: inline-block;
    font-size: 9pt;
    font-weight: 500;
    padding: 2pt 6pt;
    border: 1px solid currentColor;
    border-radius: 3pt;
  }

  /* Status badge colors for print */
  .print-badge-open {
    color: #b45309;
    border-color: #b45309;
  }

  .print-badge-in_progress,
  .print-badge-in-progress {
    color: #1d4ed8;
    border-color: #1d4ed8;
  }

  .print-badge-closed {
    color: #047857;
    border-color: #047857;
  }

  /* Priority badge colors for print */
  .print-badge-high {
    color: #dc2626;
    border-color: #dc2626;
  }

  .print-badge-medium {
    color: #b45309;
    border-color: #b45309;
  }

  .print-badge-low {
    color: #047857;
    border-color: #047857;
  }

  .print-people-row {
    padding-top: 6pt;
    border-top: 1px solid #eee;
  }

  .print-person {
    min-width: 120pt;
  }

  .print-user {
    display: flex;
    align-items: center;
    gap: 6pt;
  }

  .print-user-name {
    font-size: 10pt;
    color: #333;
  }

  .print-dates-row {
    padding-top: 6pt;
    border-top: 1px solid #eee;
    font-size: 9pt;
  }

  /* Card needs relative positioning for QR code */
  .print-ticket-details {
    position: relative;
  }

  /* QR Code - top right of card */
  .print-qr-code {
    position: absolute;
    top: 12pt;
    right: 12pt;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2pt;
  }

  /* Offset header to make room for QR code */
  .print-ticket-header {
    margin-right: 72pt;
  }

  .print-qr-code img {
    width: 56pt !important;
    height: 56pt !important;
    max-width: 56pt !important;
    max-height: 56pt !important;
  }

  .print-qr-label {
    font-size: 6pt;
    color: #666;
    text-align: center;
  }
}
</style>