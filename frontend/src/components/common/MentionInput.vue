<script setup lang="ts">
/**
 * MentionInput - Textarea with @mention autocomplete
 *
 * Features:
 * - Detects @ trigger and shows user dropdown
 * - Searches users as you type after @
 * - Inserts mention in format @[Name](uuid)
 * - Keyboard navigation (arrow keys, enter, escape)
 * - Click outside to close
 */
import { ref, computed, watch, nextTick, onMounted, onUnmounted } from 'vue';
import { useDataStore } from '@/stores/dataStore';
import UserAvatar from '@/components/UserAvatar.vue';

const props = withDefaults(defineProps<{
  modelValue: string;
  placeholder?: string;
  rows?: number;
  disabled?: boolean;
  maxLength?: number;
}>(), {
  placeholder: 'Type your message...',
  rows: 3,
  disabled: false,
  maxLength: 10000,
});

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void;
  (e: 'submit'): void;
}>();

const dataStore = useDataStore();

// Refs
const textareaRef = ref<HTMLTextAreaElement | null>(null);
const dropdownRef = ref<HTMLElement | null>(null);
const containerRef = ref<HTMLElement | null>(null);

// State
const showMentionDropdown = ref(false);
const mentionQuery = ref('');
const mentionStartPos = ref(0);
const selectedIndex = ref(0);
const isSearching = ref(false);

interface User {
  uuid: string;
  name: string;
  email?: string;
  role?: string;
  avatar_url?: string | null;
  avatar_thumb?: string | null;
}

const users = ref<User[]>([]);

// Debounce timer
let searchTimer: ReturnType<typeof setTimeout> | null = null;

// Search users
const searchUsers = async (query: string) => {
  isSearching.value = true;
  try {
    const result = await dataStore.getPaginatedUsers({
      page: 1,
      pageSize: 10,
      search: query || undefined,
    });
    users.value = result.data;
  } catch (error) {
    console.error('Failed to search users:', error);
    users.value = [];
  } finally {
    isSearching.value = false;
  }
};

// Debounced search
const debouncedSearch = (query: string) => {
  if (searchTimer) {
    clearTimeout(searchTimer);
  }
  searchTimer = setTimeout(() => {
    searchUsers(query);
  }, 200);
};

// Watch mention query
watch(mentionQuery, (query) => {
  if (showMentionDropdown.value) {
    selectedIndex.value = 0;
    debouncedSearch(query);
  }
});

// Handle input
const handleInput = (event: Event) => {
  const target = event.target as HTMLTextAreaElement;
  const value = target.value;
  const cursorPos = target.selectionStart || 0;

  emit('update:modelValue', value);

  // Check for @ trigger
  const textBeforeCursor = value.slice(0, cursorPos);
  const lastAtIndex = textBeforeCursor.lastIndexOf('@');

  if (lastAtIndex !== -1) {
    // Check if @ is at start or preceded by whitespace
    const charBefore = lastAtIndex > 0 ? textBeforeCursor[lastAtIndex - 1] : ' ';
    if (charBefore === ' ' || charBefore === '\n' || lastAtIndex === 0) {
      const queryText = textBeforeCursor.slice(lastAtIndex + 1);
      // Only show dropdown if query doesn't contain spaces (single word)
      if (!queryText.includes(' ') && queryText.length <= 50) {
        mentionQuery.value = queryText;
        mentionStartPos.value = lastAtIndex;
        showMentionDropdown.value = true;
        return;
      }
    }
  }

  // Close dropdown if no valid @ context
  showMentionDropdown.value = false;
};

// Handle keydown
const handleKeyDown = (event: KeyboardEvent) => {
  if (!showMentionDropdown.value) {
    // Submit on Ctrl/Cmd + Enter
    if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      emit('submit');
    }
    return;
  }

  switch (event.key) {
    case 'ArrowDown':
      event.preventDefault();
      selectedIndex.value = Math.min(selectedIndex.value + 1, users.value.length - 1);
      scrollToSelected();
      break;
    case 'ArrowUp':
      event.preventDefault();
      selectedIndex.value = Math.max(selectedIndex.value - 1, 0);
      scrollToSelected();
      break;
    case 'Enter':
    case 'Tab':
      if (users.value.length > 0) {
        event.preventDefault();
        selectUser(users.value[selectedIndex.value]);
      }
      break;
    case 'Escape':
      event.preventDefault();
      showMentionDropdown.value = false;
      break;
  }
};

// Scroll selected item into view
const scrollToSelected = () => {
  nextTick(() => {
    const dropdown = dropdownRef.value;
    if (!dropdown) return;

    const selected = dropdown.querySelector('.selected') as HTMLElement;
    if (selected) {
      selected.scrollIntoView({ block: 'nearest' });
    }
  });
};

// Select user and insert mention
const selectUser = (user: User) => {
  if (!textareaRef.value) return;

  const value = props.modelValue;
  const cursorPos = textareaRef.value.selectionStart || 0;

  // Find the @ that triggered this mention
  const textBeforeCursor = value.slice(0, cursorPos);
  const lastAtIndex = textBeforeCursor.lastIndexOf('@');

  if (lastAtIndex === -1) return;

  // Build the mention text: @[Name](uuid)
  const mentionText = `@[${user.name}](${user.uuid}) `;

  // Replace @ and query with the mention
  const before = value.slice(0, lastAtIndex);
  const after = value.slice(cursorPos);
  const newValue = before + mentionText + after;

  emit('update:modelValue', newValue);
  showMentionDropdown.value = false;

  // Set cursor position after the mention
  nextTick(() => {
    if (textareaRef.value) {
      const newCursorPos = lastAtIndex + mentionText.length;
      textareaRef.value.focus();
      textareaRef.value.setSelectionRange(newCursorPos, newCursorPos);
    }
  });
};

// Handle click outside
const handleClickOutside = (event: MouseEvent) => {
  if (
    containerRef.value &&
    !containerRef.value.contains(event.target as Node)
  ) {
    showMentionDropdown.value = false;
  }
};

// Dropdown position
const dropdownStyle = computed(() => {
  if (!textareaRef.value || !showMentionDropdown.value) return {};

  // Position dropdown below the textarea
  return {
    top: '100%',
    left: '0',
    right: '0',
  };
});

// Focus the textarea
const focus = () => {
  textareaRef.value?.focus();
};

// Clear the input
const clear = () => {
  emit('update:modelValue', '');
  showMentionDropdown.value = false;
};

// Lifecycle
onMounted(() => {
  document.addEventListener('click', handleClickOutside);
  // Initial load of users for quick suggestions
  searchUsers('');
});

onUnmounted(() => {
  document.removeEventListener('click', handleClickOutside);
  if (searchTimer) {
    clearTimeout(searchTimer);
  }
});

// Expose methods
defineExpose({
  focus,
  clear,
});
</script>

<template>
  <div ref="containerRef">
    <!-- Textarea wrapper for dropdown positioning -->
    <div class="relative">
      <textarea
        ref="textareaRef"
        :value="modelValue"
        :placeholder="placeholder"
        :rows="rows"
        :disabled="disabled"
        :maxlength="maxLength"
        @input="handleInput"
        @keydown="handleKeyDown"
        class="w-full px-3 py-2 text-sm text-primary bg-surface border border-default rounded-lg resize-none focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent disabled:opacity-50 disabled:cursor-not-allowed placeholder:text-tertiary"
      />

      <!-- Mention Dropdown -->
      <Transition
      enter-active-class="transition ease-out duration-100"
      enter-from-class="transform opacity-0 scale-95"
      enter-to-class="transform opacity-100 scale-100"
      leave-active-class="transition ease-in duration-75"
      leave-from-class="transform opacity-100 scale-100"
      leave-to-class="transform opacity-0 scale-95"
    >
      <div
        v-if="showMentionDropdown"
        ref="dropdownRef"
        class="absolute z-50 mt-1 bg-surface border border-default rounded-lg shadow-lg overflow-hidden"
        :style="dropdownStyle"
      >
        <!-- Search indicator -->
        <div v-if="mentionQuery" class="px-3 py-2 text-xs text-tertiary border-b border-default bg-surface-alt">
          Searching for "<span class="text-primary font-medium">{{ mentionQuery }}</span>"
        </div>

        <!-- Loading -->
        <div v-if="isSearching" class="px-3 py-4 flex items-center justify-center">
          <div class="animate-spin rounded-full h-4 w-4 border-2 border-accent border-t-transparent"></div>
        </div>

        <!-- User list -->
        <div v-else-if="users.length > 0" class="max-h-48 overflow-y-auto">
          <button
            v-for="(user, index) in users"
            :key="user.uuid"
            @click="selectUser(user)"
            @mouseenter="selectedIndex = index"
            class="w-full px-3 py-2 flex items-center gap-3 text-left hover:bg-surface-alt transition-colors"
            :class="{ 'bg-surface-alt selected': index === selectedIndex }"
          >
            <UserAvatar
              :name="user.name"
              :avatar="user.avatar_thumb || user.avatar_url"
              size="sm"
              :showName="false"
            />
            <div class="flex-1 min-w-0">
              <p class="text-sm font-medium text-primary truncate">{{ user.name }}</p>
              <p v-if="user.email" class="text-xs text-tertiary truncate">{{ user.email }}</p>
            </div>
            <span
              v-if="user.role"
              class="text-xs px-2 py-0.5 rounded-full bg-surface-alt text-secondary"
            >
              {{ user.role }}
            </span>
          </button>
        </div>

        <!-- No results -->
        <div v-else class="px-3 py-4 text-center text-sm text-tertiary">
          No users found
        </div>

        <!-- Hint -->
        <div class="px-3 py-2 text-xs text-tertiary border-t border-default bg-surface-alt flex items-center gap-4">
          <span><kbd class="px-1 py-0.5 bg-surface rounded text-xs">↑↓</kbd> Navigate</span>
          <span><kbd class="px-1 py-0.5 bg-surface rounded text-xs">Enter</kbd> Select</span>
          <span><kbd class="px-1 py-0.5 bg-surface rounded text-xs">Esc</kbd> Close</span>
        </div>
      </div>
    </Transition>
    </div>

    <!-- Helper text -->
    <div class="mt-1 flex items-center justify-between text-xs text-tertiary">
      <span>
        <span class="hidden sm:inline">Type </span><kbd class="px-1 py-0.5 bg-surface-alt rounded">@</kbd> to mention someone
        <span class="hidden sm:inline"> · Supports **markdown**</span>
      </span>
      <span v-if="maxLength">{{ modelValue.length }}/{{ maxLength }}</span>
    </div>
  </div>
</template>
