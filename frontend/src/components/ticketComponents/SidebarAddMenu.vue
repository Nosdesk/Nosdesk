<script setup lang="ts">
/**
 * SidebarAddMenu - Unified "+ Add" dropdown for the ticket sidebar
 *
 * Replaces individual empty-state buttons with a single compact menu
 * listing all available actions (native + plugin-contributed).
 */
import { ref, computed, onMounted, onUnmounted } from 'vue';

export interface SidebarAddMenuItem {
  id: string;
  label: string;
  type: 'native' | 'plugin';
  pluginName?: string;
  /** SVG path for native icons (viewBox 0 0 24 24, stroke-based) or image URL/data URI for plugins */
  icon?: string;
}

const props = defineProps<{
  items: SidebarAddMenuItem[];
}>();

const emit = defineEmits<{
  (e: 'select', itemId: string): void;
}>();

const isOpen = ref(false);
const menuRef = ref<HTMLElement | null>(null);
const buttonRef = ref<HTMLElement | null>(null);

const toggle = () => {
  isOpen.value = !isOpen.value;
};

const close = () => {
  isOpen.value = false;
};

const handleSelect = (itemId: string) => {
  emit('select', itemId);
  close();
};

const nativeItems = computed(() => props.items.filter(i => i.type === 'native'));
const pluginItems = computed(() => props.items.filter(i => i.type === 'plugin'));

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
  <div class="relative print:hidden">
    <!-- Trigger button -->
    <button
      ref="buttonRef"
      @click="toggle"
      class="group w-full py-2.5 px-4 rounded-xl border border-dashed border-default hover:border-accent/50 hover:bg-accent/5 transition-all duration-150 cursor-pointer"
    >
      <div class="flex items-center justify-center gap-2 text-sm text-tertiary group-hover:text-accent transition-colors">
        <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
          <path d="M10 3a1 1 0 011 1v5h5a1 1 0 110 2h-5v5a1 1 0 11-2 0v-5H4a1 1 0 110-2h5V4a1 1 0 011-1z" />
        </svg>
        <span>Add</span>
        <svg
          class="w-3.5 h-3.5 transition-transform"
          :class="{ 'rotate-180': isOpen }"
          viewBox="0 0 20 20"
          fill="currentColor"
        >
          <path fill-rule="evenodd" d="M5.293 7.293a1 1 0 011.414 0L10 10.586l3.293-3.293a1 1 0 111.414 1.414l-4 4a1 1 0 01-1.414 0l-4-4a1 1 0 010-1.414z" clip-rule="evenodd" />
        </svg>
      </div>
    </button>

    <!-- Dropdown menu -->
    <div
      v-if="isOpen"
      ref="menuRef"
      role="menu"
      tabindex="-1"
      class="absolute left-0 mt-1 w-56 bg-surface border border-default rounded-lg shadow-lg py-1 z-50"
    >
      <!-- Native items -->
      <button
        v-for="item in nativeItems"
        :key="item.id"
        @click="handleSelect(item.id)"
        role="menuitem"
        class="w-full px-3 py-2 text-sm text-left flex items-center gap-2.5 text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
      >
        <svg v-if="item.icon" class="w-4 h-4 flex-shrink-0 text-tertiary" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" :d="item.icon" />
        </svg>
        {{ item.label }}
      </button>

      <!-- Separator between native and plugin items -->
      <div
        v-if="nativeItems.length > 0 && pluginItems.length > 0"
        class="my-1 border-t border-default"
      />

      <!-- Plugin items -->
      <button
        v-for="item in pluginItems"
        :key="item.id"
        @click="handleSelect(item.id)"
        role="menuitem"
        class="w-full px-3 py-2 text-sm text-left flex items-center gap-2.5 text-secondary hover:text-primary hover:bg-surface-hover transition-colors"
      >
        <!-- Plugin icon: image URL or data URI -->
        <img v-if="item.icon" :src="item.icon" class="w-4 h-4 flex-shrink-0 rounded-sm" alt="" />
        <!-- Fallback: lightning bolt -->
        <svg v-else class="w-4 h-4 flex-shrink-0 text-tertiary" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M13 10V3L4 14h7v7l9-11h-7z" />
        </svg>
        <span>{{ item.label }}</span>
        <span v-if="item.pluginName" class="ml-auto text-xs text-tertiary">{{ item.pluginName }}</span>
      </button>
    </div>
  </div>
</template>
