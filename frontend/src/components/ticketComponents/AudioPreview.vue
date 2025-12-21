<!-- AudioPreview.vue -->
<script setup lang="ts">
import { formatDate, formatDateTime } from '@/utils/dateUtils';
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
  const formattedDate = formatDate(new Date(), "MMM d, yyyy");
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
  <div class="audio-preview w-full">
    <!-- Compact player with inline controls -->
    <div class="flex flex-col gap-2">
      <!-- Audio player -->
      <AudioPlayer :src="urlCreator.createObjectURL(blob)" />

      <!-- Action buttons - compact inline -->
      <div class="flex items-center justify-between">
        <div class="flex items-center gap-1 text-xs text-tertiary">
          <svg class="w-3.5 h-3.5" viewBox="0 0 20 20" fill="currentColor">
            <path fill-rule="evenodd" d="M7 4a3 3 0 016 0v4a3 3 0 11-6 0V4zm4 10.93A7.001 7.001 0 0017 8a1 1 0 10-2 0A5 5 0 015 8a1 1 0 00-2 0 7.001 7.001 0 006 6.93V17H6a1 1 0 100 2h8a1 1 0 100-2h-3v-2.07z" clip-rule="evenodd" />
          </svg>
          <span>{{ voiceNoteName }}</span>
        </div>

        <div class="flex items-center gap-2">
          <button
            type="button"
            @click="emit('cancel')"
            class="px-3 py-1.5 text-xs text-secondary hover:text-primary transition-colors"
          >
            Cancel
          </button>
          <template v-if="showRecordingControls">
            <button
              type="button"
              @click="emit('reRecord')"
              class="px-3 py-1.5 text-xs text-secondary hover:text-primary transition-colors"
            >
              Re-record
            </button>
          </template>
          <button
            type="button"
            @click="handleConfirm"
            class="px-3 py-1.5 text-xs font-medium text-white bg-accent hover:opacity-90 rounded-md transition-colors"
          >
            Add
          </button>
        </div>
      </div>
    </div>
  </div>
</template>