<script setup lang="ts">
import { computed, ref, watchEffect, onMounted } from 'vue';
import { useRouter } from 'vue-router';
import { shareableRouteUrl } from '@/utils/shareUrl';
import { useFluent } from 'fluent-vue';
import { stripHtml } from '@/composables/useSanitise';
import type { TicketPriority } from '@/constants/ticketOptions';
import { useWorkflowStatesStore } from '@/stores/workflowStates';
import {
  buildWorkflowDropdownOptions,
  isCategoryHeaderValue,
  coarseStatusBucket,
} from '@/types/workflow';
import QRCode from 'qrcode';
import UserPicker from "@/components/ticketComponents/UserPicker.vue";
import CustomDropdown from "@/components/ticketComponents/CustomDropdown.vue";
import BaseDropdown from "@/components/common/BaseDropdown.vue";
import FormTextarea from "@/components/common/FormTextarea.vue";
import Button from "@/components/common/Button.vue";
import SectionCard from "@/components/common/SectionCard.vue";
import Icon from "@/components/common/Icon.vue";
import UserAvatar from "@/components/UserAvatar.vue";
import TicketTagsField from "@/components/ticketComponents/TicketTagsField.vue";
import TicketWatchersField from "@/components/ticketComponents/TicketWatchersField.vue";
import TicketDevicesField from "@/components/ticketComponents/TicketAssetsField.vue";
import TicketAssetUsage from "@/components/ticketComponents/TicketAssetUsage.vue";
import TicketLinkedTicketsField from "@/components/ticketComponents/TicketLinkedTicketsField.vue";
import TicketProjectsField from "@/components/ticketComponents/TicketProjectsField.vue";
import TicketLinkedDocs from "@/components/ticketComponents/TicketLinkedDocs.vue";
import ProjectChip from "@/components/ticketComponents/ProjectChip.vue";
import LinkedTicketChip from "@/components/ticketComponents/LinkedTicketChip.vue";
import { useTicketDocs } from "@/composables/usePageTicketLinks";
import SlaExplainPopover from "@/components/sla/SlaExplainPopover.vue";
import DatePicker from "@/components/common/DatePicker.vue";
import { getDateConfig } from "@/utils/dateUtils";
import type { Asset } from "@/types/asset";
import type { CommentWithAttachments } from "@/types/comment";
import LogoIcon from "@/components/icons/LogoIcon.vue";
import { useBrandingStore } from "@/stores/branding";
import { useTagsStore } from "@/stores/tags";
import type { Tag } from "@/types/tag";
import { useAuthStore } from "@/stores/auth";
import { deriveSlaState, type SlaPayload } from "@/composables/useSlaState";
import { formatCompactDate, formatCompactRelativeTime, formatRelativeTime } from "@/utils/dateUtils";
import { useUsersDirectory } from "@/composables/useUsersDirectory";

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
    /** Cycle membership embedded by the detail handler. The chip is
     *  clickable and opens the cycle's detail board at /cycles/:uuid. */
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
  selectedPriority: string;
  selectedCategory?: number | null;
  selectedWorkflowStateId?: number | null;
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
onMounted(() => { void workflowStatesStore.load() });

/**
 * Workflow state options for the status dropdown, grouped by category.
 * Categories are emitted as non-selectable header rows (`disabled: true`)
 * so the picker shows the structure without letting the user pick the
 * category itself. Empty categories are skipped. Returns an empty list
 * until the store has loaded (the onMounted load above populates it).
 */
const workflowDropdownOptions = computed(() =>
  buildWorkflowDropdownOptions(
    workflowStatesStore.byCategory,
    workflowStatesStore.loaded,
    workflowStatesStore.states.length,
  ),
);

const workflowDropdownValue = computed(() =>
  props.selectedWorkflowStateId != null ? String(props.selectedWorkflowStateId) : '',
);

function handleStatusDropdownChange(v: string) {
  if (isCategoryHeaderValue(v)) return; // header row; ignore
  const id = Number(v);
  if (Number.isFinite(id)) emit('update:selectedWorkflowStateId', id);
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
  const st = props.selectedWorkflowStateId != null
    ? workflowStatesStore.findById(props.selectedWorkflowStateId)
    : undefined;
  return st?.name || t('ticket-detail-print-unknown');
});

// Coarse 3-bucket key for the print badge colour class. The print
// CSS defines print-badge-open / -in-progress / -closed; map the
// state's category onto that bucket (fallback 'backlog' -> open).
const printStatusBucket = computed(() => {
  const cat = props.selectedWorkflowStateId != null
    ? workflowStatesStore.findById(props.selectedWorkflowStateId)?.category
    : undefined;
  return coarseStatusBucket(cat ?? 'backlog');
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

/** The picker speaks `YYYY-MM-DD`; due_date is conceptually a
 * floating calendar day, not an instant. The backend column is
 * TIMESTAMPTZ but the model type is `NaiveDateTime` (the app's
 * store-UTC-as-naive convention), so it serialises without a tz and
 * its deserialiser rejects a trailing `Z` ("trailing input"). We
 * therefore write a naive midnight datetime (`<day>T00:00:00`, no
 * tz suffix) and read it back by slicing the first 10 chars — which
 * round-trips the exact picked day with no timezone-driven
 * off-by-one. Empty string from the picker clears the due date. */
const dueDateValue = computed<string>({
  get: () => (props.ticket.due_date ? props.ticket.due_date.slice(0, 10) : ''),
  set: (value: string) => {
    if (!value) {
      emit('update:dueDate', null);
      return;
    }
    emit('update:dueDate', `${value}T00:00:00`);
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
    const { defaultLocale, defaultTimezone } = getDateConfig();
    const formatted = new Date(props.ticket.due_date).toLocaleDateString(defaultLocale, {
      month: 'short',
      day: 'numeric',
      timeZone: defaultTimezone,
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

// Extra context appended to the SLA pill in non-active states.
// `compactLabel` already carries the live countdown for on-track /
// at-risk tickets; here we surface the analogue for breached
// ("Nd ago" against the missed target) and paused ("target Tue
// 12:00 PM" so the user knows what the timer will resume against).
// Active states return null and the existing countdown span
// renders unchanged.
const slaPillDetail = computed<string | null>(() => {
  const sla = props.ticket.sla;
  const state = slaState.value;
  if (!sla || !state) return null;
  if (state.breached) {
    const elapsed = formatCompactRelativeTime(sla.target_at);
    return elapsed ? `${elapsed} ago` : null;
  }
  if (state.paused) {
    return t('ticket-detail-sla-paused-target', { target: state.target });
  }
  return null;
});

// ---- Cycle pill -------------------------------------------------

const router = useRouter();

function openCycle() {
  // Open the cycle's shareable detail board (the scrum view scoped to
  // that cycle).
  const c = props.ticket.cycle;
  if (c) {
    void router.push(`/cycles/${c.uuid}`);
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
// Footer renders inline bylines ("Created by Alex · 4 May") instead
// of the earlier stacked grid. Resolves actor names through the
// shared directory composable so the byline reads as a sentence;
// falls back to a plain verb when the actor uuid is missing or
// hasn't resolved yet (UserAvatar handles its own missing-data
// state, so the avatar slot stays quiet while the line still
// reads). `formatCompactDate` returns a short relative-or-date
// string the list view's date cells share.
const { getUserHandle } = useUsersDirectory();

const createdByName = computed<string | null>(() => {
  const uuid = props.ticket.created_by;
  if (!uuid) return null;
  return getUserHandle(uuid).user.value?.name ?? null;
});

const closedByName = computed<string | null>(() => {
  const uuid = props.ticket.closed_by;
  if (!uuid) return null;
  return getUserHandle(uuid).user.value?.name ?? null;
});

const closedDateLabel = computed<string>(() =>
  props.ticket.closed_at ? formatCompactDate(props.ticket.closed_at) : ''
);

// Audit footer uses relative time as the visible label ("4 weeks
// ago") and tucks the absolute timestamp into a `title` tooltip.
// Linear / GitHub / Plain converge on this pattern: relative reads
// instantly during triage, absolute precision available on hover
// for forensic reads. Fall back to the parent's pre-formatted
// absolute string when the raw ISO isn't available (e.g. older
// payload shapes). The parent's `createdDate`/`modifiedDate`
// strings still drive the print layout (which wants the absolute
// form on paper).
const createdRelative = computed<string>(() =>
  props.ticket.created ? formatRelativeTime(props.ticket.created) : props.createdDate
);
const modifiedRelative = computed<string>(() =>
  props.ticket.modified ? formatRelativeTime(props.ticket.modified) : props.modifiedDate
);
const closedRelative = computed<string>(() =>
  props.ticket.closed_at ? formatRelativeTime(props.ticket.closed_at) : ''
);

// ---- Print-only derived fields ---------------------------------
//
// The print card is a single, complete snapshot, so it surfaces
// fields the screen sidebar spreads across collapsible groups: due
// date, SLA, cycle, source, tags, watchers and the resolution. These
// only render on paper (the screen layout keeps its richer controls).
const tagsStore = useTagsStore();

// Absolute due date — paper wants the full date, not the sidebar's
// compact "due in 2d" relative form.
const dueDateLabel = computed<string>(() => {
  if (!props.ticket.due_date) return '';
  const { defaultLocale, defaultTimezone } = getDateConfig();
  return new Date(props.ticket.due_date).toLocaleDateString(defaultLocale, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    timeZone: defaultTimezone,
  });
});

// "On track · 2h left" / "Breached · 3d ago" / "Paused · Target …".
// Reuses the sidebar pill's state + detail so paper matches screen.
const slaPrintLabel = computed<string | null>(() => {
  const s = slaState.value;
  if (!s) return null;
  const extra =
    slaPillDetail.value ??
    (s.compactLabel && s.compactLabel !== s.statusLabel ? s.compactLabel : null);
  return extra ? `${s.statusLabel} · ${extra}` : s.statusLabel;
});

const resolvedTags = computed<Tag[]>(() =>
  (props.ticket.tag_ids ?? [])
    .map((id) => tagsStore.findById(id))
    .filter((tag): tag is Tag => tag != null),
);

const resolutionText = computed<string>(() => (props.ticket.resolution_notes ?? '').trim());

// Watcher names when the set is small enough to read in a sidebar-
// width column; a bare count when it would overflow or hasn't
// finished resolving against the directory.
const watcherNames = computed<string[]>(() =>
  (props.ticket.watcher_uuids ?? [])
    .map((uuid) => getUserHandle(uuid).user.value?.name)
    .filter((name): name is string => !!name),
);
const watchersDisplay = computed<string | null>(() => {
  const count = props.ticket.watcher_uuids?.length ?? 0;
  if (count === 0) return null;
  const names = watcherNames.value;
  return names.length === count && count <= 6 ? names.join(', ') : String(count);
});

// Closed-by name appended after the closed date ("· Alex Kim").
const closedSuffix = computed<string>(() =>
  closedByName.value ? ` · ${closedByName.value}` : '',
);

// ---- Referenced content (print) --------------------------------
//
// The print sheet carries the ticket's relationships, which the
// interactive sidebar drops on paper (its property rows live inside
// a print:hidden card). Docs reuse the cached `useTicketDocs`
// composable (shares the sidebar's cache entry — no extra fetch);
// projects / linked tickets reuse the same chip components so the
// resolved names match the screen.
const { links: linkedDocLinks } = useTicketDocs(() => props.ticket.id);

const projectIds = computed<string[]>(() => {
  const list = props.ticket.projects;
  if (!list) return [];
  return list.map((p) => (typeof p === 'string' ? p : String(p.id)));
});

const linkedTicketIds = computed<number[]>(() => props.ticket.linkedTickets ?? []);

const printAssets = computed<Asset[]>(() => props.devices ?? []);

const hasReferences = computed<boolean>(
  () =>
    projectIds.value.length > 0 ||
    linkedTicketIds.value.length > 0 ||
    printAssets.value.length > 0 ||
    linkedDocLinks.value.length > 0,
);

// Generate QR code for ticket URL (for print). Workspace-scoped in path mode.
const ticketUrl = computed(() => {
  if (typeof window === 'undefined') return '';
  return shareableRouteUrl('ticket-view', { id: String(props.ticket.id) });
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

    <!-- Print-only compact layout. A single complete snapshot of the
         ticket: header + QR, a dense metadata grid that fills the page
         width, an optional tag row, and the resolution when present. -->
    <div class="hidden print:block print-ticket-details">
      <!-- QR pinned to the card's top-right corner. Absolute so it
           floats over a reserved grid cell (.print-qr-spacer) without
           inflating the header or pushing the content below it. -->
      <div v-if="qrCodeDataUrl" class="print-qr-code">
        <img :src="qrCodeDataUrl" :alt="t('ticket-detail-print-qr-alt')" />
        <span class="print-qr-label">{{ t('ticket-detail-print-qr-label') }}</span>
      </div>

      <!-- Header: ID + title. margin-right keeps the title clear of
           the QR corner. -->
      <div class="print-ticket-header">
        <span class="print-ticket-id">#{{ ticket.id }}</span>
        <h1 class="print-ticket-title">{{ ticket.title }}</h1>
      </div>

      <!-- Dense metadata grid: auto-fills the row, packing every field
           that carries a value so the sheet stays compact. The empty
           spacer reserves the top-right cell the QR floats over. -->
      <div class="print-meta-grid">
        <div class="print-qr-spacer" aria-hidden="true"></div>
        <div class="print-field">
          <span class="print-meta-label">{{ t('ticket-detail-print-status') }}</span>
          <span class="print-badge" :class="`print-badge-${printStatusBucket}`">{{ statusLabel }}</span>
        </div>
        <div class="print-field">
          <span class="print-meta-label">{{ t('ticket-detail-print-priority') }}</span>
          <span class="print-badge" :class="`print-badge-${selectedPriority}`">{{ priorityLabel }}</span>
        </div>
        <div v-if="categoryLabel" class="print-field">
          <span class="print-meta-label">{{ t('ticket-detail-print-category') }}</span>
          <span class="print-meta-value">{{ categoryLabel }}</span>
        </div>

        <div class="print-field">
          <span class="print-meta-label">{{ t('ticket-detail-print-requester') }}</span>
          <span v-if="ticket.requester_user" class="print-meta-value">{{ ticket.requester_user.name }}</span>
          <span v-else class="print-meta-empty">{{ t('ticket-detail-print-unassigned') }}</span>
        </div>
        <div class="print-field">
          <span class="print-meta-label">{{ t('ticket-detail-print-assignee') }}</span>
          <span v-if="ticket.assignee_user" class="print-meta-value">{{ ticket.assignee_user.name }}</span>
          <span v-else class="print-meta-empty">{{ t('ticket-detail-print-unassigned') }}</span>
        </div>

        <div v-if="slaPrintLabel" class="print-field">
          <span class="print-meta-label">{{ t('ticket-detail-print-sla') }}</span>
          <span class="print-meta-value">{{ slaPrintLabel }}</span>
        </div>
        <div v-if="ticket.cycle" class="print-field">
          <span class="print-meta-label">{{ t('ticket-detail-print-cycle') }}</span>
          <span class="print-meta-value">{{ ticket.cycle.name }}</span>
        </div>
        <div v-if="sourceLabel" class="print-field">
          <span class="print-meta-label">{{ t('ticket-detail-print-source') }}</span>
          <span class="print-meta-value">{{ sourceLabel }}</span>
        </div>

        <div class="print-field">
          <span class="print-meta-label">{{ t('ticket-detail-print-created') }}</span>
          <span class="print-meta-value">{{ createdDate }}</span>
        </div>
        <div class="print-field">
          <span class="print-meta-label">{{ t('ticket-detail-print-modified') }}</span>
          <span class="print-meta-value">{{ modifiedDate }}</span>
        </div>
        <div v-if="dueDateLabel" class="print-field">
          <span class="print-meta-label">{{ t('ticket-detail-print-due') }}</span>
          <span class="print-meta-value">{{ dueDateLabel }}</span>
        </div>
        <div v-if="closedDateLabel" class="print-field">
          <span class="print-meta-label">{{ t('ticket-detail-print-closed') }}</span>
          <span class="print-meta-value">{{ closedDateLabel }}{{ closedSuffix }}</span>
        </div>
        <div v-if="watchersDisplay" class="print-field">
          <span class="print-meta-label">{{ t('ticket-detail-print-watchers') }}</span>
          <span class="print-meta-value">{{ watchersDisplay }}</span>
        </div>
      </div>

      <!-- Tags: a wrapped row of bordered chips, full width. -->
      <div v-if="resolvedTags.length" class="print-tags-row">
        <span class="print-meta-label">{{ t('ticket-detail-print-tags') }}</span>
        <span v-for="tag in resolvedTags" :key="tag.id" class="print-tag">{{ tag.name }}</span>
      </div>

      <!-- Resolution: the headline fact on a closed ticket, so it gets
           its own full-width block rather than a metadata cell. -->
      <div v-if="resolutionText" class="print-resolution">
        <span class="print-meta-label">{{ t('ticket-detail-print-resolution') }}</span>
        <p class="print-resolution-body">{{ resolutionText }}</p>
      </div>

      <!-- Referenced content: projects, assets, linked tickets and
           documentation, in one compact labelled block. The interactive
           sidebar drops these on paper (its rows live in a print:hidden
           card); this carries the relationships onto the sheet. -->
      <div v-if="hasReferences" class="print-references">
        <div v-if="projectIds.length" class="print-ref-row">
          <span class="print-meta-label">{{ t('ticket-detail-print-projects') }}</span>
          <span class="print-ref-items">
            <ProjectChip v-for="id in projectIds" :key="`p-${id}`" :project-id="id" />
          </span>
        </div>
        <div v-if="printAssets.length" class="print-ref-row">
          <span class="print-meta-label">{{ t('ticket-detail-print-assets') }}</span>
          <span class="print-ref-items">
            <span v-for="device in printAssets" :key="`a-${device.id}`" class="print-ref-text">{{ device.name || t('ticket-detail-print-asset-fallback') }}<template v-if="device.serial_number"> &middot; {{ device.serial_number }}</template></span>
          </span>
        </div>
        <div v-if="linkedTicketIds.length" class="print-ref-row">
          <span class="print-meta-label">{{ t('ticket-detail-print-linked') }}</span>
          <span class="print-ref-items">
            <LinkedTicketChip v-for="id in linkedTicketIds" :key="`l-${id}`" :ticket-id="id" />
          </span>
        </div>
        <div v-if="linkedDocLinks.length" class="print-ref-row">
          <span class="print-meta-label">{{ t('ticket-detail-print-docs') }}</span>
          <span class="print-ref-items">
            <span v-for="link in linkedDocLinks" :key="`d-${link.page_id}`" class="print-ref-text">{{ link.page_title }}</span>
          </span>
        </div>
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
        <!--
          4 unlabelled clusters + Resolution + Audit footer.
          Convergent practice (Linear / Plain / Front / GitHub):
          at ~14 properties, gap-as-separator reads as structure
          without typographic noise of cluster headings. The
          single `border-t` is reserved for the audit footer,
          the only block with a categorically different register
          (system-asserted facts vs editable properties).

          Spacing scale (1:2:3 hierarchy):
            - Label-to-value in a row: gap-1 (4px) — label hugs value
            - Intra-cluster sibling rows: gap-2 (8px) — siblings breathe
            - Inter-cluster: gap-3 (12px) — silent grouping

          Heading band: every property heading sits in a 24px
          zone (min-h-6 inline-flex items-center on bare `<h3>`,
          PropertyChipRow's existing py-1 button = 24px). Without
          this normalisation, bare h3 (16px) sat next to button-
          style headings (24px) and the row cadence felt jagged.

          Padding split (unchanged): SectionCard uses px-1 py-3,
          this container adds px-2 back. Net horizontal inset to
          plain labels is 12px, matching the original p-3. The
          8px is the breathing area interactive headers extend
          into via their own -mx-2 px-2.
        -->
        <div class="flex flex-col gap-3 px-2">

          <!-- Cluster A — Identity. Who is this ticket about. -->
          <div class="flex flex-col gap-2">
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
            class="flex items-center justify-between gap-2 text-xs min-h-6"
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
            <div class="group/req flex flex-col gap-1">
              <div class="flex items-center justify-between">
                <h3 class="text-xs font-medium text-tertiary min-h-6 flex items-center">{{ t('ticket-detail-prop-requester') }}</h3>
                <!-- `+` was redundant (the UserPicker below is itself
                     a click target that opens the search); kept only
                     the `X` clear. Hidden at rest on hover-capable
                     pointers, revealed on group-hover; always visible
                     on coarse pointers (touch) since there's no hover
                     to reveal with. -->
                <button
                  v-if="selectedRequester"
                  @click="emit('update:requester', '')"
                  class="print:hidden p-1 text-tertiary hover:text-status-error hover:bg-status-error-muted rounded transition-colors opacity-0 group-hover/req:opacity-100 focus-visible:opacity-100 pointer-coarse:opacity-100"
                  type="button"
                  :title="t('ticket-detail-clear-requester')"
                >
                  <Icon name="close" />
                </button>
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
            <div class="group/ass flex flex-col gap-1">
              <div class="flex items-center justify-between">
                <h3 class="text-xs font-medium text-tertiary min-h-6 flex items-center">{{ t('ticket-detail-prop-assignee') }}</h3>
                <div class="print:hidden flex items-center gap-1">
                  <!-- One-click self-assign for staff. Only surfaced
                       on unassigned tickets, taking an unassigned
                       ticket is the daily-driver case, while reassign-
                       from-someone-else is a deliberate action that
                       should go through the picker. Stays always-
                       visible (not hover-revealed) because Claim is a
                       primary affordance, not a power-user shortcut. -->
                  <button
                    v-if="canSelfAssign && !selectedAssignee"
                    @click="toggleSelfAssign"
                    type="button"
                    class="text-[11px] font-medium px-2 h-6 rounded text-accent hover:bg-accent-muted transition-colors"
                    :title="t('ticket-detail-claim-title')"
                  >
                    {{ t('ticket-detail-claim') }}
                  </button>
                  <!-- `+` was redundant; kept only the clear. Hover-
                       revealed on fine pointers, always shown on
                       touch. -->
                  <button
                    v-if="selectedAssignee"
                    @click="emit('update:assignee', '')"
                    class="p-1 text-tertiary hover:text-status-error hover:bg-status-error-muted rounded transition-colors opacity-0 group-hover/ass:opacity-100 focus-visible:opacity-100 pointer-coarse:opacity-100"
                    type="button"
                    :title="t('ticket-detail-clear-assignee')"
                  >
                    <Icon name="close" />
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
          </div><!-- /Cluster A -->

          <!-- Cluster B — Triage. Operational state used for triage. -->
          <div class="flex flex-col gap-2">

          <!-- Status and Priority Section -->
          <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <!-- Status. Flat treatment per sidebar convention: the
                 CustomDropdown trigger already carries its own
                 hover-tint + rounded corners + status chip; the
                 previous outer card was redundant chrome that read
                 as form-mode in what's really a property display.
                 Same call for Priority and Category below. -->
            <div class="flex flex-col gap-1">
              <h3 class="text-xs font-medium text-tertiary min-h-6 flex items-center">{{ t('ticket-detail-prop-status') }}</h3>
              <CustomDropdown
                :value="workflowDropdownValue"
                :options="workflowDropdownOptions"
                type="status"
                @update:value="handleStatusDropdownChange"
                class="w-full"
              />
            </div>

            <!-- Priority -->
            <div class="flex flex-col gap-1">
              <h3 class="text-xs font-medium text-tertiary min-h-6 flex items-center">{{ t('ticket-detail-prop-priority') }}</h3>
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
            class="flex items-center justify-between gap-2 text-xs min-h-6"
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
              <!-- Countdown for active timers; pause/breach detail
                   otherwise. Either way the suffix sits in tertiary
                   text after a bullet for the same visual weight. -->
              <span
                v-if="!slaState.breached && !slaState.paused"
                class="text-tertiary tabular-nums"
              >· {{ slaState.compactLabel }}</span>
              <span
                v-else-if="slaPillDetail"
                class="text-tertiary tabular-nums"
              >· {{ slaPillDetail }}</span>
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
                <h3 class="text-xs font-medium text-tertiary min-h-6 flex items-center">
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
              <div class="flex flex-col gap-1">
                <h3 class="text-xs font-medium text-tertiary min-h-6 flex items-center">
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
              <div class="flex flex-col gap-1">
                <h3 class="text-xs font-medium text-tertiary min-h-6 flex items-center">
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
          </div><!-- /Cluster B -->

          <!-- Cluster C — Classification. How the ticket is bucketed. -->
          <div class="flex flex-col gap-2">

          <!-- Category Section -->
          <div v-if="categoryOptions && categoryOptions.length > 0" class="flex flex-col gap-1">
            <h3 class="text-xs font-medium text-tertiary min-h-6 flex items-center">{{ t('ticket-detail-prop-category') }}</h3>
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
               belongs to one. Click opens that cycle's detail board. -->
          <div
            v-if="ticket.cycle"
            class="flex items-center justify-between gap-2 text-xs min-h-6"
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
          </div><!-- /Cluster C -->

          <!-- Cluster D — Relations. People + things linked to this ticket. -->
          <div class="flex flex-col gap-2">

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

          <!-- Drag/drop drop-target lives on TicketView's wrapper
               around the whole sidebar (handlers + drop event are
               on the parent), so this row no longer needs its own
               `@dragenter.prevent @dragover.prevent` wrapper. That
               wrapper was leftover from before the sidebar-wide
               drop zone landed; removing it makes Linked Tickets a
               uniform sibling of the other Relations rows so the
               cluster's `gap-2` applies cleanly with no inert div
               in the flex flow. -->
          <TicketLinkedTicketsField
            :linked-ticket-ids="ticket.linkedTickets ?? []"
            :show-drop-affordance="!!showLinkDropAffordance"
            :is-drop-target="!!isLinkDropTarget"
            :drag-label="linkDropDragLabel"
            @add="emit('add-linked-ticket')"
            @remove="(id) => emit('remove-linked-ticket', id)"
          />

          <TicketProjectsField
            :project-ids="normalisedProjectIds"
            @add="emit('add-project')"
            @remove="(id) => emit('remove-project', id)"
          />

          <TicketLinkedDocs
            :ticket-id="ticket.id"
            @add="emit('save-as-doc')"
          />
          </div><!-- /Cluster D -->

          <!-- Resolution notes. Free-text "what fixed this?"
               capture, distinct from the comment thread because
               the resolution is a structured fact rather than
               a discussion. Always render the section so techs
               can pre-fill notes mid-investigation; visual
               treatment elevates when the ticket has landed in a
               terminal workflow state (done / cancelled) so the
               closure surface reads as a finished record. -->
          <div class="flex flex-col gap-1">
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
            <!-- Auto-resize from 2 rows up to 12, so an empty
                 resolution is compact (the common case mid-
                 investigation) and grows as content lands. Opts
                 into the manual grip (`resize="vertical"`) because
                 Resolution is a long-form authoring surface where
                 a tech may want more workspace than the 12-row
                 cap; once dragged taller it stays that way via
                 the manualMinHeight floor in FormTextarea. -->
            <FormTextarea
              v-model="localResolutionNotes"
              :placeholder="t('ticket-detail-resolution-placeholder')"
              :rows="2"
              :max-rows="12"
              resize="vertical"
              maxlength="4000"
              @blur="handleResolutionBlur"
            />
          </div>

          <!-- Audit footer. Inline bylines under a single border-t,
               one line per fact, dimmer than property labels. Pattern
               matches Linear / GitHub / Plain: position + dim weight
               communicate "footer" without needing a heading. Each
               line falls back to a plain verb when the actor uuid
               is missing or hasn't resolved yet — UserAvatar handles
               its own missing-data state, so the avatar slot stays
               quiet but the line still reads. The outer container
               keeps `-mx-2 px-2` so the border-t spans the visual
               row width. `whitespace-nowrap` per line stops the
               timestamp from wrapping mid-time at narrow widths. -->
          <div class="pt-3 border-t border-default flex flex-col gap-1 -mx-2 px-2">
            <!-- Created row. Relative time scans instantly during
                 triage; the absolute timestamp lives in the line's
                 `title` for forensic reads on hover. -->
            <div
              class="flex items-center gap-1.5 text-[11px] text-tertiary whitespace-nowrap"
              :title="createdDate"
            >
              <UserAvatar
                v-if="ticket.created_by"
                :uuid="ticket.created_by"
                size="xxs"
                :show-name="false"
                :clickable="true"
              />
              <span class="truncate">{{ createdByName
                ? t('ticket-detail-audit-created-by', { name: createdByName })
                : t('ticket-detail-audit-created') }}</span>
              <span aria-hidden="true">·</span>
              <span class="tabular-nums">{{ createdRelative }}</span>
            </div>

            <!-- Updated row. No actor data available today
                 (would need a `modified_by` column to byline). -->
            <div
              class="flex items-center gap-1.5 text-[11px] text-tertiary whitespace-nowrap"
              :title="modifiedDate"
            >
              <span>{{ t('ticket-detail-audit-modified') }}</span>
              <span aria-hidden="true">·</span>
              <span class="tabular-nums">{{ modifiedRelative }}</span>
            </div>

            <!-- Closed row (only for terminal-state tickets). -->
            <div
              v-if="ticket.closed_at"
              class="flex items-center gap-1.5 text-[11px] text-tertiary whitespace-nowrap"
              :title="closedDateLabel"
            >
              <UserAvatar
                v-if="ticket.closed_by"
                :uuid="ticket.closed_by"
                size="xxs"
                :show-name="false"
                :clickable="true"
              />
              <span class="truncate">{{ closedByName
                ? t('ticket-detail-audit-closed-by', { name: closedByName })
                : t('ticket-detail-audit-closed') }}</span>
              <span aria-hidden="true">·</span>
              <span class="tabular-nums">{{ closedRelative }}</span>
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

  /* relative so the QR can float in the top-right corner. */
  .print-ticket-details {
    position: relative;
    border: 1px solid #ccc;
    padding: 10pt 12pt;
    margin-bottom: 10pt;
    background: #fafafa;
  }

  /* Compact header: ID + title on one baseline. margin-right keeps the
     title clear of the absolutely-positioned QR corner. */
  .print-ticket-header {
    display: flex;
    align-items: baseline;
    gap: 8pt;
    margin-right: 64pt;
    margin-bottom: 8pt;
    padding-bottom: 7pt;
    border-bottom: 1px solid #ddd;
  }

  .print-ticket-id {
    font-family: ui-monospace, monospace;
    font-size: 11pt;
    font-weight: 600;
    color: #666;
    white-space: nowrap;
  }

  .print-ticket-title {
    font-size: 14pt;
    font-weight: 600;
    color: #000;
    margin: 0;
    flex: 1;
  }

  /* Reserves the top-right grid cell the QR floats over so no field
     slides underneath it. -2 / -1 = the last (rightmost) column. */
  .print-qr-spacer {
    grid-column: -2 / -1;
    grid-row: 1 / span 2;
  }

  /* Auto-fill grid packs every populated field and fills the row
     width, so the sheet stays dense regardless of field count. */
  .print-meta-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(96pt, 1fr));
    gap: 7pt 14pt;
    align-items: start;
  }

  .print-field {
    display: flex;
    flex-direction: column;
    gap: 2pt;
    min-width: 0;
    break-inside: avoid;
  }

  .print-meta-label {
    font-size: 7.5pt;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.4pt;
    color: #666;
  }

  .print-meta-value {
    font-size: 10pt;
    color: #222;
    word-break: break-word;
  }

  .print-meta-empty {
    font-size: 10pt;
    color: #999;
    font-style: italic;
  }

  .print-badge {
    align-self: flex-start;
    display: inline-block;
    font-size: 9pt;
    font-weight: 500;
    padding: 1.5pt 5pt;
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

  /* Tags: wrapped chip row under the grid. */
  .print-tags-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 5pt;
    margin-top: 8pt;
    padding-top: 6pt;
    border-top: 1px solid #eee;
  }

  .print-tag {
    font-size: 8.5pt;
    color: #333;
    background: #fff;
    border: 1px solid #ccc;
    border-radius: 2pt;
    padding: 0.5pt 4pt;
    white-space: nowrap;
  }

  /* Resolution: full-width block, the headline fact on a closed ticket. */
  .print-resolution {
    display: flex;
    flex-direction: column;
    gap: 3pt;
    margin-top: 8pt;
    padding-top: 6pt;
    border-top: 1px solid #eee;
  }

  .print-resolution-body {
    margin: 0;
    font-size: 9.5pt;
    color: #222;
    line-height: 1.4;
    white-space: pre-wrap;
  }

  /* Referenced content: compact label + inline items, one row per
     relationship type. Items wrap; commas separate them. */
  .print-references {
    display: flex;
    flex-direction: column;
    gap: 4pt;
    margin-top: 8pt;
    padding-top: 6pt;
    border-top: 1px solid #eee;
  }

  .print-ref-row {
    display: flex;
    align-items: baseline;
    gap: 6pt;
  }

  .print-ref-row .print-meta-label {
    flex-shrink: 0;
    min-width: 52pt;
  }

  .print-ref-items {
    display: flex;
    flex-wrap: wrap;
    gap: 0 8pt;
    font-size: 9.5pt;
    color: #222;
  }

  .print-ref-text:not(:last-child)::after {
    content: ",";
    color: #888;
  }

  /* QR code: floats in the card's top-right corner over the reserved
     grid spacer, so it never pushes the metadata or content below. */
  .print-qr-code {
    position: absolute;
    top: 10pt;
    right: 12pt;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 1pt;
  }

  .print-qr-code img {
    width: 50pt !important;
    height: 50pt !important;
    max-width: 50pt !important;
    max-height: 50pt !important;
  }

  .print-qr-label {
    font-size: 6pt;
    color: #666;
    text-align: center;
  }
}
</style>