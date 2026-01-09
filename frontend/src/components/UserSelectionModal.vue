<script setup lang="ts">
import { ref, watch } from 'vue';
import Modal from '@/components/Modal.vue';
import UserAvatar from '@/components/UserAvatar.vue';
import { useDataStore } from '@/stores/dataStore';
import type { UserInfo } from '@/types/user';

const props = defineProps<{
  show: boolean;
  currentUserId?: string | null;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'select-user', user: { uuid: string; name: string; email: string; role: string }): void;
}>();

const dataStore = useDataStore();

// State management
const searchQuery = ref('');
const users = ref<UserInfo[]>([]);
const loading = ref(false);
const error = ref<string | null>(null);

// Search debouncing
let searchTimeout: ReturnType<typeof setTimeout> | null = null;
const searchDebounceMs = 300;

// Load users
const loadUsers = async (query: string = '') => {
  loading.value = true;
  error.value = null;

  try {
    const response = await dataStore.getPaginatedUsers({
      page: 1,
      pageSize: 50,
      search: query,
      sortField: 'name',
      sortDirection: 'asc'
    });

    users.value = response.data;
  } catch (err) {
    console.error('Error loading users:', err);
    error.value = 'Failed to load users';
    users.value = [];
  } finally {
    loading.value = false;
  }
};

// Handle search input with debouncing
const handleSearchInput = () => {
  if (searchTimeout) {
    clearTimeout(searchTimeout);
  }

  searchTimeout = setTimeout(() => {
    loadUsers(searchQuery.value);
  }, searchDebounceMs);
};

// Select user
const selectUser = (user: UserInfo) => {
  emit('select-user', {
    uuid: user.uuid,
    name: user.name,
    email: user.email,
    role: user.role
  });
  emit('close');
};

// Watch for modal open/close
watch(() => props.show, (isShown) => {
  if (isShown) {
    searchQuery.value = '';
    loadUsers();
  }
});

// Clear selection (unassign user)
const clearUser = () => {
  emit('select-user', { uuid: '', name: '', email: '', role: '' });
  emit('close');
};
</script>

<template>
  <Modal :show="show" @close="$emit('close')" title="Assign User" size="md">
    <div class="flex flex-col gap-4">
      <!-- Search Input -->
      <div class="relative">
        <input
          v-model="searchQuery"
          @input="handleSearchInput"
          type="text"
          placeholder="Search users by name or email..."
          class="w-full px-4 py-2.5 bg-surface-alt border border-default rounded-lg text-primary placeholder-tertiary focus:outline-none focus:border-accent transition-colors"
        />
        <svg
          class="absolute right-3 top-1/2 -translate-y-1/2 w-5 h-5 text-tertiary pointer-events-none"
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
        </svg>
      </div>

      <!-- Clear/Unassign Button -->
      <button
        v-if="currentUserId"
        @click="clearUser"
        class="w-full px-4 py-2.5 bg-surface-alt border border-default rounded-lg text-secondary hover:bg-surface-hover hover:border-strong transition-colors text-sm font-medium flex items-center justify-center gap-2"
      >
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
        </svg>
        Unassign User
      </button>

      <!-- Loading State -->
      <div v-if="loading" class="flex items-center justify-center py-8">
        <div class="w-8 h-8 border-2 border-accent border-t-transparent rounded-full animate-spin"></div>
      </div>

      <!-- Error State -->
      <div v-else-if="error" class="text-center py-8 text-status-error">
        {{ error }}
      </div>

      <!-- User List -->
      <div v-else-if="users.length > 0" class="flex flex-col gap-1 max-h-96 overflow-y-auto">
        <button
          v-for="user in users"
          :key="user.uuid"
          @click="selectUser(user)"
          class="flex items-center gap-3 px-3 py-2.5 rounded-lg hover:bg-surface-alt transition-colors text-left group"
          :class="{
            'bg-surface-alt ring-1 ring-accent': currentUserId === user.uuid
          }"
        >
          <!-- Avatar -->
          <UserAvatar
            :name="user.uuid"
            :userName="user.name"
            :avatar="user.avatar_thumb || user.avatar_url"
            :showName="false"
            size="md"
          />

          <!-- User Info -->
          <div class="flex-1 min-w-0">
            <div class="text-sm font-medium text-primary truncate">{{ user.name }}</div>
            <div class="text-xs text-secondary truncate">{{ user.email }}</div>
          </div>

          <!-- Role Badge -->
          <span
            class="inline-flex items-center px-2 py-1 rounded-md text-xs font-medium flex-shrink-0"
            :class="{
              'bg-accent/20 text-accent border border-accent/30': user.role === 'admin' || user.role === 'technician',
              'bg-surface-alt text-secondary border border-default': user.role === 'user'
            }"
          >
            {{ user.role.charAt(0).toUpperCase() + user.role.slice(1) }}
          </span>

          <!-- Selected Indicator -->
          <svg
            v-if="currentUserId === user.uuid"
            class="w-5 h-5 text-accent flex-shrink-0"
            fill="currentColor"
            viewBox="0 0 20 20"
          >
            <path fill-rule="evenodd" d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z" clip-rule="evenodd" />
          </svg>
        </button>
      </div>

      <!-- Empty State -->
      <div v-else class="text-center py-8 text-secondary">
        <svg class="w-12 h-12 mx-auto mb-3 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M17 20h5v-2a3 3 0 00-5.356-1.857M17 20H7m10 0v-2c0-.656-.126-1.283-.356-1.857M7 20H2v-2a3 3 0 015.356-1.857M7 20v-2c0-.656.126-1.283.356-1.857m0 0a5.002 5.002 0 019.288 0M15 7a3 3 0 11-6 0 3 3 0 016 0zm6 3a2 2 0 11-4 0 2 2 0 014 0zM7 10a2 2 0 11-4 0 2 2 0 014 0z" />
        </svg>
        <p class="text-sm">{{ searchQuery ? 'No users found' : 'No users available' }}</p>
      </div>
    </div>
  </Modal>
</template>
