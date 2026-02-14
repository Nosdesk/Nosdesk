<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';

const props = defineProps<{
  pageId: number | string;
  pageTitle: string;
  pageSlug?: string;
  pageStatus?: string;
  showPermissions?: boolean;
}>();

const emit = defineEmits<{
  (e: 'delete'): void;
  (e: 'duplicate'): void;
  (e: 'archive'): void;
  (e: 'restore'): void;
  (e: 'publish'): void;
  (e: 'unpublish'): void;
  (e: 'move'): void;
  (e: 'export'): void;
  (e: 'collections'): void;
  (e: 'permissions'): void;
}>();

const isOpen = ref(false);
const menuRef = ref<HTMLElement | null>(null);
const buttonRef = ref<HTMLElement | null>(null);
const confirmingDelete = ref(false);
const copiedFeedback = ref(false);

const isArchived = computed(() => props.pageStatus === 'archived');
const isPublished = computed(() => props.pageStatus === 'published');

const toggle = () => {
  if (isOpen.value) {
    close();
  } else {
    isOpen.value = true;
    confirmingDelete.value = false;
  }
};

const close = () => {
  isOpen.value = false;
  confirmingDelete.value = false;
};

const handleClickOutside = (event: MouseEvent) => {
  const target = event.target as Node;
  const clickedOutsideMenu = menuRef.value && !menuRef.value.contains(target);
  const clickedOutsideButton = buttonRef.value && !buttonRef.value.contains(target);
  if (clickedOutsideMenu && clickedOutsideButton) {
    close();
  }
};

const handleKeydown = (event: KeyboardEvent) => {
  if (event.key === 'Escape') {
    close();
  }
};

const handleCopyLink = async () => {
  const url = `${window.location.origin}/documentation/${props.pageSlug || props.pageId}`;
  try {
    await navigator.clipboard.writeText(url);
    copiedFeedback.value = true;
    setTimeout(() => {
      copiedFeedback.value = false;
    }, 2000);
  } catch {
    const textArea = document.createElement('textarea');
    textArea.value = url;
    document.body.appendChild(textArea);
    textArea.select();
    document.execCommand('copy');
    document.body.removeChild(textArea);
    copiedFeedback.value = true;
    setTimeout(() => {
      copiedFeedback.value = false;
    }, 2000);
  }
  close();
};

const handlePrint = () => {
  close();
  window.print();
};

const handleDuplicate = () => {
  close();
  emit('duplicate');
};

const handleArchive = () => {
  close();
  if (isArchived.value) {
    emit('restore');
  } else {
    emit('archive');
  }
};

const handlePublishToggle = () => {
  close();
  if (isPublished.value) {
    emit('unpublish');
  } else {
    emit('publish');
  }
};

const handleMove = () => {
  close();
  emit('move');
};

const handleExport = () => {
  close();
  emit('export');
};

const handleCollections = () => {
  close();
  emit('collections');
};

const handlePermissions = () => {
  close();
  emit('permissions');
};

const handleDelete = () => {
  if (!confirmingDelete.value) {
    confirmingDelete.value = true;
    return;
  }
  close();
  emit('delete');
};

onMounted(() => {
  document.addEventListener('mousedown', handleClickOutside);
  document.addEventListener('keydown', handleKeydown);
});

onUnmounted(() => {
  document.removeEventListener('mousedown', handleClickOutside);
  document.removeEventListener('keydown', handleKeydown);
});
</script>

<template>
  <div class="relative">
    <!-- Three-dot trigger button -->
    <button
      ref="buttonRef"
      @click="toggle"
      class="p-1.5 rounded-md hover:bg-surface-hover transition-colors text-secondary hover:text-primary"
      :class="{ 'bg-surface-hover text-primary': isOpen }"
      title="Page actions"
      aria-label="Page actions"
    >
      <svg class="w-5 h-5" fill="currentColor" viewBox="0 0 20 20">
        <path d="M10 6a2 2 0 110-4 2 2 0 010 4zM10 12a2 2 0 110-4 2 2 0 010 4zM10 18a2 2 0 110-4 2 2 0 010 4z" />
      </svg>
    </button>

    <!-- Dropdown menu -->
    <div
      v-if="isOpen"
      ref="menuRef"
      role="menu"
      tabindex="-1"
      class="absolute right-0 mt-1 w-52 bg-surface border border-default rounded-lg shadow-lg py-1 z-50"
    >
      <!-- Group 1: Future features -->
      <button
        class="w-full px-3 py-2 text-sm text-left flex items-center gap-2.5 text-tertiary cursor-not-allowed"
        disabled
      >
        <svg class="w-4 h-4 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M11.049 2.927c.3-.921 1.603-.921 1.902 0l1.519 4.674a1 1 0 00.95.69h4.915c.969 0 1.371 1.24.588 1.81l-3.976 2.888a1 1 0 00-.363 1.118l1.518 4.674c.3.922-.755 1.688-1.538 1.118l-3.976-2.888a1 1 0 00-1.176 0l-3.976 2.888c-.783.57-1.838-.197-1.538-1.118l1.518-4.674a1 1 0 00-.363-1.118l-3.976-2.888c-.784-.57-.38-1.81.588-1.81h4.914a1 1 0 00.951-.69l1.519-4.674z" />
        </svg>
        <span class="flex-1">Star page</span>
        <span class="text-[10px] px-1.5 py-0.5 rounded bg-surface-alt text-tertiary">Soon</span>
      </button>
      <button
        class="w-full px-3 py-2 text-sm text-left flex items-center gap-2.5 text-tertiary cursor-not-allowed"
        disabled
      >
        <svg class="w-4 h-4 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 17h5l-1.405-1.405A2.032 2.032 0 0118 14.158V11a6.002 6.002 0 00-4-5.659V5a2 2 0 10-4 0v.341C7.67 6.165 6 8.388 6 11v3.159c0 .538-.214 1.055-.595 1.436L4 17h5m6 0v1a3 3 0 11-6 0v-1m6 0H9" />
        </svg>
        <span class="flex-1">Subscribe</span>
        <span class="text-[10px] px-1.5 py-0.5 rounded bg-surface-alt text-tertiary">Soon</span>
      </button>

      <!-- Divider -->
      <div class="my-1 border-t border-subtle"></div>

      <!-- Group 2: Quick actions -->
      <button
        @click="handleCopyLink"
        class="w-full px-3 py-2 text-sm text-left flex items-center gap-2.5 text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
      >
        <svg class="w-4 h-4 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13.828 10.172a4 4 0 00-5.656 0l-4 4a4 4 0 105.656 5.656l1.102-1.101m-.758-4.899a4 4 0 005.656 0l4-4a4 4 0 00-5.656-5.656l-1.1 1.1" />
        </svg>
        <span class="flex-1">{{ copiedFeedback ? 'Copied!' : 'Copy link' }}</span>
      </button>
      <button
        @click="handlePrint"
        class="w-full px-3 py-2 text-sm text-left flex items-center gap-2.5 text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
      >
        <svg class="w-4 h-4 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M17 17h2a2 2 0 002-2v-4a2 2 0 00-2-2H5a2 2 0 00-2 2v4a2 2 0 002 2h2m2 4h6a2 2 0 002-2v-4a2 2 0 00-2-2H9a2 2 0 00-2 2v4a2 2 0 002 2zm8-12V5a2 2 0 00-2-2H9a2 2 0 00-2 2v4h10z" />
        </svg>
        <span class="flex-1">Print</span>
      </button>
      <button
        @click="handleDuplicate"
        class="w-full px-3 py-2 text-sm text-left flex items-center gap-2.5 text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
      >
        <svg class="w-4 h-4 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" />
        </svg>
        <span class="flex-1">Duplicate</span>
      </button>
      <button
        @click="handleExport"
        class="w-full px-3 py-2 text-sm text-left flex items-center gap-2.5 text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
      >
        <svg class="w-4 h-4 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 10v6m0 0l-3-3m3 3l3-3m2 8H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
        </svg>
        <span class="flex-1">Export Markdown</span>
      </button>
      <button
        @click="handleMove"
        class="w-full px-3 py-2 text-sm text-left flex items-center gap-2.5 text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
      >
        <svg class="w-4 h-4 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 16V4m0 0L3 8m4-4l4 4m6 0v12m0 0l4-4m-4 4l-4-4" />
        </svg>
        <span class="flex-1">Move to...</span>
      </button>

      <!-- Divider -->
      <div class="my-1 border-t border-subtle"></div>

      <!-- Group 3: Management features -->
      <button
        @click="handleCollections"
        class="w-full px-3 py-2 text-sm text-left flex items-center gap-2.5 text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
      >
        <svg class="w-4 h-4 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 7h.01M7 3h5c.512 0 1.024.195 1.414.586l7 7a2 2 0 010 2.828l-7 7a2 2 0 01-2.828 0l-7-7A2 2 0 013 12V7a4 4 0 014-4z" />
        </svg>
        <span class="flex-1">Collections</span>
      </button>
      <button
        @click="handleArchive"
        class="w-full px-3 py-2 text-sm text-left flex items-center gap-2.5 text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
      >
        <svg class="w-4 h-4 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 8h14M5 8a2 2 0 110-4h14a2 2 0 110 4M5 8v10a2 2 0 002 2h10a2 2 0 002-2V8m-9 4h4" />
        </svg>
        <span class="flex-1">{{ isArchived ? 'Unarchive' : 'Archive' }}</span>
      </button>
      <button
        v-if="showPermissions"
        @click="handlePermissions"
        class="w-full px-3 py-2 text-sm text-left flex items-center gap-2.5 text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
      >
        <svg class="w-4 h-4 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />
        </svg>
        <span class="flex-1">Permissions</span>
      </button>
      <button
        @click="handlePublishToggle"
        class="w-full px-3 py-2 text-sm text-left flex items-center gap-2.5 text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
      >
        <svg class="w-4 h-4 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
        </svg>
        <span class="flex-1">{{ isPublished ? 'Unpublish' : 'Publish' }}</span>
      </button>

      <!-- Divider -->
      <div class="my-1 border-t border-subtle"></div>

      <!-- Group 4: Destructive -->
      <button
        @click="handleDelete"
        class="w-full px-3 py-2 text-sm text-left flex items-center gap-2.5 transition-colors"
        :class="confirmingDelete
          ? 'text-status-error bg-status-error/10 hover:bg-status-error/20 font-medium'
          : 'text-status-error hover:bg-surface-hover'"
      >
        <svg class="w-4 h-4 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
        </svg>
        <span class="flex-1">{{ confirmingDelete ? 'Confirm trash?' : 'Move to Trash' }}</span>
      </button>
    </div>
  </div>
</template>
