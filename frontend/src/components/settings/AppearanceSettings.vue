<script setup lang="ts">
import { ref, watch } from 'vue';
// @ts-ignore
import { useThemeStore } from '@/stores/theme';
import ToggleSwitch from '@/components/common/ToggleSwitch.vue';

const themeStore = useThemeStore();

// Local reactive state
const darkMode = ref(themeStore.isDarkMode);
const compactView = ref(false); // TODO: This should be connected to a store if needed

// Emits for notifications
const emit = defineEmits<{
  (e: 'success', message: string): void;
  (e: 'error', message: string): void;
}>();

// Watch theme store changes
watch(() => themeStore.isDarkMode, (newValue) => {
  darkMode.value = newValue;
});

// Watch local dark mode changes and update store
watch(darkMode, (newValue) => {
  if (newValue !== themeStore.isDarkMode) {
    themeStore.toggleTheme();
    emit('success', `Switched to ${newValue ? 'dark' : 'light'} mode`);
  }
});

// Handle compact view toggle
const handleCompactViewToggle = () => {
  // TODO: Implement compact view logic
  emit('success', `Compact view ${compactView.value ? 'enabled' : 'disabled'}`);
};
</script>

<template>
  <div class="bg-slate-800 rounded-xl border border-slate-700/50 hover:border-slate-600/50 transition-colors">
    <div class="px-4 py-3 bg-slate-700/30 border-b border-slate-700/50">
      <h2 class="text-lg font-medium text-white">Appearance</h2>
      <p class="text-sm text-slate-400 mt-1">Customize how Nosdesk looks and feels for you</p>
    </div>
    
    <div class="p-6 flex flex-col gap-6">
      <!-- Theme Selection -->
      <div class="flex flex-col gap-4">
        <h3 class="text-sm font-medium text-white">Theme</h3>
        <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
          <!-- Light Theme -->
          <button
            @click="darkMode = false"
            :class="[
              'p-4 rounded-lg border-2 transition-all duration-200 min-h-[100px] active:scale-[0.98]',
              !darkMode
                ? 'border-blue-500 bg-blue-500/10'
                : 'border-slate-600 hover:border-slate-500'
            ]"
          >
            <div class="flex flex-col gap-2">
              <div class="w-full h-12 bg-white rounded border flex items-center justify-center">
                <div class="w-8 h-8 bg-gray-200 rounded"></div>
              </div>
              <div class="text-sm font-medium text-center">
                <span :class="!darkMode ? 'text-blue-400' : 'text-slate-300'">Light</span>
              </div>
            </div>
          </button>

          <!-- Dark Theme -->
          <button
            @click="darkMode = true"
            :class="[
              'p-4 rounded-lg border-2 transition-all duration-200 min-h-[100px] active:scale-[0.98]',
              darkMode
                ? 'border-blue-500 bg-blue-500/10'
                : 'border-slate-600 hover:border-slate-500'
            ]"
          >
            <div class="flex flex-col gap-2">
              <div class="w-full h-12 bg-slate-800 rounded border border-slate-600 flex items-center justify-center">
                <div class="w-8 h-8 bg-slate-600 rounded"></div>
              </div>
              <div class="text-sm font-medium text-center">
                <span :class="darkMode ? 'text-blue-400' : 'text-slate-300'">Dark</span>
              </div>
            </div>
          </button>
        </div>
      </div>

      <!-- Display Options -->
      <div class="flex flex-col gap-4">
        <h3 class="text-sm font-medium text-white">Display</h3>
        
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