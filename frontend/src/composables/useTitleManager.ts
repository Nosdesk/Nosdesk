import { computed, ref, shallowRef, toValue, watch, type ComputedRef } from 'vue';
import { useRoute } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useRecentTicketsStore } from '@/stores/recentTickets';
import { useBrandingStore } from '@/stores/branding';
import { previewTicketField } from '@nosdesk/core/services/ticketService';
import { ticketNumber } from '@/utils/ticketNumbers';

import {
  activeClaim,
  type ClaimRecord,
  type TitleableDevice,
  type TitleableDocument,
  type TitleableTicket,
} from './usePageTitle';

export type { TitleableDevice, TitleableDocument, TitleableTicket } from './usePageTitle';

// The header and tab read whatever claim usePageTitle resolves to. Views
// write through usePageTitle only; this module never holds a title of its
// own, apart from a rename in progress in the site header.
const currentTicket = computed<TitleableTicket | null>(
  () => toValue(activeClaim.value?.claim.ticket) ?? null,
);
const currentDevice = computed<TitleableDevice | null>(
  () => toValue(activeClaim.value?.claim.device) ?? null,
);
const currentDocument = computed<TitleableDocument | null>(
  () => toValue(activeClaim.value?.claim.document) ?? null,
);
const customTitle = computed(() => toValue(activeClaim.value?.claim.title) ?? null);
const isTransitioning = ref(false);

// A rename typed into the header, shown until the owning view's own state
// catches up (or the claim changes hands).
const titleDraft = shallowRef<{ claim: ClaimRecord; value: string } | null>(null);
const draftFor = (record: ClaimRecord | null) =>
  record && titleDraft.value?.claim === record ? titleDraft.value.value : null;

const titleEditable = (record: ClaimRecord | null) =>
  Boolean(record?.claim.onTitleSave) && (toValue(record?.claim.titleEditable) ?? true);

const isTicketView = computed(() => currentTicket.value !== null);
const isDeviceView = computed(() => currentDevice.value !== null);
const isDocumentView = computed(() => currentDocument.value !== null);
const isCustomTitleEditable = computed(
  () =>
    customTitle.value !== null &&
    !isTicketView.value &&
    !isDeviceView.value &&
    !isDocumentView.value &&
    titleEditable(activeClaim.value),
);
const isDeviceTitleEditable = computed(
  () => currentDevice.value !== null && titleEditable(activeClaim.value),
);

// Module-level watcher for recent tickets store (no route dependency)
watch(
  () => currentTicket.value?.title,
  (newTitle) => {
    if (currentTicket.value && newTitle !== undefined) {
      const recentTicketsStore = useRecentTicketsStore();
      recentTicketsStore.updateTicketData(currentTicket.value.id, {
        title: newTitle
      });
    }
  }
);

// Guard: route-dependent watchers and computed are created only once
let pageTitle: ComputedRef<string> | null = null;

export function useTitleManager() {
  const route = useRoute();
  const brandingStore = useBrandingStore();
  const fluent = useFluent();
  const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args);
  const getAppName = () => brandingStore.appName;

  // Create route-dependent computed + watchers only on first call
  if (!pageTitle) {
    // The subject's own title, or null when the route's title applies.
    const claimedTitle = computed<string | null>(() => {
      const draft = draftFor(activeClaim.value);
      if (draft !== null) return draft;
      if (currentDocument.value) return currentDocument.value.title || customTitle.value;
      if (customTitle.value) return customTitle.value;
      if (currentTicket.value) {
        return `#${ticketNumber(currentTicket.value)} ${currentTicket.value.title}`;
      }
      if (currentDevice.value) {
        const dev = currentDevice.value;
        const label = dev.name || (dev.attributes?.hostname as string | undefined) || '';
        return `#${dev.id} ${label}`.trim();
      }
      return null;
    });

    const routeTitle = computed<string | null>(() => {
      // Prefer `titleKey` (translatable); routes that set a literal `title`
      // still fall through.
      const titleKey = route.meta?.titleKey as string | undefined;
      const titleKeyArgs = route.meta?.titleKeyArgs as
        | Record<string, string | number>
        | undefined;
      if (titleKey) return t(titleKey, titleKeyArgs);
      return (route.meta?.title as string | undefined) || null;
    });

    pageTitle = computed(() => claimedTitle.value ?? routeTitle.value ?? getAppName());

    // The only writer of document.title in the app.
    watch(
      () => brandingStore.getPageTitle(claimedTitle.value ?? routeTitle.value ?? undefined),
      (title) => {
        document.title = title;
      },
      { immediate: true },
    );
  }

  // Header edits. Each persists through the active claim's handler, which
  // belongs to the view that owns the subject; the draft covers the moment
  // between the keystroke and that view's state catching up.
  const commitTitle = async (newTitle: string) => {
    const record = activeClaim.value;
    if (!record?.claim.onTitleSave) return;
    titleDraft.value = { claim: record, value: newTitle };
    try {
      await record.claim.onTitleSave(newTitle);
    } finally {
      if (titleDraft.value?.claim === record) titleDraft.value = null;
    }
  };

  const updateCustomTitle = commitTitle;

  const previewTicketTitle = (newTitle: string) => {
    if (!currentTicket.value) return;
    // Transient broadcast ONLY. Do not mutate currentTicket.title: it IS the
    // sync pool row, the canonical value the commit path diffs against (and
    // patchTicket's rollback baseline). Writing the in-progress draft here
    // is what silently broke title saves. The header shows the draft from
    // InlineEdit's own local state; this just tells other viewers. The
    // field-preview endpoint is a DB no-op (no write, no audit) and our own
    // SSE echo is dropped via the X-SSE-Client-Id source match.
    void previewTicketField(currentTicket.value.id, 'title', newTitle).catch(() => {});
  };

  const previewDocumentTitle = (newTitle: string) => {
    const record = activeClaim.value;
    if (record && currentDocument.value) titleDraft.value = { claim: record, value: newTitle };
  };

  // Persist only. The registered save handler (ticketDetail.updateTitle ->
  // patchTicket) is the SOLE writer of the canonical pool row, applying the
  // optimistic update + rollback. currentTicket IS that pool row, so
  // pre-setting its title here would defeat the handler's `r.title === title`
  // no-op guard (skipping the PATCH) and corrupt the rollback baseline.
  const updateTicketTitle = async (newTitle: string) => {
    if (!currentTicket.value) return;
    await activeClaim.value?.claim.onTitleSave?.(newTitle);
  };

  // Persist only, mirroring updateTicketTitle: the asset view's handler
  // (updateAsset) is the sole writer. The header shows the in-progress
  // draft from HeaderTitle's own local state.
  const updateDeviceTitle = async (newTitle: string) => {
    if (!currentDevice.value) return;
    await activeClaim.value?.claim.onTitleSave?.(newTitle);
  };

  const previewDeviceTitle = (_newTitle: string) => {
    // No-op: assets have no title-preview broadcast.
  };

  const updateDocumentTitle = async (newTitle: string) => {
    if (!currentDocument.value) return;
    await commitTitle(newTitle);
  };

  const updateDocumentIcon = async (newIcon: string) => {
    if (!currentDocument.value) return;
    await activeClaim.value?.claim.onIconSave?.(newIcon);
  };

  const startTransition = () => {
    isTransitioning.value = true;
  };

  const endTransition = () => {
    isTransitioning.value = false;
  };

  return {
    // State
    currentTicket,
    currentDevice,
    currentDocument,
    isTransitioning,

    // Computed
    pageTitle: pageTitle!,
    isTicketView,
    isDeviceView,
    isDocumentView,
    isCustomTitleEditable,
    isDeviceTitleEditable,

    // Header edits
    updateCustomTitle,
    previewTicketTitle,
    previewDocumentTitle,
    updateTicketTitle,
    updateDeviceTitle,
    previewDeviceTitle,
    updateDocumentTitle,
    updateDocumentIcon,
    startTransition,
    endTransition,
  };
}
