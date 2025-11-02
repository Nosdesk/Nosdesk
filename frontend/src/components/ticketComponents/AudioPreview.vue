<!-- AudioPreview.vue -->
<script setup lang="ts">
import UserAvatar from "@/components/UserAvatar.vue";
import AudioPlayer from "@/components/ticketComponents/AudioPlayer.vue";
import { computed } from 'vue';

interface Props {
  blob: Blob;
  author: string;
  timestamp: string;
  showRecordingControls?: boolean;
}

const props = withDefaults(defineProps<Props>(), {
  showRecordingControls: false
});

const emit = defineEmits<{
  (e: 'confirm'): void;
  (e: 'reRecord'): void;
  (e: 'cancel'): void;
  (e: 'submit', data: { blob: Blob; name: string }): void;
}>();

const urlCreator = window.URL || window.webkitURL;

// Generate a more user-friendly name for the voice note
const voiceNoteName = computed(() => {
  const now = new Date();
  const formattedDate = now.toLocaleDateString('en-US', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit'
  });
  return `Voice Note ${formattedDate}`;
});

const handleConfirm = () => {
  emit('submit', {
    blob: props.blob,
    name: `${voiceNoteName.value}.webm`
  });
  emit('confirm');
};
</script>

<template>
  <div class="flex flex-col gap-2 bg-surface-alt rounded-lg p-3 w-full">
    <!-- Header with filename and controls -->
    <div class="flex items-center justify-between mb-2">
      <div class="flex items-center gap-2">
        <svg class="w-5 h-5 text-blue-400" viewBox="0 0 20 20" fill="currentColor">
          <path fill-rule="evenodd" d="M7 4a3 3 0 016 0v4a3 3 0 11-6 0V4zm4 10.93A7.001 7.001 0 0017 8a1 1 0 10-2 0A5 5 0 015 8a1 1 0 00-2 0 7.001 7.001 0 006 6.93V17H6a1 1 0 100 2h8a1 1 0 100-2h-3v-2.07z" clip-rule="evenodd" />
        </svg>
        <div class="flex flex-col">
          <span class="text-sm text-primary">{{ voiceNoteName }}</span>
          <span class="text-xs text-tertiary">{{ timestamp }}</span>
        </div>
      </div>
      <button
        type="button"
        @click="emit('cancel')"
        class="p-1.5 text-tertiary hover:text-primary hover:bg-surface-hover rounded transition-colors"
        title="Delete audio"
      >
        <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
          <path fill-rule="evenodd" d="M9 2a1 1 0 00-.894.553L7.382 4H4a1 1 0 000 2v10a2 2 0 002 2h8a2 2 0 002-2V6a1 1 0 100-2h-3.382l-.724-1.447A1 1 0 0011 2H9zM7 8a1 1 0 012 0v6a1 1 0 11-2 0V8zm5-1a1 1 0 00-1 1v6a1 1 0 102 0V8a1 1 0 00-1-1z" clip-rule="evenodd" />
        </svg>
      </button>
    </div>
    
    <!-- Audio player -->
    <AudioPlayer
      :src="urlCreator.createObjectURL(blob)"
    />
    
    <!-- Action buttons -->
    <div class="flex justify-end gap-2 mt-2">
      <template v-if="showRecordingControls">
        <button
          type="button"
          @click="emit('reRecord')"
          class="px-3 py-1.5 text-secondary hover:text-primary transition-colors"
        >
          Re-record
        </button>
        <button
          type="button"
          @click="handleConfirm"
          class="px-3 py-1.5 bg-green-500 text-white text-sm rounded hover:bg-green-600 transition-colors"
        >
          Confirm
        </button>
      </template>
      <template v-else>
        <button
          type="button"
          @click="handleConfirm"
          class="px-3 py-1.5 bg-green-500 text-white text-sm rounded hover:bg-green-600 transition-colors"
        >
          Confirm
        </button>
      </template>
      <button
        type="button"
        @click="emit('cancel')"
        class="px-3 py-1.5 text-secondary hover:text-primary transition-colors"
      >
        Cancel
      </button>
    </div>
  </div>
</template>