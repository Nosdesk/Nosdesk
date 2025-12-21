<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed } from 'vue';
import ProgressBar from '@/components/ticketComponents/ProgressBar.vue';

const props = defineProps<{
  src: string;
}>();

const videoRef = ref<HTMLVideoElement | null>(null);
const isPlaying = ref(false);
const currentTime = ref(0);
const duration = ref(0);
const volume = ref(1);
const previousVolume = ref(1); // Store previous volume for unmuting
const isMuted = ref(false);
const isFullscreen = ref(false);
const showControls = ref(false);
const controlsTimeout = ref<number | null>(null);
const wasPlaying = ref(false);

// Computed property to determine which volume icon to show
const volumeIcon = computed(() => {
  if (isMuted.value || volume.value === 0) {
    return 'muted';
  } else if (volume.value < 0.5) {
    return 'low';
  } else {
    return 'high';
  }
});

// Format time for display
const formatTime = (seconds: number): string => {
  if (!seconds || isNaN(seconds)) return "0:00";
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  return `${mins}:${secs.toString().padStart(2, "0")}`;
};

// Logging function for debugging - only log important events
const log = (event: string, details?: any) => {
  // Only log critical events, not mouse movements
  if (!event.includes('mousemove') && !event.includes('showControls')) {
    console.log(`[VideoPlayer] ${event}`, details || '');
  }
};

// Toggle play/pause
const togglePlay = (event?: Event) => {
  if (event) {
    event.preventDefault();
    event.stopPropagation();
  }
  if (!videoRef.value) return;
  if (isPlaying.value) {
    videoRef.value.pause();
    isPlaying.value = false;
  } else {
    videoRef.value.play();
    isPlaying.value = true;
  }
};

// Toggle mute
const toggleMute = (event: Event) => {
  event.preventDefault();
  event.stopPropagation();
  
  if (!videoRef.value) return;
  
  if (isMuted.value) {
    // Unmute: restore previous volume
    volume.value = previousVolume.value > 0 ? previousVolume.value : 0.5;
    videoRef.value.volume = volume.value;
    isMuted.value = false;
  } else {
    // Mute: save current volume and set to 0
    previousVolume.value = volume.value;
    volume.value = 0;
    videoRef.value.volume = 0;
    isMuted.value = true;
  }
};

// Update volume
const updateVolume = (event: Event) => {
  const input = event.target as HTMLInputElement;
  volume.value = Number(input.value);
  
  if (videoRef.value) {
    videoRef.value.volume = volume.value;
    
    // Update mute state based on volume
    if (volume.value === 0) {
      isMuted.value = true;
    } else if (isMuted.value) {
      isMuted.value = false;
    }
  }
};

// Toggle fullscreen
const toggleFullscreen = async () => {
  if (!videoRef.value) return;
  if (!document.fullscreenElement) {
    await videoRef.value.requestFullscreen();
    isFullscreen.value = true;
  } else {
    await document.exitFullscreen();
    isFullscreen.value = false;
  }
};

// Handle seek event from ProgressBar
const handleSeek = (time: number) => {
  if (videoRef.value) {
    // Ensure time is within valid range
    const validTime = Math.max(0, Math.min(time, duration.value || 0));
    
    // Update our local time state immediately for smooth UI
    currentTime.value = validTime;
    
    // Update the actual video time
    videoRef.value.currentTime = validTime;
  }
};

// Handle drag start: Pause video and disable pointer events on video
const handleDragStart = () => {
  if (videoRef.value) {
    wasPlaying.value = isPlaying.value;
    if (isPlaying.value) {
      videoRef.value.pause();
      isPlaying.value = false;
    }
    // Disable pointer events to prevent conflicts during dragging
    videoRef.value.style.pointerEvents = 'none';
  }
};

// Handle drag end: Resume playback and re-enable pointer events on video
const handleDragEnd = () => {
  if (videoRef.value) {
    // Re-enable video interaction
    videoRef.value.style.pointerEvents = 'auto';
    // Only resume if it was playing before
    if (wasPlaying.value) {
      // Use a small timeout to ensure the seek has completed
      setTimeout(() => {
        if (videoRef.value) {
          videoRef.value.play()
            .then(() => {
              isPlaying.value = true;
            })
            .catch(err => {
              console.error('Error resuming playback:', err);
            });
        }
      }, 50);
    }
  }
};

// Show controls temporarily with throttling
let lastControlsUpdate = 0;
const CONTROLS_THROTTLE = 100; // ms

const showControlsTemporarily = () => {
  const now = Date.now();
  // Throttle updates to reduce performance impact
  if (now - lastControlsUpdate < CONTROLS_THROTTLE) {
    return;
  }
  
  lastControlsUpdate = now;
  showControls.value = true;
  
  if (controlsTimeout.value) {
    clearTimeout(controlsTimeout.value);
  }
  
  controlsTimeout.value = window.setTimeout(() => {
    if (isPlaying.value) {
      showControls.value = false;
    }
  }, 2000);
};

// Setup video event listeners
onMounted(() => {
  log('mounted');
  if (!videoRef.value) return;
  
  // Set initial volume
  videoRef.value.volume = volume.value;
  
  videoRef.value.addEventListener('play', () => {
    log('video:play');
    isPlaying.value = true;
  });
  videoRef.value.addEventListener('pause', () => {
    log('video:pause');
    isPlaying.value = false;
  });
  videoRef.value.addEventListener('timeupdate', () => {
    currentTime.value = videoRef.value?.currentTime || 0;
  });
  videoRef.value.addEventListener('loadedmetadata', () => {
    log('video:loadedmetadata', { duration: videoRef.value?.duration });
    duration.value = videoRef.value?.duration || 0;
  });
  videoRef.value.addEventListener('click', (e) => {
    togglePlay(e);
  });
  document.addEventListener('fullscreenchange', () => {
    isFullscreen.value = !!document.fullscreenElement;
  });
});

// Cleanup
onUnmounted(() => {
  if (controlsTimeout.value) {
    clearTimeout(controlsTimeout.value);
  }
});
</script>

<template>
  <div
    class="relative bg-surface rounded-lg overflow-hidden group min-w-[300px]"
    @mousemove.passive="showControlsTemporarily"
    @mouseleave="showControls = false"
  >
    <!-- Video element -->
    <video
      ref="videoRef"
      :src="props.src"
      class="relative w-full h-full object-contain rounded-lg"
      preload="metadata"
      controlsList="nodownload"
    ></video>

    <!-- Video Controls -->
    <div
      class="absolute bottom-0 left-0 right-0 bg-gradient-to-t from-surface/80 to-transparent p-4 transition-opacity duration-300 flex flex-col gap-1"
      :class="[showControls || !isPlaying ? 'opacity-100' : 'opacity-0']"
    >
      <!-- Progress Bar Container -->
      <div class="py-2">
        <ProgressBar
          :current-time="currentTime"
          :duration="duration"
          @seek="handleSeek"
          @dragstart="handleDragStart"
          @dragend="handleDragEnd"
        />
      </div>

      <div class="flex items-center justify-between">
        <div class="flex items-center gap-4">
          <!-- Play/Pause Button -->
          <button
            @click="togglePlay"
            class="text-primary hover:text-accent transition-colors"
            type="button"
          >
            <svg v-if="!isPlaying" class="w-6 h-6" viewBox="0 0 24 24" fill="currentColor">
              <path d="M8 5v14l11-7z" />
            </svg>
            <svg v-else class="w-6 h-6" viewBox="0 0 24 24" fill="currentColor">
              <path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z" />
            </svg>
          </button>

          <!-- Volume Control -->
          <div class="flex items-center gap-2">
            <button
              @click="toggleMute"
              class="text-primary hover:text-accent transition-colors"
              type="button"
            >
              <!-- Muted icon -->
              <svg v-if="volumeIcon === 'muted'" class="w-5 h-5" viewBox="0 0 24 24" fill="currentColor">
                <path d="M16.5 12c0-1.77-1.02-3.29-2.5-4.03v2.21l2.45 2.45c.03-.2.05-.41.05-.63zm2.5 0c0 .94-.2 1.82-.54 2.64l1.51 1.51C20.63 14.91 21 13.5 21 12c0-4.28-2.99-7.86-7-8.77v2.06c2.89.86 5 3.54 5 6.71zM4.27 3L3 4.27 7.73 9H3v6h4l5 5v-6.73l4.25 4.25c-.67.52-1.42.93-2.25 1.18v2.06c1.38-.31 2.63-.95 3.69-1.81L19.73 21 21 19.73l-9-9L4.27 3zM12 4L9.91 6.09 12 8.18V4z" />
              </svg>
              <!-- Low volume icon -->
              <svg v-else-if="volumeIcon === 'low'" class="w-5 h-5" viewBox="0 0 24 24" fill="currentColor">
                <path d="M7 9v6h4l5 5V4l-5 5H7z" />
              </svg>
              <!-- High volume icon -->
              <svg v-else class="w-5 h-5" viewBox="0 0 24 24" fill="currentColor">
                <path d="M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02zM14 3.23v2.06c2.89.86 5 3.54 5 6.71s-2.11 5.85-5 6.71v2.06c4.01-.91 7-4.49 7-8.77s-2.99-7.86-7-8.77z" />
              </svg>
            </button>
            <input
              type="range"
              min="0"
              max="1"
              step="0.1"
              :value="volume"
              @input="updateVolume"
              class="w-20 h-1 accent-accent"
            />
          </div>

          <!-- Time Display -->
          <div class="text-primary text-sm">
            {{ formatTime(currentTime) }} / {{ formatTime(duration) }}
          </div>
        </div>

        <!-- Fullscreen Button -->
        <button
          @click="toggleFullscreen"
          class="text-primary hover:text-accent transition-colors"
          type="button"
        >
          <svg v-if="!isFullscreen" class="w-5 h-5" viewBox="0 0 24 24" fill="currentColor">
            <path d="M7 14H5v5h5v-2H7v-3zm-2-4h2V7h3V5H5v5zm12 7h-3v2h5v-5h-2v3zM14 5v2h3v3h2V5h-5z" />
          </svg>
          <svg v-else class="w-5 h-5" viewBox="0 0 24 24" fill="currentColor">
            <path d="M5 16h3v3h2v-5H5v2zm3-8H5v2h5V5H8v3zm6 11h2v-3h3v-2h-5v5zm2-11V5h-2v5h5V8h-3z" />
          </svg>
        </button>
      </div>
    </div>
  </div>
</template>