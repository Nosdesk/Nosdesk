<script setup lang="ts">
import { useRouter, useRoute } from "vue-router";
import { computed, ref, onMounted, onUnmounted } from "vue";

// Debounce utility for resize events
function debounce<T extends (...args: any[]) => void>(fn: T, delay: number): T {
  let timeoutId: ReturnType<typeof setTimeout> | null = null
  return ((...args: Parameters<T>) => {
    if (timeoutId) clearTimeout(timeoutId)
    timeoutId = setTimeout(() => fn(...args), delay)
  }) as T
}
import UserAvatar from "./UserAvatar.vue";
import UserDropdownMenu from "./UserDropdownMenu.vue";
import HeaderTitle from "./HeaderTitle.vue";
import DocumentIconSelector from "./DocumentIconSelector.vue";
import ItemIdentifier from "./ItemIdentifier.vue";
import PageUrlDisplay from "./PageUrlDisplay.vue";
import CreateActionIcon, { type CreateIconType } from "./common/CreateActionIcon.vue";
import ticketService from '@/services/ticketService';
import { useAuthStore } from '@/stores/auth';
import { useMobileDetection } from '@/composables/useMobileDetection';

// Detect mobile for responsive component sizing
const { isMobile } = useMobileDetection('sm')

const router = useRouter();
const route = useRoute();
const authStore = useAuthStore();

interface Props {
  title?: string;
  showCreateButton?: boolean;
  createButtonText?: string;
  createButtonIcon?: CreateIconType;
  useRouteTitle?: boolean;
  ticket: { id: number; title: string } | null;
  document: { id: string; title: string; icon: string } | null;
  device: { id: number; hostname: string } | null;
  isTransitioning?: boolean;
  pageUrl?: string;
  navbarCollapsed?: boolean;
}

const props = withDefaults(defineProps<Props>(), {
  useRouteTitle: false,
  createButtonText: 'Create Ticket',
  createButtonIcon: 'plus',
  ticket: null,
  document: null,
  device: null,
  isTransitioning: false,
  pageUrl: undefined,
  navbarCollapsed: false,
});

const emit = defineEmits(["updateDocumentTitle", "updateDocumentIcon", "previewDocumentTitle", "updateTicketTitle", "create"]);

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

// Responsive sizes for header elements
const iconSize = computed(() => isMobile.value ? 'md' : 'sm')
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

// Add isCreating ref (generic for any create action)
const isCreating = ref(false);

const handleCreateClick = async () => {
  if (import.meta.env.DEV) {
    console.log('handleCreateClick called');
  }
  if (isCreating.value) {
    if (import.meta.env.DEV) {
      console.log('Already creating, returning');
    }
    return; // Prevent multiple clicks
  }

  // Emit event to parent - parent can handle the action
  emit('create');

  // Default behavior for tickets if no listener is provided (backward compatibility)
  // Only execute if we're on a tickets-related route
  if (route.path.includes('/tickets') || route.path === '/') {
    try {
      if (import.meta.env.DEV) {
        console.log('Setting isCreating to true');
      }
      isCreating.value = true;

      if (import.meta.env.DEV) {
        console.log('Creating empty ticket');
      }
      // Create an empty ticket
      const newTicket = await ticketService.createEmptyTicket();

      if (import.meta.env.DEV) {
        console.log('Empty ticket created:', newTicket);
      }
      // Navigate to the new ticket
      router.push(`/tickets/${newTicket.id}`);
    } catch (error) {
      console.error('Failed to create empty ticket:', error);

      if (import.meta.env.DEV) {
        console.log('Falling back to create ticket page');
      }
      // Fallback to the tickets page if creation fails
      router.push("/tickets");
    } finally {
      if (import.meta.env.DEV) {
        console.log('Setting isCreating to false');
      }
      isCreating.value = false;
    }
  }
};

// Event listeners for click outside
onMounted(() => {
  // Mobile detection is handled by useMobileDetection composable
});

onUnmounted(() => {
  // Cleanup handled by useMobileDetection composable
});

// Add a ref for the header avatar component
interface AvatarComponentType {
  refreshUser: (uuid?: string) => Promise<void>;
}

const headerAvatarRef = ref<AvatarComponentType | null>(null);

// Method to refresh avatar data - can be called from outside
const refreshAvatar = () => {
  if (headerAvatarRef.value && headerAvatarRef.value.refreshUser && authStore.user) {
    headerAvatarRef.value.refreshUser(authStore.user.uuid);
  }
};

// Expose the refresh method
defineExpose({
  refreshAvatar
});
</script>

<template>
  <header class="bg-surface border-b border-default relative z-[999]">
    <div class="flex items-center justify-between h-14 sm:h-16 px-3 sm:px-4 md:px-6 gap-2">
      <!-- Left side - Title area -->
      <div class="flex items-center flex-1 min-w-0">
        <template v-if="isTicketView && props.ticket">
          <div class="flex items-center gap-2 min-w-0 flex-1">
            <ItemIdentifier :id="props.ticket.id" size="md" class="flex-shrink-0" />
            <!-- Editable ticket title in header - truncated to fit -->
            <HeaderTitle
              :initialTitle="props.ticket.title || 'Untitled Ticket'"
              :placeholder-text="'Enter ticket title...'"
              :truncate="true"
              @update-title="handleUpdateTicketTitle"
              class="min-w-0 flex-1"
            />
          </div>
        </template>
        <template v-else-if="isDeviceView && props.device">
          <div class="flex items-center gap-2 min-w-0 flex-1">
            <ItemIdentifier :id="props.device.id" size="md" class="flex-shrink-0" />
            <!-- Display device hostname as read-only in header -->
            <h1 class="text-xl font-semibold text-primary truncate flex-1 min-w-0">
              {{ props.device.hostname || 'Unknown Device' }}
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
              :placeholder-text="'Enter document title...'"
              :truncate="true"
              @update-title="handleUpdateDocumentTitle"
              @update-title-preview="handlePreviewDocumentTitle"
              class="min-w-0 flex-1"
            />
          </div>
        </template>
        <template v-else>
          <h1 class="text-xl font-semibold text-primary truncate">{{ displayTitle }}</h1>
        </template>
      </div>

      <!-- Right side -->
      <div class="flex items-center gap-3 sm:gap-2 md:gap-4 flex-shrink-0">
        <!-- Create Button (generic - can be ticket, project, etc.) - icon only on mobile -->
        <button
          v-if="props.showCreateButton"
          @click="handleCreateClick"
          :disabled="isCreating"
          class="flex create-button px-2.5 py-2 sm:px-4 text-sm font-medium bg-accent text-white rounded-lg hover:bg-accent-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed items-center gap-2"
          :aria-label="isCreating ? `Creating...` : `Create ${props.createButtonText}`"
        >
          <CreateActionIcon :icon="props.createButtonIcon" :loading="isCreating" :size="iconSize" />
          <span class="create-button-text">{{ isCreating ? 'Creating...' : props.createButtonText }}</span>
        </button>

        <!-- User Profile Menu -->
        <div class="relative">
          <button
            ref="buttonRef"
            @click="toggleUserMenu"
            class="flex items-center justify-center hover:ring-2 hover:ring-accent rounded-full focus:outline-none focus:ring-2 focus:ring-accent"
            aria-haspopup="true"
            :aria-expanded="showUserMenu"
          >
            <UserAvatar
              :showName="false"
              :name="user.name"
              :avatar="user.avatar"
              :size="avatarSize"
              :clickable="false"
              ref="headerAvatarRef"
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

/* Compact the create button below 640px (sm breakpoint) - icon only */
@media (max-width: 639px) {
  .create-button-text {
    display: none;
  }
}
</style>
