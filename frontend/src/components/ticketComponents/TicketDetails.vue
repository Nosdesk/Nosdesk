<script setup lang="ts">
import { computed, ref, watchEffect } from 'vue';
import { useRouter } from 'vue-router';
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
import UserCell from "@/components/views/UserCell.vue";
import TicketTagsField from "@/components/ticketComponents/TicketTagsField.vue";
import TicketWatchersField from "@/components/ticketComponents/TicketWatchersField.vue";
import TicketDevicesField from "@/components/ticketComponents/TicketDevicesField.vue";
import TicketLinkedTicketsField from "@/components/ticketComponents/TicketLinkedTicketsField.vue";
import TicketProjectsField from "@/components/ticketComponents/TicketProjectsField.vue";
import TicketLinkedDocs from "@/components/ticketComponents/TicketLinkedDocs.vue";
import type { Device } from "@/types/device";
import LogoIcon from "@/components/icons/LogoIcon.vue";
import { useBrandingStore } from "@/stores/branding";
import { useAuthStore } from "@/stores/auth";
import { deriveSlaState, type SlaPayload } from "@/composables/useSlaState";
import { formatCompactDate } from "@/utils/dateUtils";

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
    /** Actor uuids — populated by the detail handler. The
     *  sidebar's audit block resolves these to a user avatar
     *  via the directory composable (sync engine pool). */
    created_by?: string | null;
    closed_by?: string | null;
    closed_at?: string | null;
    /** SLA pill payload mirrored from the list view. The detail
     *  handler computes the same shape on read so this sidebar
     *  can render the countdown without a second round-trip. */
    sla?: SlaPayload | null;
    /** Cycle membership embedded by the detail handler. The chip
     *  is clickable, navigates to /cycles/:projectId / cycle uuid
     *  if/when that route exists; today the navigation just lands
     *  on the project's cycles surface. */
    cycle?: {
      id: number;
      uuid: string;
      project_id: number;
      name: string;
      state: string;
    } | null;
    /** Free-text "what fixed this?" capture. Surfaced as a
     *  dedicated section in the sidebar; styled prominently
     *  when the ticket has landed in a terminal workflow state
     *  (done / cancelled). */
    resolution_notes?: string | null;
    /** Tag ids attached to this ticket. The TicketTagsField
     *  component resolves each id to a Tag row via the workspace
     *  tag store and renders the chip + picker. */
    tag_ids?: number[];
    /** Uuids of users watching this ticket. The TicketWatchersField
     *  surface renders the bell toggle for the current user plus
     *  an avatar row for the broader watcher set. */
    watcher_uuids?: string[];
    /** Project membership. Backend returns either a list of ids
     *  or a list of full project objects depending on the
     *  endpoint; the field component normalises to ids. */
    projects?: string[] | { id: number | string }[];
    /** Linked-ticket ids. */
    linkedTickets?: number[];
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
  /** Devices attached to the ticket. Renders as a property-row of
   *  device chips below the standard properties. */
  devices?: Device[];
  /** Drag-to-link affordance state for the linked-tickets row. */
  showLinkDropAffordance?: boolean;
  isLinkDropTarget?: boolean;
  linkDropDragLabel?: string | null;
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
  /** Resolution notes — empty string normalises to null upstream
   *  in `useTicketData` so this emit can use either shape. */
  (e: "update:resolutionNotes", value: string | null): void;
  /** Replace the ticket's tag set. Backend computes the diff. */
  (e: "update:tag-ids", value: number[]): void;
  /** Toggle the current user's watch status on this ticket. */
  (e: "toggle-watch"): void;
  /** Open the device-attach modal. Modal state lives in the
   *  parent (TicketView) since the modal renders at page scope. */
  (e: "add-device"): void;
  /** Detach a device from the ticket. */
  (e: "remove-device", deviceId: number): void;
  /** Open the link-ticket modal. */
  (e: "add-linked-ticket"): void;
  /** Unlink a ticket. */
  (e: "remove-linked-ticket", ticketId: number): void;
  /** Open the project-attach modal. */
  (e: "add-project"): void;
  /** Detach this ticket from a project. */
  (e: "remove-project", projectId: string): void;
  /** Promote this ticket to a documentation page. */
  (e: "save-as-doc"): void;
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

// Project membership comes through as either ids (string[]) or
// full project rows depending on the upstream call. Normalise to
// a string id list so the property-row chip resolver gets one
// shape to work with.
const normalisedProjectIds = computed<string[]>(() => {
  const raw = props.ticket.projects;
  if (!raw) return [];
  if (raw.length === 0) return [];
  if (typeof raw[0] === 'string') return raw as string[];
  return (raw as { id: number | string }[]).map((p) => String(p.id));
});

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

const schedulingOpen = ref<boolean>(schedulingHasValue.value);

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

// ---- Source / channel readout ----------------------------------
//
// Promotes the inline "via email" pill out of the title section
// into a dedicated metadata row. The pill in the title compressed
// the channel into a 4-char glyph, which read fine for power users
// but cost first-time clarity. A proper row with icon + provider
// + clarification text reads as a real metadata field rather than
// a header badge.
const sourceLabel = computed<string | null>(() => {
  if (!props.ticket.origin_channel_id) return null;
  const provider = props.ticket.submitted_via ?? 'channel';
  if (provider === 'email_imap') return 'Email';
  if (provider === 'email_smtp') return 'Email';
  if (provider === 'slack') return 'Slack';
  if (provider === 'teams') return 'Microsoft Teams';
  // Fall through to the raw provider name for channels we
  // haven't pretty-named yet — better than masking the source.
  return provider;
});

// ---- SLA pill ---------------------------------------------------
//
// Same `deriveSlaState` the list view uses. The detail sidebar
// renders the longer `statusLabel` ("Breached" / "At risk" / "On
// track" / "Paused") + the compact countdown so the pill carries
// the same operational urgency as the table column without
// duplicating the logic.
const slaState = computed(() => deriveSlaState(props.ticket.sla ?? null));

// ---- Cycle pill -------------------------------------------------

const router = useRouter();

function openCycle() {
  // The cycles surface lives at /cycles for the workspace overview;
  // a per-cycle detail route hasn't shipped, so the chip currently
  // navigates to the workspace cycles board. Wired here so the
  // chip's affordance reads as actionable today and the destination
  // can swap to a cycle-specific URL without touching this file.
  if (props.ticket.cycle) {
    void router.push('/cycles');
  }
}

// ---- Resolution notes -------------------------------------------
//
// Local mirror of the prop so the textarea can be controlled
// without firing an emit per keystroke. We commit on blur (and
// debounce naturally there) — same pattern Linear / Plain use
// for free-text fields. Watching the prop keeps the local state
// in sync when the ticket reloads (e.g. after a different actor's
// update lands via SSE).
const localResolutionNotes = ref<string>(props.ticket.resolution_notes ?? '');
watchEffect(() => {
  localResolutionNotes.value = props.ticket.resolution_notes ?? '';
});

function handleResolutionBlur() {
  const next = localResolutionNotes.value.trim();
  const current = (props.ticket.resolution_notes ?? '').trim();
  if (next === current) return;
  emit('update:resolutionNotes', next.length > 0 ? next : null);
}

// Terminal state lookup for the visual treatment. Workflow states
// are workspace-configurable; their `category` (one of the six
// system categories) tells us whether the state is terminal.
// Falls back to "not terminal" when the workflow store hasn't
// loaded yet — the user just gets the muted styling until the
// store warms.
const isTerminalState = computed<boolean>(() => {
  const id = props.selectedWorkflowStateId;
  if (id == null) return false;
  const cat = workflowStatesStore.findById(id)?.category;
  return cat === 'done' || cat === 'cancelled';
});

// ---- Audit timestamps -------------------------------------------
//
// `formatCompactDate` returns a short relative-or-date string the
// list view's date cells use. Pairing with the actor uuid (via
// `<UserCell>` which already exists for the table) gives the audit
// block a consistent feel with the rest of the app.
const closedDateLabel = computed<string>(() =>
  props.ticket.closed_at ? formatCompactDate(props.ticket.closed_at) : ''
);

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
    <!--
      Padding split: SectionCard's content area uses `px-1 py-3`
      instead of the default `p-3`, then the property-list
      container adds `px-2` back. Net horizontal inset to plain
      labels (Title / Status / Resolution / etc.) is 4 + 8 = 12px,
      identical to the original `p-3`. The 8px in the property
      list's `px-2` is the breathing area that interactive
      headers (PropertyChipRow / TicketTagsField buttons) extend
      into via their own `-mx-2 px-2`. The math cancels at every
      level so every label's text aligns at exactly the same x,
      and button hover backgrounds get visible padding without
      shifting the visible label text away from where plain
      labels sit.
    -->
    <SectionCard class="print:hidden" content-padding="px-1 py-3">
      <template #title>Ticket Details</template>

      <template #default>
        <div class="flex flex-col gap-3 px-2">
          <!-- Title Section.
               The origin-channel hint that lived here as a header
               badge moved to a dedicated `Source` metadata row
               (below) so the title cell stays focused on the title
               and the channel reads as a proper field rather than
               a header decoration. -->
          <div class="flex flex-col gap-1.5">
            <h3 class="text-xs font-medium text-tertiary">Title</h3>
            <div class="bg-surface-alt rounded-lg border border-subtle hover:border-default transition-colors">
              <!-- 255 mirrors the backend's `tickets.title
                   VARCHAR(255) NOT NULL` cap. Enforcing it
                   client-side means the user sees the limit at
                   typing-time rather than getting a 500 on save. -->
              <ContentEditable
                :modelValue="ticket.title || ''"
                :max-length="255"
                @update:modelValue="handleTitleUpdate"
                @focus="emit('titleFocus')"
                @blur="emit('titleBlur')"
              />
            </div>
          </div>

          <!-- Source / channel row. Only rendered for tickets
               opened via an ingestion pipeline (email / chat /
               integration); UI- or API-created tickets stay
               unmarked. The icon + provider name signal that any
               reply gets relayed back to the originating thread,
               which is operationally meaningful for techs. -->
          <div
            v-if="sourceLabel"
            class="flex items-center justify-between gap-2 text-xs"
          >
            <span class="text-tertiary font-medium">Source</span>
            <span
              class="inline-flex items-center gap-1.5 text-secondary"
              :title="`Opened via ${ticket.submitted_via ?? 'channel'} — replies are relayed back through the thread`"
            >
              <Icon name="email" class="w-3.5 h-3.5" />
              {{ sourceLabel }}
            </span>
          </div>

          <!-- Assignment Section -->
          <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <!-- Requester -->
            <div class="flex flex-col gap-1.5">
              <div class="flex items-center justify-between">
                <h3 class="text-xs font-medium text-tertiary">Requester</h3>
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
                <h3 class="text-xs font-medium text-tertiary">Assignee</h3>
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
              <h3 class="text-xs font-medium text-tertiary">Status</h3>
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
              <h3 class="text-xs font-medium text-tertiary">Priority</h3>
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

          <!-- SLA pill — only when the ticket has a policy +
               calendar applied. Mirrors the list view's column so
               the operational urgency reads on the detail surface
               without a context switch. The countdown / breach
               state comes from `services::sla::compute_pill` on
               the backend; the visual mapping comes from
               `deriveSlaState` (shared with the list cell). -->
          <div
            v-if="slaState"
            class="flex items-center justify-between gap-2 text-xs"
            :title="slaState.detail"
          >
            <span class="text-tertiary font-medium">SLA</span>
            <span class="inline-flex items-center gap-1.5" :class="slaState.toneClass">
              <Icon name="clock" class="w-3.5 h-3.5" />
              <span class="font-medium">{{ slaState.statusLabel }}</span>
              <span
                v-if="!slaState.breached && !slaState.paused"
                class="text-tertiary tabular-nums"
              >· {{ slaState.compactLabel }}</span>
            </span>
          </div>

          <!-- Scheduling group: due date + recurrence collapsed
               by default. The SectionCard chrome matches every
               other card-with-header in the app so the form
               composition stays uniform; the headerActions slot
               carries the inline preview ("Due Jan 14 · Weekly")
               so the value reads without an open. -->
          <SectionCard content-padding="">
            <template #title>
              <button
                type="button"
                class="flex items-center gap-1.5 text-[13px] font-semibold text-primary"
                :aria-expanded="schedulingOpen"
                @click="schedulingOpen = !schedulingOpen"
              >
                <Icon
                  name="chevronDown"
                  class="w-3 h-3 text-tertiary transition-transform"
                  :class="{ '-rotate-90': !schedulingOpen }"
                />
                Scheduling
              </button>
            </template>
            <template #headerActions>
              <span class="text-[11px] text-tertiary truncate">
                {{ schedulingPreview || 'None' }}
              </span>
            </template>
            <div v-if="schedulingOpen" class="px-3 py-3 flex flex-col gap-3 border-t border-default">
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
          </SectionCard>

          <!-- Category Section -->
          <div v-if="categoryOptions && categoryOptions.length > 0" class="flex flex-col gap-1.5">
            <h3 class="text-xs font-medium text-tertiary">Category</h3>
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

          <!-- Cycle membership chip. Only rendered when the ticket
               belongs to one. Click navigates to the cycles surface
               (per-cycle detail route hasn't shipped; the click
               target stays today so the hook is in place when the
               route lands). -->
          <div
            v-if="ticket.cycle"
            class="flex items-center justify-between gap-2 text-xs"
          >
            <span class="text-tertiary font-medium">Cycle</span>
            <button
              type="button"
              class="inline-flex items-center gap-1.5 px-2 py-0.5 rounded text-[11px] font-medium bg-accent-muted text-accent hover:bg-accent/20 transition-colors"
              :title="`Cycle ${ticket.cycle.name} (${ticket.cycle.state})`"
              @click="openCycle"
            >
              {{ ticket.cycle.name }}
            </button>
          </div>

          <!-- Tags. Workspace-scoped multi-valued labels — the
               flexible second axis to the fixed Category. Always
               render the section so the "Add tag" affordance is
               discoverable; empty state shows just the trigger. -->
          <TicketTagsField
            :ticket-id="ticket.id"
            :tag-ids="ticket.tag_ids ?? []"
            @update:tag-ids="(v) => emit('update:tag-ids', v)"
          />

          <!-- Watchers. The "I want to be told about this without
               owning it" affordance. Bell toggle for the current
               user; avatar row for the broader set. Notifications
               fan out server-side on each new comment. -->
          <TicketWatchersField
            :watcher-uuids="ticket.watcher_uuids ?? []"
            @toggle="emit('toggle-watch')"
          />

          <!-- Attached collections rendered as property-list rows
               (chips + add). Folds Devices, Linked Tickets,
               Projects, and Documentation into the same idiom as
               Tags and Watchers above so the sidebar reads as one
               cohesive property list. Modals for picking new
               attachments live in the parent. -->
          <TicketDevicesField
            :devices="devices ?? []"
            @add="emit('add-device')"
            @remove="(id) => emit('remove-device', id)"
          />

          <div
            @dragenter.prevent
            @dragover.prevent
          >
            <TicketLinkedTicketsField
              :linked-ticket-ids="ticket.linkedTickets ?? []"
              :show-drop-affordance="!!showLinkDropAffordance"
              :is-drop-target="!!isLinkDropTarget"
              :drag-label="linkDropDragLabel"
              @add="emit('add-linked-ticket')"
              @remove="(id) => emit('remove-linked-ticket', id)"
            />
          </div>

          <TicketProjectsField
            :project-ids="normalisedProjectIds"
            @add="emit('add-project')"
            @remove="(id) => emit('remove-project', id)"
          />

          <TicketLinkedDocs
            :ticket-id="ticket.id"
            @add="emit('save-as-doc')"
          />

          <!-- Resolution notes. Free-text "what fixed this?"
               capture, distinct from the comment thread because
               the resolution is a structured fact rather than
               a discussion. Always render the section so techs
               can pre-fill notes mid-investigation; visual
               treatment elevates when the ticket has landed in a
               terminal workflow state (done / cancelled) so the
               closure surface reads as a finished record. -->
          <div class="flex flex-col gap-1.5">
            <!-- Heading row uses the same `-mx-2 px-2` outer
                 extent as PropertyChipRow / TicketTagsField
                 buttons so all property headings share one box
                 geometry — guaranteed alignment with the
                 button-style heading rows. -->
            <div class="flex items-center justify-between gap-2 -mx-2 px-2">
              <h3
                class="text-xs font-medium"
                :class="isTerminalState ? 'text-primary' : 'text-tertiary'"
              >Resolution</h3>
              <span
                v-if="isTerminalState"
                class="text-[10px] font-semibold text-status-closed"
              >Closed</span>
            </div>
            <textarea
              v-model="localResolutionNotes"
              :placeholder="isTerminalState
                ? 'Capture what fixed this — the answer the next person will need.'
                : 'Notes on the fix can be drafted here while you work the ticket.'"
              rows="3"
              maxlength="4000"
              class="w-full bg-surface-alt rounded-lg border text-sm text-primary px-2.5 py-2 outline-none transition-colors resize-y min-h-[3.5rem] focus:border-accent"
              :class="isTerminalState
                ? 'border-default'
                : 'border-subtle hover:border-default'"
              @blur="handleResolutionBlur"
            />
          </div>

          <!-- Audit / timestamps. Pairs each timestamp with the
               actor who effected it (UserCell renders avatar +
               name from the sync engine pool). Closed row is
               conditional — only present once the ticket has
               actually been closed. The audit section grew from a
               single Created/Modified row pair so consumers can
               answer "who did this last and when" without opening
               the activity timeline. -->
          <!-- Audit block uses the same `-mx-2 px-2` outer extent
               so its labels (Created / Last Modified / Closed) sit
               at the same x as the rest of the property list,
               and the border-t spans the full visual row width. -->
          <div class="pt-2 border-t border-default flex flex-col gap-2 -mx-2 px-2">
            <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
              <div class="flex flex-col gap-1">
                <span class="text-xs text-tertiary font-medium">Created</span>
                <span class="text-secondary text-sm font-medium">{{ createdDate }}</span>
                <UserCell
                  v-if="ticket.created_by"
                  :uuid="ticket.created_by"
                  size="xxs"
                />
              </div>

              <div class="flex flex-col gap-1">
                <span class="text-xs text-tertiary font-medium">Last Modified</span>
                <span class="text-secondary text-sm font-medium">{{ modifiedDate }}</span>
              </div>
            </div>

            <div
              v-if="ticket.closed_at"
              class="flex flex-col gap-1"
            >
              <span class="text-xs text-tertiary font-medium">Closed</span>
              <span class="text-secondary text-sm font-medium">{{ closedDateLabel }}</span>
              <UserCell
                v-if="ticket.closed_by"
                :uuid="ticket.closed_by"
                size="xxs"
              />
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