<script setup lang="ts">
import { computed, ref, watchEffect } from 'vue';
import type { TicketStatus, TicketPriority } from '@/constants/ticketOptions';
import { useWorkflowStatesStore } from '@/stores/workflowStates';
import {
  CATEGORY_LABELS,
  WORKFLOW_CATEGORIES,
  categoryHeaderValue,
  isCategoryHeaderValue,
} from '@/types/workflow';
import QRCode from 'qrcode';
import UserPicker from "@/components/ticketComponents/UserPicker.vue";
import CustomDropdown from "@/components/ticketComponents/CustomDropdown.vue";
import ContentEditable from "@/components/ticketComponents/ContentEditable.vue";
import SectionCard from "@/components/common/SectionCard.vue";
import Icon from "@/components/common/Icon.vue";
import UserAvatar from "@/components/UserAvatar.vue";
import LogoIcon from "@/components/icons/LogoIcon.vue";
import { useBrandingStore } from "@/stores/branding";
import { useAuthStore } from "@/stores/auth";

// Refs for user picker components
const requesterRef = ref<InstanceType<typeof UserPicker> | null>(null);
const assigneeRef = ref<InstanceType<typeof UserPicker> | null>(null);

// Auth context for the "Claim" / "Unassign me" affordance.
// The button only renders when the signed-in account is eligible to
// receive ticket assignments (admin / technician).
const authStore = useAuthStore();
const canSelfAssign = computed(() => authStore.isTechnician);
const isAssignedToMe = computed(
  () => !!authStore.user && selectedAssignee.value === authStore.user.uuid,
);
function toggleSelfAssign() {
  const me = authStore.user;
  if (!me) return;
  emit('update:assignee', isAssignedToMe.value ? '' : me.uuid);
}

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
    /** Opened via a channel (email_imap, ...); drives the "via email" header pill. */
    origin_channel_id?: number | null;
    /** Provider string mirrored from the channel. */
    submitted_via?: string | null;
    /** Calendar deadline. ISO string (RFC3339) or null. */
    due_date?: string | null;
    /** RFC 5545 RRULE string for recurring tickets, or null. */
    recurrence_rule?: string | null;
  };
  createdDate: string;
  modifiedDate: string;
  selectedStatus: string;
  selectedPriority: string;
  selectedCategory?: number | null;
  selectedWorkflowStateId?: number | null;
  statusOptions: { value: string; label: string }[];
  priorityOptions: { value: string; label: string }[];
  categoryOptions?: { value: string; label: string; color?: string }[];
}>();

const emit = defineEmits<{
  (e: "update:selectedStatus", value: TicketStatus): void;
  (e: "update:selectedWorkflowStateId", value: number): void;
  (e: "update:selectedPriority", value: TicketPriority): void;
  (e: "update:selectedCategory", value: string): void;
  (e: "update:requester", value: string): void;
  (e: "update:assignee", value: string): void;
  (e: "update:title", value: string): void;
  (e: "titleFocus"): void;
  (e: "titleBlur"): void;
  /** ISO string (start-of-day in user TZ) or null when cleared. */
  (e: "update:dueDate", value: string | null): void;
  /** RRULE string or null when cleared. */
  (e: "update:recurrenceRule", value: string | null): void;
}>();

const workflowStatesStore = useWorkflowStatesStore();

/**
 * Workflow state options for the status dropdown, grouped by category.
 * Categories are emitted as non-selectable header rows (`disabled: true`)
 * so the picker shows the structure without letting the user pick the
 * category itself. Empty categories are skipped.
 *
 * Falls back to the legacy three-bucket statusOptions when the store
 * hasn't loaded yet.
 */
const workflowDropdownOptions = computed<
  { value: string; label: string; disabled?: boolean; color?: string }[]
>(() => {
  if (!workflowStatesStore.loaded || workflowStatesStore.states.length === 0) {
    return props.statusOptions;
  }
  const out: { value: string; label: string; disabled?: boolean; color?: string }[] = [];
  for (const cat of WORKFLOW_CATEGORIES) {
    const states = workflowStatesStore.byCategory[cat];
    if (!states || states.length === 0) continue;
    out.push({ value: categoryHeaderValue(cat), label: CATEGORY_LABELS[cat], disabled: true });
    for (const s of states) {
      out.push({ value: String(s.id), label: s.name, color: s.color });
    }
  }
  return out;
});

const usingWorkflowDropdown = computed(
  () => workflowStatesStore.loaded && workflowStatesStore.states.length > 0,
);

const workflowDropdownValue = computed(() => {
  if (props.selectedWorkflowStateId != null) return String(props.selectedWorkflowStateId);
  return props.selectedStatus;
});

function handleStatusDropdownChange(v: string) {
  if (isCategoryHeaderValue(v)) return; // header row; ignore
  if (usingWorkflowDropdown.value) {
    const id = Number(v);
    if (Number.isFinite(id)) emit('update:selectedWorkflowStateId', id);
    return;
  }
  emit('update:selectedStatus', v as TicketStatus);
}

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

/** Backend stores due_date as a TIMESTAMPTZ; the date input wants
 * a `YYYY-MM-DD` string. Slicing the ISO string is sufficient
 * because the calendar view buckets cards by local-day, so any
 * additional precision would be misleading. */
const dueDateInputValue = computed<string>(() => {
  if (!props.ticket.due_date) return '';
  return props.ticket.due_date.slice(0, 10);
});

function handleDueDateChange(event: Event): void {
  const value = (event.target as HTMLInputElement).value;
  if (!value) {
    emit('update:dueDate', null);
    return;
  }
  // Anchor at start-of-day in the user's local timezone, then
  // serialise to RFC3339 for the API. The backend persists the
  // timezone so round-tripping is unambiguous.
  const local = new Date(`${value}T00:00:00`);
  emit('update:dueDate', local.toISOString());
}

/** Recurrence preset that maps to a known RRULE string. The picker
 * exposes a small list rather than the full RFC; an admin who
 * needs WEEKDAYS-only or interval=2 rules can edit the raw string
 * directly through the API. */
const RECURRENCE_PRESETS: { value: string; label: string }[] = [
  { value: '', label: 'Not recurring' },
  { value: 'FREQ=DAILY', label: 'Daily' },
  { value: 'FREQ=WEEKLY', label: 'Weekly' },
  { value: 'FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR', label: 'Weekdays' },
  { value: 'FREQ=MONTHLY', label: 'Monthly' },
  { value: 'FREQ=YEARLY', label: 'Yearly' },
];

const RECURRENCE_LABELS: Record<string, string> = {
  'FREQ=DAILY': 'Daily',
  'FREQ=WEEKLY': 'Weekly',
  'FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR': 'Weekdays',
  'FREQ=MONTHLY': 'Monthly',
  'FREQ=YEARLY': 'Yearly',
};

/** True when the ticket carries either a due date or a recurrence
 * rule — drives whether the Scheduling group opens by default. */
const schedulingHasValue = computed<boolean>(() => {
  return !!(props.ticket.due_date || props.ticket.recurrence_rule);
});

/** Inline preview rendered in the Scheduling summary line so the
 * user can read state without expanding. Empty string falls back
 * to "None" in the template. */
const schedulingPreview = computed<string>(() => {
  const parts: string[] = [];
  if (props.ticket.due_date) {
    const formatted = new Date(props.ticket.due_date).toLocaleDateString(undefined, {
      month: 'short',
      day: 'numeric',
    });
    parts.push(`Due ${formatted}`);
  }
  const rule = props.ticket.recurrence_rule;
  if (rule) {
    parts.push(RECURRENCE_LABELS[rule] ?? 'Recurring');
  }
  return parts.join(' · ');
});

const recurrenceSelectValue = computed<string>(() => {
  const rule = props.ticket.recurrence_rule ?? '';
  // Show 'custom' when the rule isn't one of our presets so the
  // dropdown stays honest about not being able to edit it here.
  if (!rule) return '';
  return RECURRENCE_PRESETS.some(p => p.value === rule) ? rule : '__custom__';
});

function handleRecurrenceChange(event: Event): void {
  const value = (event.target as HTMLSelectElement).value;
  if (value === '__custom__') return; // no-op; custom rules are read-only in this picker
  emit('update:recurrenceRule', value || null);
}

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
            <div class="flex items-center justify-between gap-2">
              <h3 class="text-xs font-medium text-tertiary uppercase tracking-wide">Title</h3>
              <!-- Origin-channel hint: small badge when the ticket was
                   opened via the email / chat ingestion pipeline. Lets
                   techs spot at a glance that a reply will be relayed
                   back to the requester's inbox. -->
              <span
                v-if="ticket.origin_channel_id"
                class="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-semibold uppercase tracking-wide bg-accent-muted text-accent"
                :title="`Opened via ${ticket.submitted_via ?? 'channel'} — replies are relayed back through the thread`"
              >
                <Icon name="email" size="xs" />
                via {{ ticket.submitted_via === 'email_imap' ? 'email' : ticket.submitted_via ?? 'channel' }}
              </span>
            </div>
            <div class="bg-surface-alt rounded-lg border border-subtle hover:border-default transition-colors">
              <ContentEditable
                :modelValue="ticket.title || ''"
                @update:modelValue="handleTitleUpdate"
                @focus="emit('titleFocus')"
                @blur="emit('titleBlur')"
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
                    <Icon name="close" />
                  </button>
                  <button
                    @click="requesterRef?.focus()"
                    class="p-1 text-tertiary hover:text-accent hover:bg-accent-muted rounded transition-colors"
                    type="button"
                    title="Add requester"
                  >
                    <Icon name="add" />
                  </button>
                </div>
              </div>
              <div class="bg-surface-alt rounded-lg border border-subtle hover:border-default transition-colors">
                <UserPicker
                  ref="requesterRef"
                  :modelValue="selectedRequester"
                  @update:modelValue="emit('update:requester', $event)"
                  :currentUser="ticket.requester_user"
                  placeholder="Find a user..."
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
                <div class="print:hidden flex items-center gap-1">
                  <!-- One-click self-assign for staff. Only surfaced
                       on unassigned tickets — taking an unassigned
                       ticket is the daily-driver case, while reassign-
                       from-someone-else is a deliberate action that
                       should go through the picker. -->
                  <button
                    v-if="canSelfAssign && !selectedAssignee"
                    @click="toggleSelfAssign"
                    type="button"
                    class="text-[11px] font-medium px-2 h-6 rounded text-accent hover:bg-accent-muted transition-colors"
                    title="Assign this ticket to yourself"
                  >
                    Claim
                  </button>
                  <button
                    v-if="selectedAssignee"
                    @click="emit('update:assignee', '')"
                    class="p-1 text-tertiary hover:text-status-error hover:bg-status-error-muted rounded transition-colors"
                    type="button"
                    title="Clear assignee"
                  >
                    <Icon name="close" />
                  </button>
                  <button
                    @click="assigneeRef?.focus()"
                    class="p-1 text-tertiary hover:text-accent hover:bg-accent-muted rounded transition-colors"
                    type="button"
                    title="Add assignee"
                  >
                    <Icon name="add" />
                  </button>
                </div>
              </div>
              <div class="bg-surface-alt rounded-lg border border-subtle hover:border-default transition-colors">
                <UserPicker
                  ref="assigneeRef"
                  :modelValue="selectedAssignee"
                  @update:modelValue="emit('update:assignee', $event)"
                  :currentUser="ticket.assignee_user"
                  placeholder="Assign to..."
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
                  :value="workflowDropdownValue"
                  :options="workflowDropdownOptions"
                  type="status"
                  @update:value="handleStatusDropdownChange"
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
                  @update:value="(v: string) => emit('update:selectedPriority', v as TicketPriority)"
                  class="w-full"
                />
              </div>
            </div>
          </div>

          <!-- Scheduling group: due date + recurrence collapsed
               by default. Most tickets don't carry either; folding
               them keeps the form short for the common case while
               preserving discoverability. The summary line shows
               an inline preview ("Due Jan 14 · Weekly") so users
               can read the state without expanding. -->
          <details
            class="rounded-lg border border-subtle bg-surface-alt"
            :open="schedulingHasValue"
          >
            <summary
              class="flex items-center justify-between cursor-pointer px-3 py-2 text-xs font-medium text-tertiary uppercase tracking-wide select-none"
            >
              <span>Scheduling</span>
              <span class="text-[11px] normal-case tracking-normal text-secondary font-normal">
                {{ schedulingPreview || 'None' }}
              </span>
            </summary>
            <div class="px-3 pb-3 pt-1 flex flex-col gap-3 border-t border-subtle">
              <label class="flex flex-col gap-1">
                <span class="text-[11px] text-tertiary">Due date</span>
                <div class="flex items-center bg-app rounded-md border border-subtle">
                  <input
                    type="date"
                    class="flex-1 bg-transparent text-sm text-primary px-2 py-1.5 outline-none"
                    :value="dueDateInputValue"
                    @change="handleDueDateChange"
                  />
                  <button
                    v-if="ticket.due_date"
                    type="button"
                    class="text-xs text-tertiary hover:text-primary px-2"
                    title="Clear due date"
                    @click="emit('update:dueDate', null)"
                  >×</button>
                </div>
              </label>
              <label class="flex flex-col gap-1">
                <span class="text-[11px] text-tertiary">Recurrence</span>
                <select
                  class="bg-app border border-subtle rounded-md text-sm px-2 py-1.5 text-primary"
                  :value="recurrenceSelectValue"
                  @change="handleRecurrenceChange"
                >
                  <option v-for="preset in RECURRENCE_PRESETS" :key="preset.value" :value="preset.value">
                    {{ preset.label }}
                  </option>
                </select>
                <span
                  v-if="recurrenceSelectValue === '__custom__'"
                  class="text-[10px] text-tertiary italic"
                >Custom RRULE in use ({{ ticket.recurrence_rule }}). Edit via API.</span>
                <span
                  v-else-if="ticket.recurrence_rule"
                  class="text-[10px] text-tertiary italic"
                >Closing this ticket spawns the next occurrence.</span>
              </label>
            </div>
          </details>

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