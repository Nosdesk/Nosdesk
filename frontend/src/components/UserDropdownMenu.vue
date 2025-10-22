<script setup lang="ts">
import { computed, ref, onMounted, onUnmounted } from 'vue';
import { useRouter } from 'vue-router';
import { useAuthStore } from '@/stores/auth';
import UserAvatar from './UserAvatar.vue';

interface Props {
  showMenu: boolean;
  buttonRef?: HTMLElement | null;
}

const props = defineProps<Props>();
const emit = defineEmits(['close']);

const router = useRouter();
const authStore = useAuthStore();

// Template ref for the dropdown
const dropdownRef = ref<HTMLElement | null>(null);

// User data from auth store
const user = computed(() => {
  if (authStore.user) {
    return {
      name: authStore.user.name,
      email: authStore.user.email,
      avatar: authStore.user.avatar_url
    };
  }
  return {
    name: "Guest",
    email: "guest@example.com",
    avatar: null
  };
});

// Click outside handler - exclude the toggle button from click-outside logic
const handleClickOutside = (event: MouseEvent) => {
  const target = event.target as Node;
  const clickedOutsideDropdown = dropdownRef.value && !dropdownRef.value.contains(target);
  const clickedOutsideButton = props.buttonRef && !props.buttonRef.contains(target);

  if (clickedOutsideDropdown && clickedOutsideButton) {
    emit('close');
  }
};

// Escape key handler
const handleEscape = (event: KeyboardEvent) => {
  if (event.key === 'Escape') {
    emit('close');
  }
};

// Add event listeners when component mounts
onMounted(() => {
  document.addEventListener('mousedown', handleClickOutside);
  document.addEventListener('keydown', handleEscape);
});

// Clean up event listeners
onUnmounted(() => {
  document.removeEventListener('mousedown', handleClickOutside);
  document.removeEventListener('keydown', handleEscape);
});

const handleUserProfileClick = () => {
  if (authStore.user) {
    router.push(`/users/${authStore.user.uuid}`);
  }
  emit('close');
};

const handleProfileSettingsClick = () => {
  router.push('/profile/settings');
  emit('close');
};

const handleAdminClick = () => {
  router.push('/admin/settings');
  emit('close');
};

const handleLogout = () => {
  try {
    // Close the user menu
    emit('close');
    
    // Log the user out using the auth store
    // The auth store will handle the redirect to the login page
    authStore.logout();
  } catch (error) {
    console.error('Logout failed:', error);
    // You could show an error notification here
  }
};
</script>

<template>
  <div
    v-if="showMenu"
    ref="dropdownRef"
    class="absolute right-0 mt-2 w-48 bg-slate-800 border border-slate-700 rounded-lg shadow-lg py-1 z-50"
    role="menu"
    tabindex="-1"
  >
    <!-- User Info -->
    <div
      class="px-4 py-3 border-b border-slate-700 hover:bg-slate-700 cursor-pointer flex items-center gap-3 min-w-0"
      @click="handleUserProfileClick"
    >
      <UserAvatar
        :name="user.name"
        :avatar="user.avatar"
        size="xl" 
        :showName="false"
        :clickable="false"
        class="flex-shrink-0"
      />
      <div class="min-w-0 flex-1">
        <div class="text-sm font-medium text-white truncate">{{ user.name }}</div>
        <div class="text-xs text-blue-400 mt-1">View Profile</div>
      </div>
    </div>

    <!-- Menu Items -->
    <div class="py-1">
      <button
        @click="handleProfileSettingsClick"
        class="w-full text-left px-4 py-2 text-sm text-slate-300 hover:bg-slate-700 focus:bg-slate-700 focus:outline-none"
        role="menuitem"
      >
        <div class="flex items-center gap-1">
          <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 mr-2" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z" />
          </svg>
          Settings
        </div>
      </button>
      
      <button
        v-if="authStore.user?.role === 'admin'"
        @click="handleAdminClick"
        class="w-full text-left px-4 py-2 text-sm text-slate-300 hover:bg-slate-700 focus:bg-slate-700 focus:outline-none"
        role="menuitem"
      >
        <div class="flex items-center gap-1">
          <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 mr-2" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
          </svg>
          Administration
        </div>
      </button>
      
      <div v-if="authStore.user?.role === 'admin'" class="border-t border-slate-600 my-1"></div>
      
      <button
        @click="handleLogout"
        class="w-full text-left px-4 py-2 text-sm text-red-400 hover:bg-slate-700 focus:bg-slate-700 focus:outline-none"
        role="menuitem"
      >
        Sign Out
      </button>
    </div>
  </div>
</template> 