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
    <!-- Header -->
    <div class="px-4 sm:px-6 py-4 bg-surface-alt border-b border-default">
      <div class="flex items-center justify-between gap-3">
        <div class="flex items-center gap-3">
          <div class="w-8 h-8 bg-purple-500/20 rounded-lg flex items-center justify-center flex-shrink-0">
            <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 text-purple-500" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 21a4 4 0 01-4-4V5a2 2 0 012-2h4a2 2 0 012 2v12a4 4 0 01-4 4zm0 0h12a2 2 0 002-2v-4a2 2 0 00-2-2h-2.343M11 7.343l1.657-1.657a2 2 0 012.828 0l2.829 2.829a2 2 0 010 2.828l-8.486 8.485M7 17h.01" />
            </svg>
          </div>
          <div>
            <h2 class="text-base sm:text-lg font-semibold text-primary">Appearance</h2>
            <p class="text-xs text-secondary hidden sm:block">Customize how Nosdesk looks</p>
          </div>
        </div>
        <div v-if="isUpdating || themeStore.isSyncing" class="flex items-center gap-2 text-blue-500">
          <div class="animate-spin h-4 w-4 border-2 border-blue-500 border-t-transparent rounded-full"></div>
        </div>
      </div>
    </div>

    <div class="p-4 sm:p-6 flex flex-col gap-6">
      <!-- Theme Selection -->
      <div class="flex flex-col gap-4">
        <div class="flex items-center justify-between">
          <div>
            <h3 class="text-sm font-medium text-primary">Theme</h3>
            <p class="text-xs text-tertiary mt-0.5">Choose your preferred color scheme</p>
          </div>
        </div>

        <div class="grid grid-cols-3 gap-3">
          <!-- System Theme -->
          <button
            @click="selectTheme('system')"
            :disabled="isUpdating || themeStore.isSyncing"
            class="group p-3 rounded-xl border-2 transition-all text-center"
            :class="[
              selectedTheme === 'system'
                ? 'border-blue-500 bg-blue-500/10'
                : 'border-transparent bg-surface-alt hover:bg-surface-hover hover:border-default',
              (isUpdating || themeStore.isSyncing) && 'opacity-50 cursor-not-allowed'
            ]"
          >
            <div class="flex flex-col items-center gap-2">
              <!-- Theme preview -->
              <div class="w-full aspect-[4/3] rounded-lg overflow-hidden border border-default bg-gradient-to-br from-slate-100 to-slate-800 flex">
                <div class="w-1/2 bg-slate-100 flex items-center justify-center">
                  <div class="w-4 h-4 sm:w-6 sm:h-6 rounded bg-white border border-slate-200"></div>
                </div>
                <div class="w-1/2 bg-slate-800 flex items-center justify-center">
                  <div class="w-4 h-4 sm:w-6 sm:h-6 rounded bg-slate-700 border border-slate-600"></div>
                </div>
              </div>
              <div class="flex items-center gap-1.5">
                <svg
                  v-if="selectedTheme === 'system'"
                  xmlns="http://www.w3.org/2000/svg"
                  class="h-4 w-4 text-blue-500"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                >
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" />
                </svg>
                <span class="text-sm font-medium" :class="selectedTheme === 'system' ? 'text-blue-500' : 'text-secondary'">System</span>
              </div>
            </div>
          </button>

          <!-- Light Theme -->
          <button
            @click="selectTheme('light')"
            :disabled="isUpdating || themeStore.isSyncing"
            class="group p-3 rounded-xl border-2 transition-all text-center"
            :class="[
              selectedTheme === 'light'
                ? 'border-blue-500 bg-blue-500/10'
                : 'border-transparent bg-surface-alt hover:bg-surface-hover hover:border-default',
              (isUpdating || themeStore.isSyncing) && 'opacity-50 cursor-not-allowed'
            ]"
          >
            <div class="flex flex-col items-center gap-2">
              <!-- Theme preview -->
              <div class="w-full aspect-[4/3] rounded-lg overflow-hidden border border-slate-200 bg-slate-100 flex items-center justify-center">
                <div class="w-8 h-6 sm:w-12 sm:h-8 rounded bg-white border border-slate-200 shadow-sm"></div>
              </div>
              <div class="flex items-center gap-1.5">
                <svg
                  v-if="selectedTheme === 'light'"
                  xmlns="http://www.w3.org/2000/svg"
                  class="h-4 w-4 text-blue-500"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                >
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" />
                </svg>
                <span class="text-sm font-medium" :class="selectedTheme === 'light' ? 'text-blue-500' : 'text-secondary'">Light</span>
              </div>
            </div>
          </button>

          <!-- Dark Theme -->
          <button
            @click="selectTheme('dark')"
            :disabled="isUpdating || themeStore.isSyncing"
            class="group p-3 rounded-xl border-2 transition-all text-center"
            :class="[
              selectedTheme === 'dark'
                ? 'border-blue-500 bg-blue-500/10'
                : 'border-transparent bg-surface-alt hover:bg-surface-hover hover:border-default',
              (isUpdating || themeStore.isSyncing) && 'opacity-50 cursor-not-allowed'
            ]"
          >
            <div class="flex flex-col items-center gap-2">
              <!-- Theme preview -->
              <div class="w-full aspect-[4/3] rounded-lg overflow-hidden border border-slate-700 bg-slate-800 flex items-center justify-center">
                <div class="w-8 h-6 sm:w-12 sm:h-8 rounded bg-slate-700 border border-slate-600"></div>
              </div>
              <div class="flex items-center gap-1.5">
                <svg
                  v-if="selectedTheme === 'dark'"
                  xmlns="http://www.w3.org/2000/svg"
                  class="h-4 w-4 text-blue-500"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                >
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" />
                </svg>
                <span class="text-sm font-medium" :class="selectedTheme === 'dark' ? 'text-blue-500' : 'text-secondary'">Dark</span>
              </div>
            </div>
          </button>
        </div>
      </div>

      <!-- Display Options -->
      <div class="flex flex-col gap-4 pt-2 border-t border-default">
        <div>
          <h3 class="text-sm font-medium text-primary">Display</h3>
          <p class="text-xs text-tertiary mt-0.5">Adjust layout preferences</p>
        </div>

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
