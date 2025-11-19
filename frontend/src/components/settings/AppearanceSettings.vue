<script setup lang="ts">
import { ref, watch, computed } from 'vue';
import { useThemeStore, type Theme } from '@/stores/theme';
import { useAuthStore } from '@/stores/auth';
import ToggleSwitch from '@/components/common/ToggleSwitch.vue';

const themeStore = useThemeStore();
const authStore = useAuthStore();

// Props
const props = defineProps<{
  targetUserUuid?: string;
}>();

// Emits for notifications
const emit = defineEmits<{
  (e: 'success', message: string): void;
  (e: 'error', message: string): void;
}>();

// Local reactive state
const selectedTheme = ref<Theme>(themeStore.currentTheme);
const compactView = ref(false);
const isUpdating = ref(false);

// Watch theme store changes
watch(() => themeStore.currentTheme, (newValue) => {
  selectedTheme.value = newValue;
});

// Get the UUID to update (target user or current user)
const userUuid = computed(() => {
  return props.targetUserUuid || authStore.user?.uuid;
});

// Handle theme selection
const selectTheme = async (theme: string) => {
  if (selectedTheme.value === theme) return;

  isUpdating.value = true;
  selectedTheme.value = theme;

  // Update local theme
  themeStore.setTheme(theme);

  // Sync to backend
  if (userUuid.value) {
    const success = await themeStore.syncThemeToBackend(userUuid.value);

    if (success) {
      const themeName = theme === 'system' ? 'System' : (theme === 'light' ? 'Light' : 'Dark');
      emit('success', `Theme changed to ${themeName}`);
    } else {
      emit('error', 'Failed to save theme preference');
      // Revert to previous theme on failure
      const previousTheme = themeStore.currentTheme;
      selectedTheme.value = previousTheme;
      themeStore.setTheme(previousTheme);
    }
  }

  isUpdating.value = false;
};

// Handle compact view toggle
const handleCompactViewToggle = () => {
  emit('success', `Compact view ${compactView.value ? 'enabled' : 'disabled'}`);
};
</script>

<template>
  <div class="bg-surface rounded-xl border border-default hover:border-strong transition-colors overflow-hidden">
    <div class="px-4 py-3 bg-surface-alt border-b border-default">
      <h2 class="text-lg font-medium text-primary">Appearance</h2>
      <p class="text-sm text-tertiary mt-1">Customize how Nosdesk looks and feels for you</p>
    </div>

    <div class="p-6 flex flex-col gap-6">
      <!-- Theme Selection -->
      <div class="flex flex-col gap-4">
        <div class="flex items-center justify-between">
          <h3 class="text-sm font-medium text-primary">Theme</h3>
          <div v-if="isUpdating || themeStore.isSyncing" class="flex items-center gap-2 text-blue-400">
            <div class="animate-spin h-3 w-3 border border-blue-400 border-t-transparent rounded-full"></div>
            <span class="text-xs">Saving...</span>
          </div>
        </div>

        <div class="grid grid-cols-1 sm:grid-cols-3 gap-3">
          <!-- System Theme -->
          <button
            @click="selectTheme('system')"
            :disabled="isUpdating || themeStore.isSyncing"
            :class="[
              'p-4 rounded-lg border-2 transition-all duration-200 min-h-[100px] active:scale-[0.98]',
              selectedTheme === 'system'
                ? 'border-blue-500 bg-blue-500/10'
                : 'border-subtle hover:border-default',
              (isUpdating || themeStore.isSyncing) && 'opacity-50 cursor-not-allowed'
            ]"
          >
            <div class="flex flex-col gap-2">
              <div class="w-full h-12 bg-gradient-to-r from-surface-alt via-border-default to-surface rounded border flex items-center justify-center">
                <div class="w-8 h-8 bg-gradient-to-br from-surface-alt to-surface rounded"></div>
              </div>
              <div class="text-sm font-medium text-center">
                <span :class="selectedTheme === 'system' ? 'text-blue-400' : 'text-secondary'">System</span>
              </div>
            </div>
          </button>

          <!-- Light Theme -->
          <button
            @click="selectTheme('light')"
            :disabled="isUpdating || themeStore.isSyncing"
            :class="[
              'p-4 rounded-lg border-2 transition-all duration-200 min-h-[100px] active:scale-[0.98]',
              selectedTheme === 'light'
                ? 'border-blue-500 bg-blue-500/10'
                : 'border-subtle hover:border-default',
              (isUpdating || themeStore.isSyncing) && 'opacity-50 cursor-not-allowed'
            ]"
          >
            <div class="flex flex-col gap-2">
              <div class="w-full h-12 bg-surface-alt rounded border flex items-center justify-center">
                <div class="w-8 h-8 bg-surface rounded"></div>
              </div>
              <div class="text-sm font-medium text-center">
                <span :class="selectedTheme === 'light' ? 'text-blue-400' : 'text-secondary'">Light</span>
              </div>
            </div>
          </button>

          <!-- Dark Theme -->
          <button
            @click="selectTheme('dark')"
            :disabled="isUpdating || themeStore.isSyncing"
            :class="[
              'p-4 rounded-lg border-2 transition-all duration-200 min-h-[100px] active:scale-[0.98]',
              selectedTheme === 'dark'
                ? 'border-blue-500 bg-blue-500/10'
                : 'border-subtle hover:border-default',
              (isUpdating || themeStore.isSyncing) && 'opacity-50 cursor-not-allowed'
            ]"
          >
            <div class="flex flex-col gap-2">
              <div class="w-full h-12 bg-surface rounded border border-default flex items-center justify-center">
                <div class="w-8 h-8 bg-surface-hover rounded"></div>
              </div>
              <div class="text-sm font-medium text-center">
                <span :class="selectedTheme === 'dark' ? 'text-blue-400' : 'text-secondary'">Dark</span>
              </div>
            </div>
          </button>
        </div>
      </div>

      <!-- Display Options -->
      <div class="flex flex-col gap-4">
        <h3 class="text-sm font-medium text-primary">Display</h3>

        <!-- Compact View Toggle -->
        <ToggleSwitch
          v-model="compactView"
          label="Compact view"
          description="Reduce spacing between elements for a denser layout"
          @update:modelValue="handleCompactViewToggle"
        />
      </div>
    </div>
  </div>
</template>
