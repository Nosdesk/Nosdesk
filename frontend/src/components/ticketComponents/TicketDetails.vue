<script setup lang="ts">
import { computed, ref, watchEffect } from 'vue';
import { useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { stripHtml } from '@/composables/useSanitise';
import type { TicketStatus, TicketPriority } from '@/constants/ticketOptions';
import { useWorkflowStatesStore } from '@/stores/workflowStates';
import {
  getCategoryLabel,
  WORKFLOW_CATEGORIES,
  categoryHeaderValue,
  isCategoryHeaderValue,
} from '@/types/workflow';
import QRCode from 'qrcode';
import UserPicker from "@/components/ticketComponents/UserPicker.vue";
import CustomDropdown from "@/components/ticketComponents/CustomDropdown.vue";
import BaseDropdown from "@/components/common/BaseDropdown.vue";
import Button from "@/components/common/Button.vue";
import SectionCard from "@/components/common/SectionCard.vue";
import Icon from "@/components/common/Icon.vue";
import UserAvatar from "@/components/UserAvatar.vue";
import UserCell from "@/components/views/UserCell.vue";
import TicketTagsField from "@/components/ticketComponents/TicketTagsField.vue";
import TicketWatchersField from "@/components/ticketComponents/TicketWatchersField.vue";
import TicketDevicesField from "@/components/ticketComponents/TicketAssetsField.vue";
import TicketAssetUsage from "@/components/ticketComponents/TicketAssetUsage.vue";
import TicketLinkedTicketsField from "@/components/ticketComponents/TicketLinkedTicketsField.vue";
import TicketProjectsField from "@/components/ticketComponents/TicketProjectsField.vue";
import TicketLinkedDocs from "@/components/ticketComponents/TicketLinkedDocs.vue";
import SlaExplainPopover from "@/components/sla/SlaExplainPopover.vue";
import DatePicker from "@/components/common/DatePicker.vue";
import type { Asset } from "@/types/asset";
import type { CommentWithAttachments } from "@/types/comment";
import LogoIcon from "@/components/icons/LogoIcon.vue";
import { useBrandingStore } from "@/stores/branding";
import { useAuthStore } from "@/stores/auth";
import { deriveSlaState, type SlaPayload } from "@/composables/useSlaState";
import { formatCompactDate } from "@/utils/dateUtils";

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

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

// "Why this SLA?" popover. Click the pill to toggle; popover loads
// its data lazily the first time and caches per ticket id (see
// SlaExplainPopover for the cache rule).
const slaPillRef = ref<HTMLElement | null>(null);
const slaExplainOpen = ref(false);

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
  devices?: Asset[];
  /** Drag-to-link affordance state for the linked-tickets row. */
  showLinkDropAffordance?: boolean;
  isLinkDropTarget?: boolean;
  linkDropDragLabel?: string | null;
  /** Internal-note comments on this ticket. Drives the "Draft
   *  from internal notes" button on the Resolution section so a
   *  tech can promote their working notes into a fixed-record
   *  resolution without retyping. */
  internalComments?: CommentWithAttachments[];
}>();

const emit = defineEmits<{
  (e: "update:selectedStatus", value: TicketStatus): void;
  (e: "update:selectedWorkflowStateId", value: number): void;
  (e: "update:selectedPriority", value: TicketPriority): void;
  (e: "update:selectedCategory", value: string): void;
  (e: "update:requester", value: string): void;
  (e: "update:assignee", value: string): void;
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
  /** Fired after a usage row lands; the parent should refresh
   *  its copy of the asset because assets.quantity decremented
   *  in the same transaction. */
  (e: "asset-usage-recorded", assetId: number): void;
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
    out.push({ value: categoryHeaderValue(cat), label: getCategoryLabel(cat), disabled: true });
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

// Print-friendly display values
const statusLabel = computed(() => {
  const option = props.statusOptions.find(o => o.value === props.selectedStatus);
  return option?.label || props.selectedStatus || t('ticket-detail-print-unknown');
});

const priorityLabel = computed(() => {
  const option = props.priorityOptions.find(o => o.value === props.selectedPriority);
  return option?.label || props.selectedPriority || t('ticket-detail-print-unknown');
});

const categoryLabel = computed(() => {
  if (!props.selectedCategory) return null;
  const option = props.categoryOptions?.find(o => o.value === String(props.selectedCategory));
  return option?.label || props.ticket.category?.name || null;
});

/** Backend stores due_date as a TIMESTAMPTZ; the picker speaks
 * `YYYY-MM-DD`. Slicing the ISO string on read is sufficient
 * because the calendar view buckets cards by local-day, so any
 * additional precision would be misleading. On write we anchor at
 * start-of-day in the user's local timezone before serialising to
 * RFC3339 — the backend persists the tz so round-tripping is
 * unambiguous. Empty string from the picker clears the due date. */
const dueDateValue = computed<string>({
  get: () => (props.ticket.due_date ? props.ticket.due_date.slice(0, 10) : ''),
  set: (value: string) => {
    if (!value) {
      emit('update:dueDate', null);
      return;
    }
    const local = new Date(`${value}T00:00:00`);
    emit('update:dueDate', local.toISOString());
  },
});

/** Recurrence preset that maps to a known RRULE string. The picker
 * exposes a small list rather than the full RFC; an admin who
 * needs WEEKDAYS-only or interval=2 rules can edit the raw string
 * directly through the API. */
const RECURRENCE_PRESETS = computed<{ value: string; label: string }[]>(() => [
  { value: '', label: t('ticket-detail-recurrence-none') },
  { value: 'FREQ=DAILY', label: t('ticket-detail-recurrence-daily') },
  { value: 'FREQ=WEEKLY', label: t('ticket-detail-recurrence-weekly') },
  { value: 'FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR', label: t('ticket-detail-recurrence-weekdays') },
  { value: 'FREQ=MONTHLY', label: t('ticket-detail-recurrence-monthly') },
  { value: 'FREQ=YEARLY', label: t('ticket-detail-recurrence-yearly') },
]);

const RECURRENCE_LABELS = computed<Record<string, string>>(() => ({
  'FREQ=DAILY': t('ticket-detail-recurrence-daily'),
  'FREQ=WEEKLY': t('ticket-detail-recurrence-weekly'),
  'FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR': t('ticket-detail-recurrence-weekdays'),
  'FREQ=MONTHLY': t('ticket-detail-recurrence-monthly'),
  'FREQ=YEARLY': t('ticket-detail-recurrence-yearly'),
}));

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
    parts.push(t('ticket-detail-scheduling-due-prefix', { date: formatted }));
  }
  const rule = props.ticket.recurrence_rule;
  if (rule) {
    parts.push(RECURRENCE_LABELS.value[rule] ?? t('ticket-detail-recurrence-recurring'));
  }
  return parts.join(' · ');
});

const recurrenceSelectValue = computed<string>(() => {
  const rule = props.ticket.recurrence_rule ?? '';
  // Show 'custom' when the rule isn't one of our presets so the
  // dropdown stays honest about not being able to edit it here.
  if (!rule) return '';
  return RECURRENCE_PRESETS.value.some(p => p.value === rule) ? rule : '__custom__';
});

function handleRecurrenceChange(value: string): void {
  if (value === '__custom__') return; // no-op; custom rules are read-only in this picker
  emit('update:recurrenceRule', value || null);
}

// One hint string folded into the dropdown's `description` slot so
// the two old inline `<span>` notes (custom-rule readout / respawn
// note) don't need separate markup and inherit the dropdown's own
// description-text styling. Empty string -> no description rendered.
const recurrenceHint = computed<string | undefined>(() => {
  if (recurrenceSelectValue.value === '__custom__') {
    return t('ticket-detail-recurrence-custom-note', {
      rule: props.ticket.recurrence_rule ?? '',
    });
  }
  if (props.ticket.recurrence_rule) {
    return t('ticket-detail-recurrence-respawn-note');
  }
  return undefined;
});

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
  if (provider === 'email_imap') return t('ticket-detail-source-email');
  if (provider === 'email_smtp') return t('ticket-detail-source-email');
  if (provider === 'slack') return t('ticket-detail-source-slack');
  if (provider === 'teams') return t('ticket-detail-source-teams');
  // Fall through to the raw provider name for channels we
  // haven't pretty-named yet, better than masking the source.
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

/** Append the ticket's internal notes to the resolution textarea
 *  as a starting draft. Each note becomes its own paragraph; HTML
 *  is stripped via the shared `useSanitise` composable (DOMPurify-
 *  backed) so the resolution stays plain text. Saves immediately
 *  on insert so the draft survives accidental navigation; the user
 *  can still edit and re-save normally through the blur handler. */
function draftResolutionFromInternalNotes() {
  const notes = props.internalComments ?? [];
  if (notes.length === 0) return;
  const lines = notes
    .map((c) => stripHtml(c.content).trim())
    .filter((s) => s.length > 0);
  if (lines.length === 0) return;
  const appended = lines.join('\n\n');
  const current = localResolutionNotes.value.trim();
  localResolutionNotes.value = current
    ? `${current}\n\n${appended}`
    : appended;
  handleResolutionBlur();
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
      <img v-if="customLogoUrl" :src="customLogoUrl" :alt="t('ticket-detail-print-logo-alt')" class="print-logo-image" />
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
            <span class="print-meta-label">{{ t('ticket-detail-print-status') }}</span>
            <span class="print-badge" :class="`print-badge-${selectedStatus}`">{{ statusLabel }}</span>
          </div>
          <div class="print-meta-item">
            <span class="print-meta-label">{{ t('ticket-detail-print-priority') }}</span>
            <span class="print-badge" :class="`print-badge-${selectedPriority}`">{{ priorityLabel }}</span>
          </div>
          <div v-if="categoryLabel" class="print-meta-item">
            <span class="print-meta-label">{{ t('ticket-detail-print-category') }}</span>
            <span class="print-badge">{{ categoryLabel }}</span>
          </div>
        </div>

        <!-- People Row -->
        <div class="print-meta-row print-people-row">
          <div class="print-meta-item print-person">
            <span class="print-meta-label">{{ t('ticket-detail-print-requester') }}</span>
            <div v-if="ticket.requester_user" class="print-user">
              <UserAvatar
                :uuid="ticket.requester_user.uuid"
                :fallbackName="ticket.requester_user.name"
                :fallbackAvatar="ticket.requester_user.avatar_thumb || ticket.requester_user.avatar_url"
                size="sm"
                :showName="false"
                :clickable="false"
              />
              <span class="print-user-name">{{ ticket.requester_user.name }}</span>
            </div>
            <span v-else class="print-meta-empty">{{ t('ticket-detail-print-unassigned') }}</span>
          </div>
          <div class="print-meta-item print-person">
            <span class="print-meta-label">{{ t('ticket-detail-print-assignee') }}</span>
            <div v-if="ticket.assignee_user" class="print-user">
              <UserAvatar
                :uuid="ticket.assignee_user.uuid"
                :fallbackName="ticket.assignee_user.name"
                :fallbackAvatar="ticket.assignee_user.avatar_thumb || ticket.assignee_user.avatar_url"
                size="sm"
                :showName="false"
                :clickable="false"
              />
              <span class="print-user-name">{{ ticket.assignee_user.name }}</span>
            </div>
            <span v-else class="print-meta-empty">{{ t('ticket-detail-print-unassigned') }}</span>
          </div>
        </div>

        <!-- Dates Row -->
        <div class="print-meta-row print-dates-row">
          <div class="print-meta-item">
            <span class="print-meta-label">{{ t('ticket-detail-print-created') }}</span>
            <span class="print-meta-value">{{ createdDate }}</span>
          </div>
          <div class="print-meta-item">
            <span class="print-meta-label">{{ t('ticket-detail-print-modified') }}</span>
            <span class="print-meta-value">{{ modifiedDate }}</span>
          </div>
        </div>
      </div>

      <!-- QR Code (bottom right) -->
      <div v-if="qrCodeDataUrl" class="print-qr-code">
        <span class="print-qr-label">{{ t('ticket-detail-print-qr-label') }}</span>
        <img :src="qrCodeDataUrl" :alt="t('ticket-detail-print-qr-alt')" />
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
      <template #title>{{ t('ticket-detail-section-details') }}</template>

      <template #default>
        <div class="flex flex-col gap-3 px-2">
          <!-- Title removed from the sidebar (research finding,
               2026-05-31): every comparable product (Linear, Jira,
               GitHub, Front, Help Scout, Asana, Notion, ClickUp,
               Monday, Trello — 10/10) puts the title in the main
               content area as a click-to-edit heading and never
               duplicates it in the sidebar. The SiteHeader at the
               top of the page already provides editable-title via
               `titleManager.onTicketTitleSave`, so the sidebar
               duplicate was redundant and forced the only
               remaining "carded" field in an otherwise-flat panel.
               Trade-off: the sidebar field was the only path that
               wired through `titleAutoSave` (SSE typing preview to
               other viewers + 3s/8s debounced commits). SiteHeader
               commits on blur via direct PATCH, no preview. If
               typing-preview collaboration matters, future work is
               to rewire SiteHeader through `titleAutoSave`. -->

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
            <span class="text-tertiary font-medium">{{ t('ticket-detail-source-label') }}</span>
            <span
              class="inline-flex items-center gap-1.5 text-secondary"
              :title="t('ticket-detail-source-tooltip', { provider: ticket.submitted_via ?? 'channel' })"
            >
              <Icon name="email" class="w-3.5 h-3.5" />
              {{ sourceLabel }}
            </span>
          </div>

          <!-- Assignment Section. Container query rather than the
               viewport `sm:` breakpoint so the two-column pairing
               only kicks in when the sidebar itself is wide enough,
               not when the browser window happens to be wide. A
               narrow embedded sidebar (split layout, drawer, etc.)
               now stacks Requester above Assignee even on a 1440px
               viewport. Matches the @container pattern already used
               by TicketsTable. -->
          <div class="@container grid grid-cols-1 @sm:grid-cols-2 gap-3">
            <!-- Requester -->
            <div class="flex flex-col gap-1.5">
              <div class="flex items-center justify-between">
                <h3 class="text-xs font-medium text-tertiary">{{ t('ticket-detail-prop-requester') }}</h3>
                <div class="print:hidden flex items-center gap-0.5">
                  <button
                    v-if="selectedRequester"
                    @click="emit('update:requester', '')"
                    class="p-1 text-tertiary hover:text-status-error hover:bg-status-error-muted rounded transition-colors"
                    type="button"
                    :title="t('ticket-detail-clear-requester')"
                  >
                    <Icon name="close" />
                  </button>
                  <button
                    @click="requesterRef?.focus()"
                    class="p-1 text-tertiary hover:text-accent hover:bg-accent-muted rounded transition-colors"
                    type="button"
                    :title="t('ticket-detail-add-requester')"
                  >
                    <Icon name="add" />
                  </button>
                </div>
              </div>
              <!-- UserPicker's trigger has no built-in hover-tint
                   (unlike CustomDropdown). A light wrapper provides
                   the row-wide editability cue without the heavier
                   permanent border + background the previous version
                   carried, matching the flat-panel pattern adopted
                   for Status / Priority / Category / Scheduling. -->
              <div class="rounded-lg hover:bg-surface-hover transition-colors">
                <UserPicker
                  ref="requesterRef"
                  :modelValue="selectedRequester"
                  @update:modelValue="emit('update:requester', $event)"
                  :currentUser="ticket.requester_user"
                  :placeholder="t('ticket-detail-find-user-placeholder')"
                  type="requester"
                  :hideInlineClear="true"
                  class="w-full"
                />
              </div>
            </div>

            <!-- Assignee -->
            <div class="flex flex-col gap-1.5">
              <div class="flex items-center justify-between">
                <h3 class="text-xs font-medium text-tertiary">{{ t('ticket-detail-prop-assignee') }}</h3>
                <div class="print:hidden flex items-center gap-1">
                  <!-- One-click self-assign for staff. Only surfaced
                       on unassigned tickets, taking an unassigned
                       ticket is the daily-driver case, while reassign-
                       from-someone-else is a deliberate action that
                       should go through the picker. -->
                  <button
                    v-if="canSelfAssign && !selectedAssignee"
                    @click="toggleSelfAssign"
                    type="button"
                    class="text-[11px] font-medium px-2 h-6 rounded text-accent hover:bg-accent-muted transition-colors"
                    :title="t('ticket-detail-claim-title')"
                  >
                    {{ t('ticket-detail-claim') }}
                  </button>
                  <button
                    v-if="selectedAssignee"
                    @click="emit('update:assignee', '')"
                    class="p-1 text-tertiary hover:text-status-error hover:bg-status-error-muted rounded transition-colors"
                    type="button"
                    :title="t('ticket-detail-clear-assignee')"
                  >
                    <Icon name="close" />
                  </button>
                  <button
                    @click="assigneeRef?.focus()"
                    class="p-1 text-tertiary hover:text-accent hover:bg-accent-muted rounded transition-colors"
                    type="button"
                    :title="t('ticket-detail-add-assignee')"
                  >
                    <Icon name="add" />
                  </button>
                </div>
              </div>
              <div class="rounded-lg hover:bg-surface-hover transition-colors">
                <UserPicker
                  ref="assigneeRef"
                  :modelValue="selectedAssignee"
                  @update:modelValue="emit('update:assignee', $event)"
                  :currentUser="ticket.assignee_user"
                  :placeholder="t('ticket-detail-assign-to-placeholder')"
                  type="assignee"
                  :hideInlineClear="true"
                  class="w-full"
                />
              </div>
            </div>
          </div>

          <!-- Status and Priority Section -->
          <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <!-- Status. Flat treatment per sidebar convention: the
                 CustomDropdown trigger already carries its own
                 hover-tint + rounded corners + status chip; the
                 previous outer card was redundant chrome that read
                 as form-mode in what's really a property display.
                 Same call for Priority and Category below. -->
            <div class="flex flex-col gap-1.5">
              <h3 class="text-xs font-medium text-tertiary">{{ t('ticket-detail-prop-status') }}</h3>
              <CustomDropdown
                :value="workflowDropdownValue"
                :options="workflowDropdownOptions"
                type="status"
                @update:value="handleStatusDropdownChange"
                class="w-full"
              />
            </div>

            <!-- Priority -->
            <div class="flex flex-col gap-1.5">
              <h3 class="text-xs font-medium text-tertiary">{{ t('ticket-detail-prop-priority') }}</h3>
              <CustomDropdown
                :value="selectedPriority"
                :options="priorityOptions"
                type="priority"
                @update:value="(v: string) => emit('update:selectedPriority', v as TicketPriority)"
                class="w-full"
              />
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
            <span class="text-tertiary font-medium">{{ t('ticket-detail-sla-label') }}</span>
            <button
              ref="slaPillRef"
              type="button"
              class="inline-flex items-center gap-1.5 transition-colors duration-200 rounded focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
              :class="slaState.toneClass"
              :aria-expanded="slaExplainOpen"
              :aria-label="t('ticket-detail-sla-explain-aria')"
              @click="slaExplainOpen = !slaExplainOpen"
            >
              <Icon name="clock" class="w-3.5 h-3.5" />
              <span class="font-medium">{{ slaState.statusLabel }}</span>
              <span
                v-if="!slaState.breached && !slaState.paused"
                class="text-tertiary tabular-nums"
              >· {{ slaState.compactLabel }}</span>
            </button>
            <SlaExplainPopover
              :anchor="slaPillRef"
              :open="slaExplainOpen"
              :ticket-id="ticket.id"
              @close="slaExplainOpen = false"
            />
          </div>

          <!-- Scheduling group: due date + recurrence collapsed
               by default. The SectionCard chrome matches every
               other card-with-header in the app so the form
               composition stays uniform; the headerActions slot
               carries the inline preview ("Due Jan 14 · Weekly")
               so the value reads without an open. -->
          <!-- Scheduling. Flat collapsible matching the Status /
               Priority / Category sibling pattern; deliberately not
               wrapped in SectionCard, whose header `border-b` is
               correct for dashboard widgets but doubles up against
               a disclosure body (the "thicker border when collapsed"
               issue surfaced in design review).
               Design draws on the Linear / Front sidebar recipe:
                 - chevron-leading header strip with hover-tint
                   affordance (the only sidebar element with that
                   hover; signals interactivity vs. the flat sibling
                   labels);
                 - no borders anywhere — separation comes from the
                   parent's sibling-section gap and from indented
                   body content;
                 - preview pill only renders when collapsed (once
                   the body is open, the preview is redundant);
                 - body hugs the header (smaller top padding than
                   inter-field gap), so expanded reads as one unit. -->
          <div class="flex flex-col gap-1">
            <button
              type="button"
              class="flex items-center justify-between gap-2 -mx-2 px-2 py-1 rounded-md hover:bg-surface-hover transition-colors text-left"
              :aria-expanded="schedulingOpen"
              @click="schedulingOpen = !schedulingOpen"
            >
              <span class="flex items-center gap-1.5 min-w-0">
                <Icon
                  name="chevronDown"
                  class="w-3 h-3 text-tertiary transition-transform shrink-0"
                  :class="{ '-rotate-90': !schedulingOpen }"
                />
                <h3 class="text-xs font-medium text-tertiary">
                  {{ t('ticket-detail-scheduling-label') }}
                </h3>
              </span>
              <span
                v-if="!schedulingOpen"
                class="text-xs text-tertiary truncate"
              >
                {{ schedulingPreview || t('ticket-detail-scheduling-none') }}
              </span>
            </button>

            <div v-if="schedulingOpen" class="flex flex-col gap-3 pt-1 pl-5">
              <!-- Due date: picker + ghost clear button. The clear
                   only renders when a date is set so the row has no
                   trailing dead space in the empty case. -->
              <div class="flex flex-col gap-1.5">
                <h3 class="text-xs font-medium text-tertiary">
                  {{ t('ticket-detail-scheduling-due-date') }}
                </h3>
                <div class="flex items-center gap-2">
                  <DatePicker
                    v-model="dueDateValue"
                    size="sm"
                    block
                    :aria-label="t('ticket-detail-scheduling-due-date')"
                  />
                  <Button
                    v-if="ticket.due_date"
                    variant="ghost"
                    size="sm"
                    icon="close"
                    :aria-label="t('ticket-detail-scheduling-clear-due')"
                    @click="emit('update:dueDate', null)"
                  />
                </div>
              </div>

              <!-- Recurrence: dropdown + optional hint folded into
                   the dropdown's `description` slot so the spacing
                   matches the Due-date row exactly. -->
              <div class="flex flex-col gap-1.5">
                <h3 class="text-xs font-medium text-tertiary">
                  {{ t('ticket-detail-scheduling-recurrence') }}
                </h3>
                <BaseDropdown
                  :model-value="recurrenceSelectValue"
                  :options="RECURRENCE_PRESETS"
                  :description="recurrenceHint"
                  size="sm"
                  @update:model-value="(v) => handleRecurrenceChange(v as string)"
                />
              </div>
            </div>
          </div>

          <!-- Category Section -->
          <div v-if="categoryOptions && categoryOptions.length > 0" class="flex flex-col gap-1.5">
            <h3 class="text-xs font-medium text-tertiary">{{ t('ticket-detail-prop-category') }}</h3>
            <CustomDropdown
              :value="selectedCategory?.toString() || ''"
              :options="categoryOptions"
              type="category"
              @update:value="emit('update:selectedCategory', $event)"
              class="w-full"
              :placeholder="t('ticket-detail-category-placeholder')"
            />
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
            <span class="text-tertiary font-medium">{{ t('ticket-detail-cycle-label') }}</span>
            <button
              type="button"
              class="inline-flex items-center gap-1.5 px-2 py-0.5 rounded text-[11px] font-medium bg-accent-muted text-accent hover:bg-accent/20 transition-colors"
              :title="t('ticket-detail-cycle-tooltip', { name: ticket.cycle.name, state: ticket.cycle.state })"
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
            :ticket-id="ticket.id"
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

          <TicketAssetUsage
            v-if="ticket.id"
            :ticket-id="ticket.id"
            :assets="devices ?? []"
            @asset-updated="(id) => emit('asset-usage-recorded', id)"
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
              >{{ t('ticket-detail-resolution-label') }}</h3>
              <div class="flex items-center gap-2">
                <!-- Promote internal notes into the resolution draft.
                     Hidden when the ticket has no internal notes
                     yet, nothing to pull from, so the affordance
                     would just confuse. The notes append (don't
                     replace) so a half-written resolution survives
                     the pull. -->
                <button
                  v-if="(props.internalComments?.length ?? 0) > 0"
                  type="button"
                  class="inline-flex items-center gap-1 px-2 h-6 rounded text-[11px] font-medium text-status-warning hover:bg-status-warning-muted transition-colors"
                  :title="t('ticket-detail-resolution-draft-from-notes-title', { count: props.internalComments?.length ?? 0 })"
                  @click="draftResolutionFromInternalNotes"
                >
                  <svg class="w-3 h-3" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true">
                    <path d="M13 7H7v6h6V7z" />
                    <path fill-rule="evenodd" d="M7 2a1 1 0 012 0v1h2V2a1 1 0 112 0v1h2a2 2 0 012 2v2h1a1 1 0 110 2h-1v2h1a1 1 0 110 2h-1v2a2 2 0 01-2 2h-2v1a1 1 0 11-2 0v-1H9v1a1 1 0 11-2 0v-1H5a2 2 0 01-2-2v-2H2a1 1 0 110-2h1V9H2a1 1 0 010-2h1V5a2 2 0 012-2h2V2zM5 5h10v10H5V5z" clip-rule="evenodd" />
                  </svg>
                  <span>{{ t('ticket-detail-resolution-draft-from-notes') }}</span>
                </button>
                <span
                  v-if="isTerminalState"
                  class="text-[10px] font-semibold text-status-closed"
                >{{ t('ticket-detail-resolution-closed') }}</span>
              </div>
            </div>
            <textarea
              v-model="localResolutionNotes"
              :placeholder="isTerminalState
                ? t('ticket-detail-resolution-placeholder-active')
                : t('ticket-detail-resolution-placeholder-draft')"
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
                <span class="text-xs text-tertiary font-medium">{{ t('ticket-detail-audit-created') }}</span>
                <span class="text-secondary text-sm font-medium">{{ createdDate }}</span>
                <UserCell
                  v-if="ticket.created_by"
                  :uuid="ticket.created_by"
                  size="xxs"
                />
              </div>

              <div class="flex flex-col gap-1">
                <span class="text-xs text-tertiary font-medium">{{ t('ticket-detail-audit-modified') }}</span>
                <span class="text-secondary text-sm font-medium">{{ modifiedDate }}</span>
              </div>
            </div>

            <div
              v-if="ticket.closed_at"
              class="flex flex-col gap-1"
            >
              <span class="text-xs text-tertiary font-medium">{{ t('ticket-detail-audit-closed') }}</span>
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