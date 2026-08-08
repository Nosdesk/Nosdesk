<script setup lang="ts">
import { computed, ref } from "vue";
import { RouterLink, useRoute } from "vue-router";
import { useBackNavigation } from '@/router/navigation';
import { useFluent } from 'fluent-vue';
import type { FluentVariable } from '@fluent/bundle';
import UserAvatar from "./UserAvatar.vue";
import UserDropdownMenu from "./UserDropdownMenu.vue";
import HeaderTitle from "./HeaderTitle.vue";
import DocumentIconSelector from "./DocumentIconSelector.vue";
import ItemIdentifier from "./ItemIdentifier.vue";
import CreateActionIcon, { type CreateIconType } from "./common/CreateActionIcon.vue";
import NotificationBell from "./NotificationBell.vue";
import Icon from "./common/Icon.vue";
import { useAuthStore } from '@/stores/auth';
import { useMobileDetection } from '@/composables/useMobileDetection';

const fluent = useFluent();
const t = (k: string, args?: Record<string, FluentVariable>) => fluent.$t(k, args);

// Detect mobile for responsive component sizing
const { isMobile } = useMobileDetection('sm')

// Leading back-arrow: the primary back affordance on mobile (the header is the
// nav bar there). Shown on non-root views only — a real in-app previous view, or
// a declared hierarchical parent so cold-start deep links still get a back
// affordance. On desktop the inline BackButton (in each view's toolbar) is used
// instead, so this stays mobile-only to avoid two back controls per screen.
const route = useRoute();
const { canGoBack, goBack } = useBackNavigation();
const showBackArrow = computed(
  () => isMobile.value && (canGoBack.value || !!route.meta.parent),
);

const authStore = useAuthStore();

interface Props {
  title?: string;
  titleIcon?: string;
  showCreateButton?: boolean;
  createButtonText?: string;
  createButtonIcon?: CreateIconType;
  useRouteTitle?: boolean;
  ticket: { id: number; title: string } | null;
  document: { id: string; title: string; icon: string } | null;
  device: { id: number; name: string; attributes?: Record<string, unknown> } | null;
  isTransitioning?: boolean;
  pageUrl?: string;
  navbarCollapsed?: boolean;
  /** When true, the (custom) title is rendered inline-editable, the same
   *  way a ticket title is. Used by project views to rename in-header. */
  customTitleEditable?: boolean;
  /** When true, the device (asset) name is rendered inline-editable in
   *  the header, the same way a ticket title is. */
  deviceTitleEditable?: boolean;
}

const props = withDefaults(defineProps<Props>(), {
  useRouteTitle: false,
  createButtonText: undefined,
  createButtonIcon: 'plus',
  ticket: null,
  document: null,
  device: null,
  isTransitioning: false,
  pageUrl: undefined,
  navbarCollapsed: false,
  customTitleEditable: false,
  deviceTitleEditable: false,
});

const emit = defineEmits(["updateDocumentTitle", "updateDocumentIcon", "previewDocumentTitle", "updateTicketTitle", "previewTicketTitle", "updateDeviceTitle", "previewDeviceTitle", "updateCustomTitle", "create"]);

const resolvedCreateButtonText = computed(() => props.createButtonText ?? t('header-create-ticket'));

const isTicketView = computed(() => {
  return props.ticket !== null;
});

const isDocumentView = computed(() => {
  return props.document !== null;
});

const isDeviceView = computed(() => {
  return props.device !== null;
});

// Only log in development mode
if (import.meta.env.DEV) {
  console.log("SiteHeader rendering with:", {
    isTicketView: isTicketView.value,
    isDocumentView: isDocumentView.value,
    ticket: props.ticket,
    document: props.document,
    title: props.title,
  });
}

// Use the provided title if available
const displayTitle = computed(() => {
  if (props.title) {
    return props.title;
  }
  return '';
});

// Responsive avatar size
const avatarSize = computed(() => isMobile.value ? 'lg' : 'md')

const handleUpdateDocumentTitle = (newTitle: string) => {
  if (props.document) {
    if (import.meta.env.DEV) {
      console.log(`SiteHeader: Updating document title to "${newTitle}" for document:`, props.document);
    }
    emit("updateDocumentTitle", newTitle);
  }
};

const handlePreviewDocumentTitle = (newTitle: string) => {
  if (props.document) {
    if (import.meta.env.DEV) {
      console.log(`SiteHeader: Previewing document title as "${newTitle}" for document:`, props.document);
    }
    emit("previewDocumentTitle", newTitle);
  }
};

const handleUpdateDocumentIcon = (newIcon: string) => {
  if (props.document) {
    emit("updateDocumentIcon", newIcon);
  }
};

const handleUpdateTicketTitle = (newTitle: string) => {
  if (props.ticket) {
    if (import.meta.env.DEV) {
      console.log(`SiteHeader: Updating ticket title to "${newTitle}" for ticket:`, props.ticket);
    }
    emit("updateTicketTitle", newTitle);
  }
};

const handlePreviewTicketTitle = (newTitle: string) => {
  if (props.ticket) {
    emit("previewTicketTitle", newTitle);
  }
};

const handleUpdateCustomTitle = (newTitle: string) => {
  emit("updateCustomTitle", newTitle);
};

const handleUpdateDeviceTitle = (newTitle: string) => {
  if (props.device) emit("updateDeviceTitle", newTitle);
};

const handlePreviewDeviceTitle = (newTitle: string) => {
  if (props.device) emit("previewDeviceTitle", newTitle);
};

const showUserMenu = ref(false);
const buttonRef = ref<HTMLElement | null>(null);

// Replace mock user data with actual user data from auth store
const user = computed(() => {
  if (authStore.user) {
    return {
      name: authStore.user.name,
      email: authStore.user.email,
      avatar: authStore.user.avatar_url // Use the avatar_url from the auth store
    };
  }
  return {
    name: "Guest",
    email: "guest@example.com",
    avatar: null
  };
});

const toggleUserMenu = () => {
  showUserMenu.value = !showUserMenu.value;
};

const closeUserMenu = () => {
  showUserMenu.value = false;
};

const handleCreateClick = () => {
  emit('create');
};

// Avatar refresh is no longer manual: UserAvatar binds to the sync
// pool by uuid, so any `user.updated` SSE frame (which fires when
// the profile screen uploads a new avatar) reactively repaints
// every mounted instance without a side-channel ping.
</script>

<template>
  <header class="bg-surface border-b border-default relative z-header">
    <!-- min-height instead of fixed height so long titles wrap to
         two lines (line-clamp-2 in the title elements below) and
         grow the header up to ~80px rather than getting lost to a
         single-line ellipsis. py-2 keeps the title from touching
         the top/bottom edge once wrapped. -->
    <div class="flex items-center justify-between min-h-14 sm:min-h-16 px-3 sm:px-4 md:px-6 py-2 gap-2">
      <!-- Mobile leading back-arrow (non-root views). goBack() pops the in-app
           stack when possible, else navigates to the hierarchical parent. -->
      <button
        v-if="showBackArrow"
        type="button"
        aria-label="Go back"
        class="flex-shrink-0 -ml-1 p-1.5 min-h-[44px] min-w-[44px] sm:min-h-0 sm:min-w-0 inline-flex items-center justify-center text-secondary hover:text-primary rounded"
        @click="goBack()"
      >
        <Icon name="chevronLeft" size="md" />
      </button>
      <!-- Left side - Title area -->
      <div class="flex items-center flex-1 min-w-0">
        <template v-if="isTicketView && props.ticket">
          <div class="flex items-center gap-2 min-w-0 flex-1">
            <ItemIdentifier :id="props.ticket.id" size="md" class="flex-shrink-0" />
            <!-- Editable ticket title in header: wraps to a second
                 line when long instead of getting lost to ellipsis.
                 maxLines=2 caps the wrap so the header tops out
                 around ~80px rather than growing unboundedly. -->
            <HeaderTitle
              :initialTitle="props.ticket.title || t('ui-site-header-untitled-ticket')"
              :placeholder-text="t('ui-site-header-ticket-title-placeholder')"
              :max-lines="2"
              @update-title="handleUpdateTicketTitle"
              @update-title-preview="handlePreviewTicketTitle"
              class="min-w-0 flex-1"
            />
          </div>
        </template>
        <template v-else-if="isDeviceView && props.device">
          <div class="flex items-center gap-2 min-w-0 flex-1">
            <ItemIdentifier :id="props.device.id" size="md" class="flex-shrink-0" />
            <!-- Editable asset name in header, mirroring the ticket
                 title; falls back to read-only when no save handler is
                 registered (e.g. a sync-owned asset). -->
            <HeaderTitle
              v-if="props.deviceTitleEditable"
              :initialTitle="props.device.name || t('ui-site-header-untitled-asset')"
              :placeholder-text="t('ui-site-header-asset-title-placeholder')"
              :max-lines="2"
              @update-title="handleUpdateDeviceTitle"
              @update-title-preview="handlePreviewDeviceTitle"
              class="min-w-0 flex-1"
            />
            <h1
              v-else
              class="text-xl font-semibold text-primary line-clamp-2 leading-tight break-words flex-1 min-w-0"
              :title="props.device.name || undefined"
            >
              {{ props.device.name || t('ui-site-header-unknown-device') }}
            </h1>
          </div>
        </template>
        <template v-else-if="isDocumentView && props.document">
          <div class="flex items-center gap-2 min-w-0 flex-1">
            <DocumentIconSelector
              :initial-icon="props.document.icon"
              @update:icon="handleUpdateDocumentIcon"
              class="flex-shrink-0"
            />
            <HeaderTitle
              :initialTitle="props.document.title"
              :placeholder-text="t('ui-site-header-document-title-placeholder')"
              :max-lines="2"
              @update-title="handleUpdateDocumentTitle"
              @update-title-preview="handlePreviewDocumentTitle"
              class="min-w-0 flex-1"
            />
          </div>
        </template>
        <template v-else-if="props.customTitleEditable">
          <!-- Inline-editable custom title (e.g. a project name). Same
               affordance as the ticket title; the parent persists via
               the updateCustomTitle handler. -->
          <HeaderTitle
            :initialTitle="displayTitle"
            :placeholder-text="t('ui-site-header-untitled')"
            :max-lines="2"
            @update-title="handleUpdateCustomTitle"
            class="min-w-0 flex-1"
          />
        </template>
        <template v-else>
          <div class="flex items-center gap-2 min-w-0">
            <!-- PDF icon -->
            <svg v-if="props.titleIcon === 'pdf'" class="w-6 h-6 text-status-error flex-shrink-0" viewBox="0 0 20 20" fill="currentColor">
              <path fill-rule="evenodd" d="M4 4a2 2 0 012-2h4.586A2 2 0 0112 2.586L15.414 6A2 2 0 0116 7.414V16a2 2 0 01-2 2H6a2 2 0 01-2-2V4zm2 6a1 1 0 011-1h6a1 1 0 110 2H7a1 1 0 01-1-1zm1 3a1 1 0 100 2h6a1 1 0 100-2H7z" clip-rule="evenodd" />
            </svg>
            <h1
              class="text-xl font-semibold text-primary line-clamp-2 leading-tight break-words"
              :title="displayTitle"
            >{{ displayTitle }}</h1>
          </div>
        </template>
      </div>

      <!-- Right side -->
      <div class="flex items-center gap-3 sm:gap-2 md:gap-4 flex-shrink-0">
        <!-- Create button: ghost-styled global affordance. The
             brand-orange treatment is reserved for the page's
             single primary action (e.g. "Add reply" on a ticket)
             so a navigation utility doesn't out-shout the page
             content. Hover lifts to a soft accent background. -->
        <!-- Label collapses to icon-only below md (768px) per
             industry convention (Jira, Asana, Stripe all use a
             single breakpoint label-hide). `title` mirrors the
             aria-label so the visible name comes back as a
             tooltip when the label is hidden, matching Primer's
             icon-button guidance. -->
        <button
          v-if="props.showCreateButton"
          @click="handleCreateClick"
          class="group flex create-button px-2.5 py-1.5 sm:px-3 min-h-[44px] sm:min-h-0 text-sm font-medium text-secondary border border-default rounded-lg hover:text-primary hover:border-accent hover:bg-accent-muted transition-colors items-center gap-2"
          :aria-label="t('ui-site-header-create-aria', { action: resolvedCreateButtonText })"
          :title="resolvedCreateButtonText"
        >
          <CreateActionIcon :icon="props.createButtonIcon" />
          <span class="create-button-text">{{ resolvedCreateButtonText }}</span>
        </button>

        <!-- Messaging cluster: Inbox link + Bell. Hidden below `sm:`
             because the mobile bottom nav already carries an Inbox
             primary slot (with an unread-count badge) and the top
             header isn't sticky, so a second affordance up here
             scrolls out of view while the bottom tile stays in
             thumb reach. On desktop the cluster pairs the two
             buttons by proximity rather than a shared border — they
             look related but behave differently (Inbox navigates,
             Bell opens a popover), so they share button chrome and
             a tight gap rather than a segmented control. -->
        <div class="hidden sm:flex items-center gap-0.5">
            <RouterLink
                to="/inbox"
                class="relative inline-flex rounded-lg p-2 text-secondary transition-colors hover:bg-surface-hover hover:text-primary focus:outline-none focus:ring-2 focus:ring-accent items-center justify-center"
                active-class="text-accent bg-accent/10"
                :aria-label="t('ui-site-header-inbox-aria')"
                :title="t('ui-site-header-inbox-tooltip')"
            >
                <Icon name="inbox" size="md" />
            </RouterLink>
            <NotificationBell />
        </div>

        <!-- User Profile Menu -->
        <div class="relative">
          <button
            ref="buttonRef"
            @click="toggleUserMenu"
            class="flex items-center justify-center min-h-[44px] min-w-[44px] sm:min-h-0 sm:min-w-0 hover:ring-2 hover:ring-accent rounded-full focus:outline-none focus:ring-2 focus:ring-accent"
            aria-haspopup="true"
            :aria-expanded="showUserMenu"
          >
            <UserAvatar
              :showName="false"
              :uuid="authStore.user?.uuid"
              :fallbackName="user.name"
              :fallbackAvatar="user.avatar"
              :size="avatarSize"
              :clickable="false"
            />
          </button>

          <!-- User Dropdown Menu -->
          <UserDropdownMenu
            :showMenu="showUserMenu"
            :buttonRef="buttonRef"
            @close="closeUserMenu"
          />
        </div>
      </div>
    </div>
  </header>
</template>

<style scoped>
.dropdown-menu {
  position: fixed;
  transform: translateZ(0);
  will-change: transform;
}

/* Collapse the create button label below md (767px) so the long
   locales ("Nouveau ticket" / "Nieuw ticket") never crowd the
   header. The icon, aria-label, and title tooltip preserve the
   action's identity at narrow widths. Matches the Jira / Asana /
   Stripe responsive convention for top-right primary CTAs. */
@media (max-width: 767px) {
  .create-button-text {
    display: none;
  }
}
</style>
