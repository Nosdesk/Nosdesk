<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue';
import UserAvatar from '@/components/UserAvatar.vue';
import { useDataStore } from '@/stores/dataStore';

const props = defineProps<{
  modelValue: string; // The selected user's ID or name (bound via v-model)
  placeholder?: string;
  type: 'requester' | 'assignee'; // To differentiate between requester and assignee for emits
  currentUser?: { uuid: string; name: string; email?: string; avatar_thumb?: string | null; avatar_url?: string | null } | null; // Optional complete user object
}>();

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void; // For v-model
}>();

// Use data store for user lookups
const dataStore = useDataStore();

// Simplified state - only what we need for search
const inputValue = ref('');
const isDropdownOpen = ref(false);
const searchResults = ref<{ id: string; name: string; email: string; role?: string }[]>([]);
const isSearching = ref(false);

// References for positioning
const containerRef = ref<HTMLElement | null>(null);

// Debounce timer for search
let searchTimeout: ReturnType<typeof setTimeout> | null = null;

// Initialize input value when modelValue or currentUser changes
watch(() => [props.modelValue, props.currentUser] as const, async ([newValue, currentUserProp]) => {
  if (!newValue) {
    inputValue.value = '';
    return;
  }
  
  // If we have complete user data from props, use it directly
  if (currentUserProp && currentUserProp.uuid === newValue) {
    inputValue.value = currentUserProp.name;
    return;
  }
  
  // Fallback to cache lookup
  const userName = dataStore.getUserName(newValue);
  if (userName) {
    inputValue.value = userName;
  } else if (newValue.length === 36 && newValue.includes('-')) {
    // If it's a UUID, try to fetch user data
    const user = await dataStore.getUserByUuid(newValue);
    if (user) {
      inputValue.value = user.name;
    } else {
      inputValue.value = newValue;
    }
  } else {
    inputValue.value = newValue;
  }
}, { immediate: true });

// Computed position for the dropdown
const dropdownPosition = computed(() => {
  if (!containerRef.value || !isDropdownOpen.value) {
    return { top: '0px', left: '0px', width: '0px' }
  }
  
  const rect = containerRef.value.getBoundingClientRect()
  return {
    top: `${rect.bottom + window.scrollY + 4}px`,
    left: `${rect.left + window.scrollX}px`,
    width: `${rect.width}px`
  }
})

// Search users via API with backend role filtering
const searchUsers = async (query: string) => {
  console.log(`🔍 searchUsers called with query: "${query}", type: ${props.type}`);
  
  // For requesters, require at least 2 characters
  if (props.type === 'requester' && (!query || query.length < 2)) {
    console.log('🔍 Requester query too short, clearing results');
    searchResults.value = [];
    return;
  }
  
  try {
    isSearching.value = true;
    console.log('🔍 Making API call to getPaginatedUsers');
    
    // For assignees, we'll make two API calls to get both admin and technician users
    if (props.type === 'assignee') {
      console.log('🔍 Fetching assignee users (admins and technicians)');
      
      // Fetch admins and technicians separately, then combine
      const [adminResponse, technicianResponse] = await Promise.all([
        dataStore.getPaginatedUsers({
          page: 1,
          pageSize: 50,
          search: query || '',
          sortField: 'name',
          sortDirection: 'asc',
          role: 'admin'
        }),
        dataStore.getPaginatedUsers({
          page: 1,
          pageSize: 50,
          search: query || '',
          sortField: 'name',
          sortDirection: 'asc',
          role: 'technician'
        })
      ]);
      
      // Combine and deduplicate results
      const allAssigneeUsers = [...adminResponse.data, ...technicianResponse.data];
      console.log(`🔍 Found ${adminResponse.data.length} admins and ${technicianResponse.data.length} technicians`);
      
      // Transform users to the format expected by the component
      searchResults.value = allAssigneeUsers.map(user => ({
        id: user.uuid,
        name: user.name,
        email: user.email,
        role: user.role
      }));
    } else {
      // For requesters, search all users
      const response = await dataStore.getPaginatedUsers({
        page: 1,
        pageSize: 50,
        search: query || '',
        sortField: 'name',
        sortDirection: 'asc'
      });
      
      console.log(`🔍 API response received, ${response.data.length} users found`);
      
      // Transform users to the format expected by the component
      searchResults.value = response.data.map(user => ({
        id: user.uuid,
        name: user.name,
        email: user.email,
        role: user.role
      }));
    }
    
    console.log(`🔍 Final search results: ${searchResults.value.length} users`);
  } catch (error) {
    console.error('Error searching users:', error);
    searchResults.value = [];
  } finally {
    isSearching.value = false;
  }
};

// Debounced search function
const debouncedSearch = (query: string) => {
  if (searchTimeout) {
    clearTimeout(searchTimeout);
  }
  
  searchTimeout = setTimeout(() => {
    searchUsers(query);
  }, 300);
};

// Handle user selection
const selectUser = (user: { id: string; name: string; email: string; role?: string }) => {
  inputValue.value = user.name;
  emit('update:modelValue', user.id);
  isDropdownOpen.value = false;
  searchResults.value = [];
};

// Clear selection
const clearSelection = () => {
  inputValue.value = '';
  emit('update:modelValue', '');
  isDropdownOpen.value = false;
  searchResults.value = [];
};

// Handle input focus
const handleFocus = async (event: Event) => {
  isDropdownOpen.value = true;

  // Select all text when input receives focus
  const input = event.target as HTMLInputElement;
  setTimeout(() => input.select(), 0);

  // For assignees, always show all eligible users when focused
  if (props.type === 'assignee') {
    console.log('🎯 Loading assignee users on focus');
    await searchUsers(''); // Load all eligible users with empty search
  }
  // For requesters, only search if there's already input
  else if (inputValue.value && inputValue.value.length >= 2) {
    debouncedSearch(inputValue.value);
  }
};

// Handle input changes
const handleInput = () => {
  isDropdownOpen.value = true;

  // If user clears the input, clear the selection
  if (inputValue.value === '') {
    emit('update:modelValue', '');
    if (props.type === 'assignee') {
      searchUsers(''); // Show all eligible users for assignees
    } else {
      searchResults.value = [];
    }
    return;
  }

  // Search when input changes (for both types)
  debouncedSearch(inputValue.value);
};

// Handle input blur - just close dropdown, don't restore anything
const handleBlur = () => {
  // Delay closing to allow click events on dropdown items
  setTimeout(() => {
    isDropdownOpen.value = false;
    searchResults.value = [];
  }, 200);
};

// Handle clicks outside
const handleClickOutside = (event: MouseEvent) => {
  const dropdown = document.querySelector('.user-autocomplete-dropdown');
  
  if (containerRef.value && !containerRef.value.contains(event.target as Node) && 
      dropdown && !dropdown.contains(event.target as Node)) {
    isDropdownOpen.value = false;
    searchResults.value = [];
  }
};

// Cleanup
onMounted(() => {
  document.addEventListener('click', handleClickOutside);
});

onUnmounted(() => {
  document.removeEventListener('click', handleClickOutside);
  if (searchTimeout) {
    clearTimeout(searchTimeout);
  }
});
</script>

<template>
  <div ref="containerRef" class="relative w-full">
    <!-- User Selection Container -->
    <div class="flex items-center gap-2 px-2.5 py-1.5 min-h-[36px]">
      <!-- Avatar Space -->
      <div class="flex-shrink-0 w-6 h-6 flex items-center justify-center">
        <UserAvatar
          v-if="modelValue && inputValue && !isDropdownOpen"
          :name="modelValue"
          :userName="currentUser?.uuid === modelValue ? currentUser.name : undefined"
          :avatarUrl="currentUser?.uuid === modelValue ? (currentUser.avatar_thumb || currentUser.avatar_url) : undefined"
          :showName="false"
          size="sm"
        />
        <!-- Placeholder when no user selected -->
        <div
          v-else
          class="w-6 h-6 rounded-full bg-surface border border-subtle flex items-center justify-center transition-all duration-200"
          :class="{ 'border-default': isDropdownOpen }"
        >
          <svg class="w-3 h-3 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z" />
          </svg>
        </div>
      </div>
      
      <!-- Input Container -->
      <div class="flex-1 relative">
        <input
          type="text"
          v-model="inputValue"
          @focus="handleFocus"
          @input="handleInput"
          @blur="handleBlur"
          :placeholder="placeholder || 'Select user...'"
          class="w-full bg-transparent text-secondary placeholder-tertiary focus:outline-none text-sm transition-all duration-200 leading-tight"
        />
      </div>
      
      <!-- Action Buttons -->
      <div class="flex items-center gap-1 flex-shrink-0">
        <!-- Clear button -->
        <button
          v-if="modelValue"
          @click.stop="clearSelection"
          class="p-1 text-tertiary hover:text-secondary hover:bg-surface-hover rounded transition-all duration-200 group"
          type="button"
          title="Clear selection"
        >
          <svg class="w-3 h-3 group-hover:scale-110 transition-transform duration-200" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>
        
        <!-- Search/Loading indicator -->
        <div class="flex items-center justify-center w-5 h-5">
          <div v-if="isSearching" class="w-3 h-3 border-2 border-tertiary border-t-transparent rounded-full animate-spin"></div>
          <svg v-else-if="isDropdownOpen" class="w-3 h-3 text-tertiary transition-transform duration-200" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
          </svg>
          <svg v-else class="w-3 h-3 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
          </svg>
        </div>
      </div>
    </div>
    
    <!-- Dropdown with users -->
    <Teleport to="body">
      <div
        v-if="isDropdownOpen && searchResults.length > 0 && containerRef"
        class="user-autocomplete-dropdown fixed bg-surface-alt rounded-lg shadow-xl border border-default min-w-max z-[9999] overflow-hidden"
        :style="dropdownPosition"
      >
        <div class="py-1 max-h-56 overflow-y-auto">
          <button
            v-for="user in searchResults.slice(0, 8)"
            :key="user.id"
            @click="selectUser(user)"
            class="w-full px-2.5 py-1.5 text-left flex items-center gap-2.5 hover:bg-surface-hover transition-all duration-150 group"
            :class="{
              'bg-surface': modelValue === user.id
            }"
          >
            <UserAvatar
              :name="user.id"
              :userName="user.name"
              :showName="false"
              size="sm"
            />
            <div class="flex-1 min-w-0">
              <div class="text-sm font-medium text-secondary truncate">{{ user.name }}</div>
              <div class="flex items-center gap-2">
                <div class="text-xs text-tertiary truncate">{{ user.email }}</div>
                <div v-if="user.role && props.type === 'assignee'" class="flex-shrink-0">
                  <span class="px-1.5 py-0.5 text-xs font-medium rounded-md"
                    :class="{
                      'bg-red-900/30 text-red-300 border border-red-700/50': user.role === 'admin',
                      'bg-blue-900/30 text-blue-300 border border-blue-700/50': user.role === 'technician'
                    }">
                    {{ user.role === 'admin' ? 'Admin' : 'Technician' }}
                  </span>
                </div>
              </div>
            </div>
            <svg 
              v-if="modelValue === user.id" 
              class="w-3 h-3 text-blue-400 flex-shrink-0" 
              fill="currentColor" 
              viewBox="0 0 20 20"
            >
              <path fill-rule="evenodd" d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z" clip-rule="evenodd" />
            </svg>
          </button>
        </div>
      </div>
    </Teleport>
    
    <!-- Loading state -->
    <Teleport to="body">
      <div
        v-if="isDropdownOpen && isSearching && containerRef"
        class="user-autocomplete-dropdown fixed bg-surface-alt rounded-lg shadow-xl border border-default min-w-max z-[9999]"
        :style="dropdownPosition"
      >
        <div class="p-3 flex items-center justify-center gap-2 text-tertiary">
          <div class="w-4 h-4 border-2 border-tertiary border-t-transparent rounded-full animate-spin"></div>
          <span class="text-sm">Searching...</span>
        </div>
      </div>
    </Teleport>

    <!-- No users found message -->
    <Teleport to="body">
      <div
        v-if="isDropdownOpen && !isSearching && searchResults.length === 0 && inputValue && inputValue.length >= 2 && containerRef"
        class="user-autocomplete-dropdown fixed bg-surface-alt rounded-lg shadow-xl border border-default min-w-max z-[9999]"
        :style="dropdownPosition"
      >
        <div class="p-3 text-center text-tertiary">
          <div class="text-sm">
            {{ props.type === 'assignee' ? 'No technicians or administrators found' : 'No users found' }}
          </div>
        </div>
      </div>
    </Teleport>

    <!-- Search prompt -->
    <Teleport to="body">
      <div
        v-if="isDropdownOpen && !isSearching && inputValue && inputValue.length < 2 && !modelValue && containerRef"
        class="user-autocomplete-dropdown fixed bg-surface-alt rounded-lg shadow-xl border border-default min-w-max z-[9999]"
        :style="dropdownPosition"
      >
        <div class="p-3 text-center text-tertiary">
          <div class="text-sm">Type to search</div>
        </div>
      </div>
    </Teleport>
  </div>
</template>