<script setup lang="ts">
import { ref } from 'vue';
import ToggleSwitch from '@/components/common/ToggleSwitch.vue';

// Notification preferences
const emailNotifications = ref(true);
const desktopNotifications = ref(false);

// Emits for notifications
const emit = defineEmits<{
  (e: 'success', message: string): void;
  (e: 'error', message: string): void;
}>();

// Handle notification toggles
const handleEmailNotificationToggle = () => {
  // TODO: Implement save to backend/store
  emit('success', `Email notifications ${emailNotifications.value ? 'enabled' : 'disabled'}`);
};

const handleDesktopNotificationToggle = () => {
  // TODO: Implement save to backend/store and request browser permission if enabling
  if (desktopNotifications.value && 'Notification' in window) {
    Notification.requestPermission().then((permission) => {
      if (permission === 'granted') {
        emit('success', 'Desktop notifications enabled');
      } else {
        desktopNotifications.value = false;
        emit('error', 'Desktop notification permission denied');
      }
    });
  } else {
    emit('success', `Desktop notifications ${desktopNotifications.value ? 'enabled' : 'disabled'}`);
  }
};
</script>

<template>
  <div class="bg-surface rounded-xl border border-default hover:border-strong transition-colors overflow-hidden">
    <div class="px-4 py-3 bg-surface-alt border-b border-default">
      <h2 class="text-lg font-medium text-primary">Notifications</h2>
      <p class="text-sm text-tertiary mt-1">Configure how you'd like to be notified about updates</p>
    </div>
    
    <div class="p-6 flex flex-col gap-3">
      <!-- Email Notifications -->
      <ToggleSwitch
        v-model="emailNotifications"
        label="Email notifications"
        description="Get notified about important updates via email"
        @update:modelValue="handleEmailNotificationToggle"
      />

      <!-- Desktop Notifications -->
      <ToggleSwitch
        v-model="desktopNotifications"
        label="Desktop notifications"
        description="Show notifications on your desktop when the app is closed"
        @update:modelValue="handleDesktopNotificationToggle"
      />
    </div>
  </div>
</template> 