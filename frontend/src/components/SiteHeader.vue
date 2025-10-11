<script setup lang="ts">
import { useRouter, useRoute } from "vue-router";
import { computed, ref, onMounted, onUnmounted } from "vue";
import UserAvatar from "./UserAvatar.vue";
import UserDropdownMenu from "./UserDropdownMenu.vue";
import HeaderTitle from "./HeaderTitle.vue";
import DocumentIconSelector from "./DocumentIconSelector.vue";
import TicketIdentifier from "./TicketIdentifier.vue";
import PageUrlDisplay from "./PageUrlDisplay.vue";
import ticketService from '@/services/ticketService';
import { useAuthStore } from '@/stores/auth';

const router = useRouter();
const authStore = useAuthStore();

interface Props {
  title?: string;
  showCreateButton?: boolean;
  useRouteTitle?: boolean;
  ticket: { id: number; title: string } | null;
  document: { id: string; title: string; icon: string } | null;
  isTransitioning?: boolean;
  pageUrl?: string;
  navbarCollapsed?: boolean;
}

const props = withDefaults(defineProps<Props>(), {
  useRouteTitle: false,
  ticket: null,
  document: null,
  isTransitioning: false,
  pageUrl: undefined,
  navbarCollapsed: false,
});

const emit = defineEmits(["updateDocumentTitle", "updateDocumentIcon", "previewDocumentTitle"]);

const isTicketView = computed(() => {
  return props.ticket !== null;
});

const isDocumentView = computed(() => {
  return props.document !== null;
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

// Add isCreatingTicket ref
const isCreatingTicket = ref(false);

const navigateToCreateTicket = async () => {
  if (import.meta.env.DEV) {
    console.log('navigateToCreateTicket called');
  }
  if (isCreatingTicket.value) {
    if (import.meta.env.DEV) {
      console.log('Already creating ticket, returning');
    }
    return; // Prevent multiple clicks
  }
  
  try {
    if (import.meta.env.DEV) {
      console.log('Setting isCreatingTicket to true');
    }
    isCreatingTicket.value = true;
    
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
    // You could show an error notification here
    
    if (import.meta.env.DEV) {
      console.log('Falling back to create ticket page');
    }
    // Fallback to the tickets page if creation fails
    router.push("/tickets");
  } finally {
    if (import.meta.env.DEV) {
      console.log('Setting isCreatingTicket to false');
    }
    isCreatingTicket.value = false;
  }
};

// Check if we're on mobile
const isMobile = ref(window.innerWidth < 768); // md breakpoint

// Update mobile status on resize
const updateMobileStatus = () => {
  isMobile.value = window.innerWidth < 768;
};

// Event listeners for click outside
onMounted(() => {
  // Add resize listener
  window.addEventListener('resize', updateMobileStatus);
});

onUnmounted(() => {
  // Remove resize listener
  window.removeEventListener('resize', updateMobileStatus);
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
  <header class="bg-slate-800 border-b border-slate-700 relative z-[999]">
    <div class="flex items-center justify-between h-16 px-4 md:px-6 gap-2">
      <!-- Left side - Title area -->
      <div class="flex items-center flex-1 min-w-0">
        <template v-if="isTicketView && props.ticket">
          <div class="flex items-center gap-2 min-w-0 flex-1">
            <TicketIdentifier :ticketId="props.ticket.id" size="md" class="flex-shrink-0" />
            <!-- Display ticket title as read-only in header -->
            <h1 class="text-xl font-semibold text-white truncate flex-1 min-w-0">
              {{ props.ticket.title || 'Untitled Ticket' }}
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
              @update-title="handleUpdateDocumentTitle"
              @update-title-preview="handlePreviewDocumentTitle"
              class="min-w-0 flex-1"
            />
          </div>
        </template>
        <template v-else>
          <h1 class="text-xl font-semibold text-white truncate">{{ displayTitle }}</h1>
        </template>
      </div>

      <!-- Right side -->
      <div class="flex items-center gap-2 md:gap-4 flex-shrink-0">
        <!-- Create Ticket Button -->
        <button
          v-if="props.showCreateButton"
          @click="navigateToCreateTicket"
          :disabled="isCreatingTicket"
          class="px-2 md:px-4 py-2 text-sm font-medium bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-1 md:gap-2"
          :aria-label="isCreatingTicket ? 'Creating new ticket...' : 'Create new ticket'"
        >
          <!-- Always show icon -->
          <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path v-if="isCreatingTicket" stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 6v6m0 0v6m0-6h6m-6 0H6" class="animate-spin" />
            <path v-else stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 6v6m0 0v6m0-6h6m-6 0H6" />
          </svg>
          
          <span class="hidden md:inline">{{ isCreatingTicket ? 'Creating...' : 'Create Ticket' }}</span>
        </button>

        <!-- User Profile Menu -->
        <div class="relative">
          <button
            ref="buttonRef"
            @click="toggleUserMenu"
            class="flex items-center justify-center hover:ring-2 hover:ring-blue-500 rounded-full focus:outline-none focus:ring-2 focus:ring-blue-500"
            aria-haspopup="true"
            :aria-expanded="showUserMenu"
          >
            <UserAvatar
              :showName="false"
              :name="user.name"
              :avatar="user.avatar"
              size="md" 
              :clickable="false"
              ref="headerAvatarRef"
            />
          </button>

          <!-- User Dropdown Menu -->
          <UserDropdownMenu
            :showMenu="showUserMenu"
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
</style>
