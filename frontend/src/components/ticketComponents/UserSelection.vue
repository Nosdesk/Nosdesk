<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from 'vue';
import UserAvatar from '@/components/UserAvatar.vue';
import { useDataStore } from '@/stores/dataStore';
import { useMobileDetection } from '@/composables/useMobileDetection';

interface UserResult {
  id: string;
  name: string;
  email: string;
  role?: string;
  avatar_thumb?: string | null;
  avatar_url?: string | null;
}

const props = defineProps<{
  modelValue: string;
  placeholder?: string;
  type: 'requester' | 'assignee';
  currentUser?: { uuid: string; name: string; email?: string; avatar_thumb?: string | null; avatar_url?: string | null } | null;
}>();

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void;
}>();

const dataStore = useDataStore();
const { isMobile } = useMobileDetection('md'); // 768px breakpoint

// State
const inputValue = ref('');
const isDropdownOpen = ref(false);
const searchResults = ref<UserResult[]>([]);
const isSearching = ref(false);
const highlightedIndex = ref(-1);

// References
const containerRef = ref<HTMLElement | null>(null);
const inputRef = ref<HTMLInputElement | null>(null);
const dropdownRef = ref<HTMLElement | null>(null);
const listRef = ref<HTMLElement | null>(null);
const mobileSearchInput = ref<HTMLInputElement | null>(null);


// Debounce timer
let searchTimeout: ReturnType<typeof setTimeout> | null = null;

// Scroll position for iOS scroll lock
let scrollPosition = 0;

// Position state for desktop dropdown
const menuPosition = ref({ top: 0, left: 0, width: 0, openUpward: false });

// Initialize input value when modelValue or currentUser changes
watch(() => [props.modelValue, props.currentUser] as const, async ([newValue, currentUserProp]) => {
  if (!newValue) {
    inputValue.value = '';
    return;
  }

  if (currentUserProp && currentUserProp.uuid === newValue) {
    inputValue.value = currentUserProp.name;
    return;
  }

  const userName = dataStore.getUserName(newValue);
  if (userName) {
    inputValue.value = userName;
  } else if (newValue.length === 36 && newValue.includes('-')) {
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

// Computed styles for desktop dropdown positioning
const dropdownStyles = computed(() => {
  if (isMobile.value) return {};

  return {
    position: 'fixed' as const,
    top: `${menuPosition.value.top}px`,
    left: `${menuPosition.value.left}px`,
    width: `${menuPosition.value.width}px`,
    zIndex: 9999,
  };
});

// Update position for desktop dropdown
const updatePosition = () => {
  if (!containerRef.value || !isDropdownOpen.value || isMobile.value) return;

  const rect = containerRef.value.getBoundingClientRect();
  const viewportHeight = window.innerHeight;
  const viewportWidth = window.innerWidth;
  const spaceBelow = viewportHeight - rect.bottom;
  const spaceAbove = rect.top;
  const menuHeight = 320; // max-h-80

  // Determine if should open upward
  const openUpward = spaceBelow < menuHeight && spaceAbove > spaceBelow;

  // Calculate width - minimum 280px for good readability
  const dropdownWidth = Math.max(rect.width, 280);

  // Ensure dropdown doesn't overflow viewport horizontally
  let left = rect.left;
  if (left + dropdownWidth > viewportWidth - 16) {
    left = viewportWidth - dropdownWidth - 16;
  }
  if (left < 16) left = 16;

  menuPosition.value = {
    top: openUpward ? rect.top - Math.min(menuHeight, spaceAbove - 8) : rect.bottom + 4,
    left,
    width: dropdownWidth,
    openUpward,
  };
};

// Search users via API
const searchUsers = async (query: string) => {
  // For requesters, require at least 2 characters
  if (props.type === 'requester' && (!query || query.length < 2)) {
    searchResults.value = [];
    return;
  }

  try {
    isSearching.value = true;

    if (props.type === 'assignee') {
      // Fetch admins and technicians
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

      const allAssigneeUsers = [...adminResponse.data, ...technicianResponse.data];

      // Include avatar data in results
      searchResults.value = allAssigneeUsers.map(user => ({
        id: user.uuid,
        name: user.name,
        email: user.email,
        role: user.role,
        avatar_thumb: user.avatar_thumb,
        avatar_url: user.avatar_url,
      }));
    } else {
      const response = await dataStore.getPaginatedUsers({
        page: 1,
        pageSize: 50,
        search: query || '',
        sortField: 'name',
        sortDirection: 'asc'
      });

      // Include avatar data in results
      searchResults.value = response.data.map(user => ({
        id: user.uuid,
        name: user.name,
        email: user.email,
        role: user.role,
        avatar_thumb: user.avatar_thumb,
        avatar_url: user.avatar_url,
      }));
    }

    highlightedIndex.value = -1;
  } catch (error) {
    console.error('Error searching users:', error);
    searchResults.value = [];
  } finally {
    isSearching.value = false;
  }
};

// Debounced search
const debouncedSearch = (query: string) => {
  if (searchTimeout) clearTimeout(searchTimeout);
  searchTimeout = setTimeout(() => searchUsers(query), 300);
};

// Lock scroll for iOS - prevents background scrolling when modal is open
const lockScroll = () => {
  scrollPosition = window.pageYOffset;
  document.body.style.overflow = 'hidden';
  document.body.style.position = 'fixed';
  document.body.style.top = `-${scrollPosition}px`;
  document.body.style.left = '0';
  document.body.style.right = '0';
};

// Unlock scroll for iOS - restores scroll position
const unlockScroll = () => {
  document.body.style.overflow = '';
  document.body.style.position = '';
  document.body.style.top = '';
  document.body.style.left = '';
  document.body.style.right = '';
  window.scrollTo(0, scrollPosition);
};

// Open dropdown
const openDropdown = async () => {
  isDropdownOpen.value = true;
  highlightedIndex.value = -1;

  // Lock body scroll on mobile using iOS-safe method
  if (isMobile.value) {
    lockScroll();
  }

  await nextTick();
  updatePosition();

  // Focus mobile search input after sheet opens
  if (isMobile.value && mobileSearchInput.value) {
    // Small delay to ensure sheet animation has started
    setTimeout(() => {
      mobileSearchInput.value?.focus();
    }, 100);
  }

  // For assignees, load all eligible users
  if (props.type === 'assignee') {
    await searchUsers('');
  } else if (inputValue.value && inputValue.value.length >= 2) {
    debouncedSearch(inputValue.value);
  }
};

// Close dropdown
const closeDropdown = () => {
  isDropdownOpen.value = false;
  searchResults.value = [];
  highlightedIndex.value = -1;

  // Unlock body scroll using iOS-safe method
  if (isMobile.value) {
    unlockScroll();
  }
};

// Select user
const selectUser = (user: UserResult) => {
  inputValue.value = user.name;
  emit('update:modelValue', user.id);
  closeDropdown();
};

// Clear selection
const clearSelection = () => {
  inputValue.value = '';
  emit('update:modelValue', '');
  closeDropdown();
};

// Handle input focus
const handleFocus = (event: Event) => {
  openDropdown();
  const input = event.target as HTMLInputElement;
  setTimeout(() => input.select(), 0);
};

// Handle input changes
const handleInput = () => {
  if (!isDropdownOpen.value) {
    openDropdown();
  }

  if (inputValue.value === '') {
    emit('update:modelValue', '');
    if (props.type === 'assignee') {
      searchUsers('');
    } else {
      searchResults.value = [];
    }
    return;
  }

  debouncedSearch(inputValue.value);
};

// Handle blur - delay to allow click events
const handleBlur = () => {
  setTimeout(() => {
    if (!dropdownRef.value?.contains(document.activeElement)) {
      closeDropdown();
    }
  }, 150);
};

// Keyboard navigation
const handleKeydown = (event: KeyboardEvent) => {
  if (!isDropdownOpen.value) {
    if (event.key === 'ArrowDown' || event.key === 'Enter') {
      openDropdown();
      event.preventDefault();
    }
    return;
  }

  const maxIndex = Math.min(searchResults.value.length, 10) - 1;

  switch (event.key) {
    case 'ArrowDown':
      event.preventDefault();
      highlightedIndex.value = Math.min(highlightedIndex.value + 1, maxIndex);
      scrollToHighlighted();
      break;
    case 'ArrowUp':
      event.preventDefault();
      highlightedIndex.value = Math.max(highlightedIndex.value - 1, 0);
      scrollToHighlighted();
      break;
    case 'Enter':
      event.preventDefault();
      if (highlightedIndex.value >= 0 && searchResults.value[highlightedIndex.value]) {
        selectUser(searchResults.value[highlightedIndex.value]);
      }
      break;
    case 'Escape':
      event.preventDefault();
      closeDropdown();
      break;
  }
};

// Scroll to keep highlighted item visible
const scrollToHighlighted = () => {
  nextTick(() => {
    const list = listRef.value;
    if (!list) return;

    const highlighted = list.querySelector('[data-highlighted="true"]');
    if (highlighted) {
      highlighted.scrollIntoView({ block: 'nearest' });
    }
  });
};

// Handle clicks outside
const handleClickOutside = (event: MouseEvent) => {
  if (!isDropdownOpen.value) return;

  const target = event.target as Node;
  if (containerRef.value?.contains(target)) return;
  if (dropdownRef.value?.contains(target)) return;

  closeDropdown();
};

// Update position on scroll (desktop only)
let rafId: number | null = null;
const handleScroll = () => {
  if (!isDropdownOpen.value || isMobile.value) return;

  if (rafId) cancelAnimationFrame(rafId);
  rafId = requestAnimationFrame(() => {
    updatePosition();
    rafId = null;
  });
};

// Handle resize
const handleResize = () => {
  if (isDropdownOpen.value) {
    updatePosition();
  }
};


// Lifecycle
onMounted(() => {
  document.addEventListener('mousedown', handleClickOutside);
  window.addEventListener('scroll', handleScroll, true);
  window.addEventListener('resize', handleResize);
});

onUnmounted(() => {
  document.removeEventListener('mousedown', handleClickOutside);
  window.removeEventListener('scroll', handleScroll, true);
  window.removeEventListener('resize', handleResize);
  if (searchTimeout) clearTimeout(searchTimeout);
  if (rafId) cancelAnimationFrame(rafId);
  // Ensure scroll is unlocked
  if (isMobile.value && isDropdownOpen.value) {
    unlockScroll();
  }
});

// Get avatar for a user
const getUserAvatar = (user: UserResult) => {
  return user.avatar_thumb || user.avatar_url || undefined;
};

// Limit displayed results
const displayedResults = computed(() => searchResults.value.slice(0, 10));

// Show helper text
const showHelperText = computed(() => {
  if (isSearching.value) return false;
  if (searchResults.value.length > 0) return false;
  if (props.type === 'requester' && (!inputValue.value || inputValue.value.length < 2)) {
    return 'type-to-search';
  }
  if (inputValue.value && inputValue.value.length >= 2) {
    return 'no-results';
  }
  return false;
});
</script>

<template>
  <div ref="containerRef" class="relative w-full">
    <!-- Trigger/Input Container -->
    <div
      class="flex items-center gap-2 sm:gap-2.5 px-2.5 sm:px-3 min-h-[44px] sm:min-h-[40px] cursor-text"
      @click="inputRef?.focus()"
    >
      <!-- Avatar -->
      <div class="flex-shrink-0 w-7 h-7 sm:w-6 sm:h-6 flex items-center justify-center">
        <UserAvatar
          v-if="modelValue && inputValue && !isDropdownOpen"
          :name="modelValue"
          :userName="currentUser?.uuid === modelValue ? currentUser.name : undefined"
          :avatar="currentUser?.uuid === modelValue ? (currentUser.avatar_thumb || currentUser.avatar_url) : undefined"
          :showName="false"
          size="sm"
          :clickable="false"
        />
        <div
          v-else
          class="w-7 h-7 sm:w-6 sm:h-6 rounded-full bg-surface border border-subtle flex items-center justify-center transition-colors"
          :class="{ 'border-accent/50 bg-accent/5': isDropdownOpen }"
        >
          <svg class="w-3.5 h-3.5 sm:w-3 sm:h-3 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z" />
          </svg>
        </div>
      </div>

      <!-- Input -->
      <div class="flex-1 min-w-0">
        <input
          ref="inputRef"
          type="text"
          v-model="inputValue"
          @focus="handleFocus"
          @input="handleInput"
          @blur="handleBlur"
          @keydown="handleKeydown"
          :placeholder="placeholder || 'Select user...'"
          class="w-full bg-transparent text-secondary placeholder-tertiary focus:outline-none text-sm leading-tight py-1"
          autocomplete="off"
          autocorrect="off"
          autocapitalize="off"
          spellcheck="false"
        />
      </div>

      <!-- Action Buttons -->
      <div class="flex items-center gap-1.5 flex-shrink-0">
        <!-- Clear button - larger touch target on mobile -->
        <button
          v-if="modelValue"
          @click.stop="clearSelection"
          class="p-2 sm:p-1.5 -m-1 sm:m-0 text-tertiary hover:text-secondary hover:bg-surface-hover rounded-full transition-colors"
          type="button"
          title="Clear selection"
        >
          <svg class="w-4 h-4 sm:w-3.5 sm:h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>

        <!-- Loading/Search indicator -->
        <div class="flex items-center justify-center w-5 h-5">
          <div v-if="isSearching" class="w-4 h-4 border-2 border-tertiary border-t-transparent rounded-full animate-spin" />
          <svg v-else-if="isDropdownOpen" class="w-4 h-4 text-tertiary rotate-180 transition-transform" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
          </svg>
          <svg v-else class="w-4 h-4 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
          </svg>
        </div>
      </div>
    </div>

    <!-- Dropdown - Desktop (Floating) -->
    <Teleport to="body">
      <Transition
        enter-active-class="transition duration-150 ease-out"
        enter-from-class="opacity-0 scale-95"
        enter-to-class="opacity-100 scale-100"
        leave-active-class="transition duration-100 ease-in"
        leave-from-class="opacity-100 scale-100"
        leave-to-class="opacity-0 scale-95"
      >
        <div
          v-if="isDropdownOpen && !isMobile && containerRef"
          ref="dropdownRef"
          class="user-selection-dropdown bg-surface border border-default rounded-xl shadow-2xl overflow-hidden"
          :style="dropdownStyles"
        >
          <!-- Results List -->
          <div
            v-if="displayedResults.length > 0"
            ref="listRef"
            class="py-1 max-h-80 overflow-y-auto overscroll-contain"
          >
            <button
              v-for="(user, index) in displayedResults"
              :key="user.id"
              @click="selectUser(user)"
              @mouseenter="highlightedIndex = index"
              :data-highlighted="highlightedIndex === index"
              class="w-full px-3 py-2.5 text-left flex items-center gap-3 transition-colors"
              :class="{
                'bg-accent/10': highlightedIndex === index,
                'bg-surface-alt': modelValue === user.id && highlightedIndex !== index,
              }"
            >
              <UserAvatar
                :name="user.id"
                :userName="user.name"
                :avatar="getUserAvatar(user)"
                :showName="false"
                size="sm"
                :clickable="false"
              />
              <div class="flex-1 min-w-0">
                <div class="text-sm font-medium text-primary truncate">{{ user.name }}</div>
                <div class="flex items-center gap-2 mt-0.5">
                  <span class="text-xs text-tertiary truncate">{{ user.email }}</span>
                  <span
                    v-if="user.role && type === 'assignee'"
                    class="flex-shrink-0 px-1.5 py-0.5 text-xs font-medium rounded bg-accent/10 text-accent"
                  >
                    {{ user.role === 'admin' ? 'Admin' : 'Tech' }}
                  </span>
                </div>
              </div>
              <svg
                v-if="modelValue === user.id"
                class="w-4 h-4 text-accent flex-shrink-0"
                fill="currentColor"
                viewBox="0 0 20 20"
              >
                <path fill-rule="evenodd" d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z" clip-rule="evenodd" />
              </svg>
            </button>
          </div>

          <!-- Helper Text -->
          <div v-else-if="showHelperText" class="px-4 py-6 flex flex-col items-center justify-center text-tertiary">
            <template v-if="showHelperText === 'type-to-search'">
              <svg class="w-8 h-8 mb-2 opacity-50" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
              </svg>
              <p class="text-sm">Type to search users</p>
            </template>
            <template v-else-if="showHelperText === 'no-results'">
              <svg class="w-8 h-8 mb-2 opacity-50" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M18.364 18.364A9 9 0 005.636 5.636m12.728 12.728A9 9 0 015.636 5.636m12.728 12.728L5.636 5.636" />
              </svg>
              <p class="text-sm">{{ type === 'assignee' ? 'No technicians or admins found' : 'No users found' }}</p>
            </template>
          </div>

          <!-- Loading -->
          <div v-else-if="isSearching" class="px-4 py-6 text-center">
            <div class="w-6 h-6 mx-auto border-2 border-tertiary border-t-transparent rounded-full animate-spin" />
            <p class="text-sm text-tertiary mt-2">Searching...</p>
          </div>
        </div>
      </Transition>
    </Teleport>

    <!-- Dropdown - Mobile (Bottom Sheet) -->
    <Teleport to="body">
      <Transition
        enter-active-class="transition duration-300 ease-out"
        enter-from-class="opacity-0"
        enter-to-class="opacity-100"
        leave-active-class="transition duration-200 ease-in"
        leave-from-class="opacity-100"
        leave-to-class="opacity-0"
      >
        <div
          v-if="isDropdownOpen && isMobile"
          class="fixed inset-0 bg-black/50 z-[9998]"
          style="touch-action: none;"
          @click="closeDropdown"
          @touchmove.prevent
        />
      </Transition>

      <Transition
        enter-active-class="transition duration-300 ease-out"
        enter-from-class="translate-y-full"
        enter-to-class="translate-y-0"
        leave-active-class="transition duration-200 ease-in"
        leave-from-class="translate-y-0"
        leave-to-class="translate-y-full"
      >
        <div
          v-if="isDropdownOpen && isMobile"
          ref="dropdownRef"
          class="user-selection-dropdown fixed inset-x-0 bottom-0 bg-surface rounded-t-2xl shadow-2xl z-[9999] flex flex-col max-h-[55dvh]"
        >
          <!-- Handle bar -->
          <div class="flex justify-center py-2 flex-shrink-0">
            <div class="w-8 h-1 bg-border-default rounded-full" />
          </div>

          <!-- Header -->
          <div class="px-3 pb-1.5 flex-shrink-0">
            <h3 class="text-sm font-semibold text-primary">
              {{ type === 'assignee' ? 'Select Assignee' : 'Select Requester' }}
            </h3>
          </div>

          <!-- Results List -->
          <div
            v-if="displayedResults.length > 0"
            ref="listRef"
            class="flex-1 overflow-y-auto overscroll-contain min-h-0"
          >
            <button
              v-for="(user, index) in displayedResults"
              :key="user.id"
              @click="selectUser(user)"
              class="w-full px-3 py-2.5 text-left flex items-center gap-3 active:bg-surface-alt transition-colors border-b border-subtle last:border-b-0"
              :class="{ 'bg-accent/5': modelValue === user.id }"
            >
              <UserAvatar
                :name="user.id"
                :userName="user.name"
                :avatar="getUserAvatar(user)"
                :showName="false"
                size="sm"
                :clickable="false"
              />
              <div class="flex-1 min-w-0">
                <div class="text-sm font-medium text-primary truncate">{{ user.name }}</div>
                <div class="flex items-center gap-1.5">
                  <span class="text-xs text-tertiary truncate">{{ user.email }}</span>
                  <span
                    v-if="user.role && type === 'assignee'"
                    class="flex-shrink-0 px-1.5 py-0.5 text-xs font-medium rounded bg-accent/10 text-accent"
                  >
                    {{ user.role === 'admin' ? 'Admin' : 'Tech' }}
                  </span>
                </div>
              </div>
              <svg
                v-if="modelValue === user.id"
                class="w-4 h-4 text-accent flex-shrink-0"
                fill="currentColor"
                viewBox="0 0 20 20"
              >
                <path fill-rule="evenodd" d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z" clip-rule="evenodd" />
              </svg>
            </button>
          </div>

          <!-- Helper Text -->
          <div v-else-if="showHelperText" class="flex-1 flex flex-col items-center justify-center px-3 py-6 text-tertiary min-h-0">
            <template v-if="showHelperText === 'type-to-search'">
              <svg class="w-8 h-8 mb-1.5 opacity-50" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
              </svg>
              <p class="text-xs">Type to search users</p>
            </template>
            <template v-else-if="showHelperText === 'no-results'">
              <svg class="w-8 h-8 mb-1.5 opacity-50" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M18.364 18.364A9 9 0 005.636 5.636m12.728 12.728A9 9 0 015.636 5.636m12.728 12.728L5.636 5.636" />
              </svg>
              <p class="text-xs">{{ type === 'assignee' ? 'No technicians or admins found' : 'No users found' }}</p>
            </template>
          </div>

          <!-- Loading -->
          <div v-else-if="isSearching" class="flex-1 flex items-center justify-center px-3 py-6 min-h-0">
            <div class="text-center">
              <div class="w-5 h-5 mx-auto border-2 border-tertiary border-t-transparent rounded-full animate-spin" />
              <p class="text-xs text-tertiary mt-1.5">Searching...</p>
            </div>
          </div>

          <!-- Bottom section: Search + Actions -->
          <div class="flex-shrink-0 border-t border-default bg-surface">
            <!-- Search bar -->
            <div class="px-3 pt-2 pb-1.5">
              <div class="flex items-center gap-2.5 bg-surface-alt rounded-lg px-3 py-2">
                <svg class="w-4 h-4 text-tertiary flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
                </svg>
                <input
                  ref="mobileSearchInput"
                  type="text"
                  v-model="inputValue"
                  @input="handleInput"
                  :placeholder="type === 'assignee' ? 'Search technicians & admins...' : 'Search users...'"
                  class="flex-1 bg-transparent text-primary placeholder-tertiary focus:outline-none text-sm"
                  autocomplete="off"
                  autocorrect="off"
                  autocapitalize="off"
                  enterkeyhint="search"
                />
                <div v-if="isSearching" class="w-4 h-4 border-2 border-tertiary border-t-transparent rounded-full animate-spin" />
              </div>
            </div>

            <!-- Action buttons -->
            <div class="px-3 pb-3 safe-area-inset-bottom">
              <div class="flex gap-2">
                <button
                  v-if="modelValue"
                  @click="clearSelection"
                  class="flex-1 py-2 px-3 text-sm font-medium text-status-error bg-status-error-muted rounded-lg active:opacity-80 transition-opacity"
                >
                  Clear
                </button>
                <button
                  @click="closeDropdown"
                  class="flex-1 py-2 px-3 text-sm font-medium text-primary bg-surface-alt rounded-lg active:opacity-80 transition-opacity"
                  :class="{ 'flex-[2]': !modelValue }"
                >
                  Cancel
                </button>
              </div>
            </div>
          </div>
        </div>
      </Transition>
    </Teleport>
  </div>
</template>

<style scoped>
/* Safe area for iOS devices */
.safe-area-inset-bottom {
  padding-bottom: max(1rem, env(safe-area-inset-bottom));
}

/* Smooth scrolling with overscroll containment */
.overscroll-contain {
  overscroll-behavior: contain;
  -webkit-overflow-scrolling: touch;
}

/* Custom scrollbar for desktop */
@media (min-width: 768px) {
  .overflow-y-auto::-webkit-scrollbar {
    width: 6px;
  }

  .overflow-y-auto::-webkit-scrollbar-track {
    background: transparent;
  }

  .overflow-y-auto::-webkit-scrollbar-thumb {
    background: var(--color-border-default);
    border-radius: 3px;
  }

  .overflow-y-auto::-webkit-scrollbar-thumb:hover {
    background: var(--color-border-strong);
  }
}
</style>
