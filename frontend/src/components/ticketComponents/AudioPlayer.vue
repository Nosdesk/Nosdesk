<!-- AudioPlayer.vue -->
<script setup lang="ts">
import { formatDate, formatDateTime } from '@/utils/dateUtils';
import { ref, onMounted, onUnmounted, computed, watch } from 'vue';
import UserAvatar from "@/components/UserAvatar.vue";
import { useAudioPlayer } from '@/composables/useAudioPlayer';

const props = defineProps<{
  src: string;
  transcription?: string;
}>();

const emit = defineEmits<{
  (e: 'delete'): void;
}>();

// Global audio player management - ensures only one plays at a time
const { playerId, onPauseOthers, notifyPlayStarted } = useAudioPlayer();
let cleanupPauseListener: (() => void) | null = null;

const audioRef = ref<HTMLAudioElement | null>(null);
const waveformCanvasRef = ref<HTMLCanvasElement | null>(null);
const audioContext = ref<AudioContext | null>(null);
const isPlaying = ref(false);
const currentTime = ref(0);
const duration = ref(0);
const progressBarRef = ref<HTMLDivElement | null>(null);
const audioData = ref<Float32Array | null>(null);
const isLoading = ref(true);
const error = ref<string | null>(null);
const isDragging = ref(false);
const durationLoaded = ref(false);
const isTouchDragging = ref(false);

// Lazy loading - only load audio when visible
const containerRef = ref<HTMLElement | null>(null);
const isVisible = ref(false);
const hasLoadedOnce = ref(false);
let intersectionObserver: IntersectionObserver | null = null;

// Blob URL for single-fetch optimization - download once, use for both waveform and playback
const blobUrl = ref<string | null>(null);

const formatTime = (seconds: number): string => {
  if (!seconds || isNaN(seconds) || !isFinite(seconds)) return "0:00";
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  return `${mins}:${secs.toString().padStart(2, "0")}`;
};

const formattedCurrentTime = computed(() => formatTime(currentTime.value));
const formattedDuration = computed(() => {
  // Only return formatted duration if a valid duration exists
  if (durationLoaded.value && duration.value > 0 && isFinite(duration.value)) {
    return formatTime(duration.value);
  }
  return "0:00";
});

const formattedDate = (dateString?: string): string => {
  if (!dateString) return "";
  const date = new Date(dateString);
  return formatDate(dateString, "MMM d, yyyy");
};

// Helper to get CSS variable value
const getCSSVar = (name: string): string => {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
};

// Parse color to RGB for gradient creation
const parseColor = (color: string): { r: number; g: number; b: number } => {
  if (color.startsWith('#')) {
    const hex = color.slice(1);
    return {
      r: parseInt(hex.slice(0, 2), 16),
      g: parseInt(hex.slice(2, 4), 16),
      b: parseInt(hex.slice(4, 6), 16)
    };
  }
  const match = color.match(/(\d+)/g);
  if (match && match.length >= 3) {
    return { r: parseInt(match[0]), g: parseInt(match[1]), b: parseInt(match[2]) };
  }
  return { r: 44, g: 128, b: 255 };
};

const drawWaveform = () => {
  if (!waveformCanvasRef.value || !audioData.value) return;

  const canvas = waveformCanvasRef.value;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  // Handle high DPI displays
  const dpr = window.devicePixelRatio || 1;
  const rect = canvas.getBoundingClientRect();
  canvas.width = rect.width * dpr;
  canvas.height = rect.height * dpr;
  ctx.scale(dpr, dpr);

  const width = rect.width;
  const height = rect.height;
  const centerY = height / 2;

  // Get theme colors from CSS variables
  const accentColor = getCSSVar('--color-accent') || '#2C80FF';
  const isDark = document.documentElement.classList.contains('dark');
  const bgColor = getCSSVar('--color-surface') || (isDark ? '#1a1f2e' : '#f1f5f9');

  // Clear background
  ctx.fillStyle = bgColor;
  ctx.fillRect(0, 0, width, height);

  // Calculate progress
  const progress = durationLoaded.value && duration.value > 0
    ? currentTime.value / duration.value
    : 0;
  const progressX = progress * width;

  // More points for detailed waveform that reflects actual audio
  const numPoints = Math.max(100, Math.min(200, Math.floor(width / 2)));
  const step = width / (numPoints - 1);
  const maxAmplitude = height * 0.42;

  // Sample audio data with minimal smoothing to preserve detail
  const getAmplitude = (index: number): number => {
    const dataIndex = Math.floor((index / numPoints) * audioData.value!.length);
    // Light 3-point smoothing to reduce harsh spikes while keeping detail
    const idx0 = Math.max(0, dataIndex - 1);
    const idx1 = dataIndex;
    const idx2 = Math.min(audioData.value!.length - 1, dataIndex + 1);

    const smoothed = (
      (audioData.value![idx0] || 0) * 0.2 +
      (audioData.value![idx1] || 0) * 0.6 +
      (audioData.value![idx2] || 0) * 0.2
    );
    // Slight boost to quieter parts while preserving peaks
    return Math.pow(smoothed, 0.85);
  };

  // Simple linear interpolation - less smoothing than Catmull-Rom
  const getInterpolatedAmplitude = (index: number): number => {
    const i = Math.floor(index);
    const t = index - i;
    const p1 = getAmplitude(i);
    const p2 = getAmplitude(Math.min(numPoints - 1, i + 1));
    return p1 + (p2 - p1) * t;
  };

  const rgb = parseColor(accentColor);
  const unplayedRgb = isDark ? { r: 255, g: 255, b: 255 } : { r: 0, g: 0, b: 0 };

  // Helper to draw a smooth filled waveform section
  const drawWaveSection = (startX: number, endX: number, isPlayed: boolean) => {
    if (endX <= startX) return;

    const color = isPlayed ? rgb : unplayedRgb;
    const baseAlpha = isPlayed ? 0.7 : (isDark ? 0.12 : 0.08);

    // Create vertical gradient for depth
    const gradient = ctx.createLinearGradient(0, centerY - maxAmplitude, 0, centerY + maxAmplitude);
    gradient.addColorStop(0, `rgba(${color.r}, ${color.g}, ${color.b}, ${baseAlpha * 0.7})`);
    gradient.addColorStop(0.5, `rgba(${color.r}, ${color.g}, ${color.b}, ${baseAlpha})`);
    gradient.addColorStop(1, `rgba(${color.r}, ${color.g}, ${color.b}, ${baseAlpha * 0.7})`);

    ctx.save();
    ctx.fillStyle = gradient;
    ctx.beginPath();

    // Find start and end point indices
    const startIndex = Math.floor((startX / width) * (numPoints - 1));
    const endIndex = Math.ceil((endX / width) * (numPoints - 1));

    // Start from bottom-left of section
    ctx.moveTo(startX, centerY);

    // Draw top edge with smooth Bezier curves
    for (let i = startIndex; i <= endIndex; i++) {
      const x = i * step;
      if (x < startX || x > endX) continue;

      const amplitude = getInterpolatedAmplitude(i);
      const y = centerY - amplitude * maxAmplitude;

      if (i === startIndex || x === startX) {
        ctx.lineTo(x, y);
      } else {
        const prevX = (i - 1) * step;
        const prevAmplitude = getInterpolatedAmplitude(i - 1);
        const prevY = centerY - prevAmplitude * maxAmplitude;
        const cpX = (prevX + x) / 2;
        ctx.quadraticCurveTo(prevX, prevY, cpX, (prevY + y) / 2);
      }
    }

    // Connect to right edge at center
    ctx.lineTo(endX, centerY);

    // Draw bottom edge (mirrored) going backwards
    for (let i = endIndex; i >= startIndex; i--) {
      const x = i * step;
      if (x < startX || x > endX) continue;

      const amplitude = getInterpolatedAmplitude(i);
      const y = centerY + amplitude * maxAmplitude;

      if (i === endIndex || x === endX) {
        ctx.lineTo(x, y);
      } else {
        const nextX = (i + 1) * step;
        const nextAmplitude = getInterpolatedAmplitude(i + 1);
        const nextY = centerY + nextAmplitude * maxAmplitude;
        const cpX = (nextX + x) / 2;
        ctx.quadraticCurveTo(nextX, nextY, cpX, (nextY + y) / 2);
      }
    }

    ctx.closePath();
    ctx.fill();
    ctx.restore();
  };

  // Draw unplayed portion (full width, will be overlaid)
  drawWaveSection(0, width, false);

  // Draw played portion on top
  if (progressX > 0) {
    drawWaveSection(0, progressX, true);
  }

  // Draw elegant playhead
  if (progress > 0 && progress < 1) {
    // Get amplitude at playhead position for dynamic height
    const playheadIndex = (progressX / width) * (numPoints - 1);
    const playheadAmplitude = getInterpolatedAmplitude(playheadIndex);
    const playheadHeight = Math.max(height * 0.3, playheadAmplitude * maxAmplitude + 4);

    ctx.save();
    // Glowing line
    ctx.strokeStyle = accentColor;
    ctx.lineWidth = 2;
    ctx.shadowColor = accentColor;
    ctx.shadowBlur = 8;
    ctx.lineCap = 'round';
    ctx.beginPath();
    ctx.moveTo(progressX, centerY - playheadHeight);
    ctx.lineTo(progressX, centerY + playheadHeight);
    ctx.stroke();

    // Small dot at center
    ctx.fillStyle = accentColor;
    ctx.shadowBlur = 12;
    ctx.beginPath();
    ctx.arc(progressX, centerY, 3, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
  }

  // Subtle center line when no audio
  const avgAmplitude = audioData.value.reduce((a, b) => a + b, 0) / audioData.value.length;
  if (avgAmplitude < 0.05) {
    ctx.save();
    ctx.strokeStyle = getCSSVar('--color-tertiary') || (isDark ? 'rgba(255,255,255,0.15)' : 'rgba(0,0,0,0.1)');
    ctx.lineWidth = 1;
    ctx.globalAlpha = 0.5;
    ctx.beginPath();
    ctx.moveTo(0, centerY);
    ctx.lineTo(width, centerY);
    ctx.stroke();
    ctx.restore();
  }
};

const initAudioContext = () => {
  if (!audioContext.value) {
    audioContext.value = new AudioContext();
  } else if (audioContext.value.state === 'suspended') {
    audioContext.value.resume();
  }
};

const loadAudioData = async () => {
  if (!props.src) return;

  try {
    isLoading.value = true;
    error.value = null;
    durationLoaded.value = false;

    // Reset audio data
    if (audioData.value) {
      audioData.value = null;
    }

    // Revoke previous blob URL if exists
    if (blobUrl.value) {
      URL.revokeObjectURL(blobUrl.value);
      blobUrl.value = null;
    }

    // Initialize audio context first
    initAudioContext();

    // Fetch audio once - used for both waveform AND playback
    const response = await fetch(props.src);
    if (!response.ok) {
      throw new Error(`Failed to load audio: ${response.statusText}`);
    }

    // Get as blob for creating object URL (for audio element playback)
    const blob = await response.blob();
    blobUrl.value = URL.createObjectURL(blob);

    // Set the audio element src to use the blob URL (no second download!)
    if (audioRef.value) {
      audioRef.value.src = blobUrl.value;
    }

    // Convert blob to ArrayBuffer for waveform analysis
    const arrayBuffer = await blob.arrayBuffer();
    const audioBuffer = await audioContext.value!.decodeAudioData(arrayBuffer);

    // Set duration immediately and mark as loaded
    if (audioBuffer.duration && isFinite(audioBuffer.duration)) {
      duration.value = audioBuffer.duration;
      durationLoaded.value = true;
    }

    const rawData = audioBuffer.getChannelData(0);
    const samples = 2000;
    const blockSize = Math.floor(rawData.length / samples);
    const filteredData = new Float32Array(samples);

    let maxAmp = 0;
    for (let i = 0; i < rawData.length; i += blockSize) {
      const sampleAmp = Math.abs(rawData[i]);
      if (sampleAmp > maxAmp) maxAmp = sampleAmp;
    }

    for (let i = 0; i < samples; i++) {
      const blockStart = blockSize * i;
      let sum = 0;
      let count = 0;

      for (let j = 0; j < blockSize && (blockStart + j) < rawData.length; j++) {
        sum += Math.abs(rawData[blockStart + j]);
        count++;
      }

      filteredData[i] = count > 0 ? (sum / count) / (maxAmp || 1) : 0;
    }

    audioData.value = filteredData;
    drawWaveform();
    isLoading.value = false;

    // Force a redraw after a short delay to ensure canvas is properly sized
    setTimeout(() => {
      drawWaveform();
    }, 100);
  } catch (err) {
    console.error('Error loading audio:', err);
    error.value = err instanceof Error ? err.message : "Failed to load audio";
    isLoading.value = false;
    audioData.value = null;
  }
};

// Update progress from a clientX position
const updateProgressFromX = (clientX: number) => {
  if (!progressBarRef.value || !audioRef.value || !durationLoaded.value || !duration.value) return;

  const rect = progressBarRef.value.getBoundingClientRect();
  const x = Math.max(0, Math.min(clientX - rect.left, rect.width));
  const percentage = x / rect.width;

  audioRef.value.currentTime = percentage * duration.value;
  currentTime.value = audioRef.value.currentTime;
  drawWaveform();
};

const updateProgress = (event: MouseEvent) => {
  updateProgressFromX(event.clientX);
};

const handleMouseDown = (event: MouseEvent) => {
  if (!audioRef.value || isLoading.value || error.value) return;
  isDragging.value = true;
  updateProgress(event);
};

const handleMouseMove = (event: MouseEvent) => {
  if (!isDragging.value) return;
  updateProgress(event);
};

const handleMouseUp = (event: MouseEvent) => {
  if (!isDragging.value) return;
  isDragging.value = false;
  updateProgress(event);
};

// Touch event handlers for mobile scrubbing
const handleTouchStart = (event: TouchEvent) => {
  if (!audioRef.value || isLoading.value || error.value) return;
  // Prevent scroll while scrubbing
  event.preventDefault();
  isTouchDragging.value = true;

  const touch = event.touches[0];
  if (touch) {
    updateProgressFromX(touch.clientX);
  }
};

const handleTouchMove = (event: TouchEvent) => {
  if (!isTouchDragging.value) return;
  // Prevent scroll while scrubbing
  event.preventDefault();

  const touch = event.touches[0];
  if (touch) {
    updateProgressFromX(touch.clientX);
  }
};

const handleTouchEnd = (event: TouchEvent) => {
  if (!isTouchDragging.value) return;
  isTouchDragging.value = false;

  // Use changedTouches for the final position
  const touch = event.changedTouches[0];
  if (touch) {
    updateProgressFromX(touch.clientX);
  }
};

const togglePlayPause = async () => {
  if (!audioRef.value || isLoading.value || error.value) return;

  try {
    if (isPlaying.value) {
      audioRef.value.pause();
    } else {
      // Notify other players to pause before we start
      notifyPlayStarted();

      if (audioContext.value?.state === 'suspended') {
        await audioContext.value.resume();
      }
      await audioRef.value.play();
    }
  } catch (err) {
    console.error('Playback error:', err);
    error.value = "Failed to play audio";
  }
};

// Function to manually check and update duration
const checkAndUpdateDuration = () => {
  if (audioRef.value && audioRef.value.duration && isFinite(audioRef.value.duration)) {
    duration.value = audioRef.value.duration;
    durationLoaded.value = true;
    drawWaveform();
  }
};

// Initialize audio and set up event listeners (called when visible)
const initializeAudio = async () => {
  if (!audioRef.value || hasLoadedOnce.value) return;
  hasLoadedOnce.value = true;

  // Initialize audio context
  initAudioContext();

  // Load audio data
  await loadAudioData();

  // Set up event listeners
  audioRef.value.addEventListener('timeupdate', () => {
    if (!audioRef.value) return;
    currentTime.value = audioRef.value.currentTime;

    // Check duration on timeupdate as well (helps with some browsers/formats)
    if (!durationLoaded.value) {
      checkAndUpdateDuration();
    }

    drawWaveform();
  });
  audioRef.value.addEventListener('play', () => {
    isPlaying.value = true;
  });
  audioRef.value.addEventListener('pause', () => {
    isPlaying.value = false;
  });
  audioRef.value.addEventListener('ended', () => {
    isPlaying.value = false;
    currentTime.value = 0;
    drawWaveform();
  });

  // Add load event listeners
  audioRef.value.addEventListener('loadedmetadata', () => {
    checkAndUpdateDuration();
  });

  audioRef.value.addEventListener('durationchange', () => {
    checkAndUpdateDuration();
  });

  audioRef.value.addEventListener('canplaythrough', () => {
    checkAndUpdateDuration();
  });

  // Try to get duration after a short delay as a fallback
  setTimeout(() => {
    checkAndUpdateDuration();
  }, 500);
};

onMounted(() => {
  // Listen for pause events from other players
  cleanupPauseListener = onPauseOthers(() => {
    if (audioRef.value && isPlaying.value) {
      audioRef.value.pause();
    }
  });

  // Add global mouse event listeners
  document.addEventListener('mousemove', handleMouseMove);
  document.addEventListener('mouseup', handleMouseUp);

  // Add global touch event listeners for scrubbing outside the element
  document.addEventListener('touchmove', handleTouchMove, { passive: false });
  document.addEventListener('touchend', handleTouchEnd);
  document.addEventListener('touchcancel', handleTouchEnd);

  // Set up Intersection Observer for lazy loading
  if (containerRef.value && 'IntersectionObserver' in window) {
    intersectionObserver = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (entry.isIntersecting && !hasLoadedOnce.value) {
            isVisible.value = true;
            // Load audio when component becomes visible
            initializeAudio();
          }
        });
      },
      {
        // Start loading slightly before the element is visible
        rootMargin: '100px',
        threshold: 0
      }
    );
    intersectionObserver.observe(containerRef.value);
  } else {
    // Fallback for browsers without IntersectionObserver
    isVisible.value = true;
    initializeAudio();
  }
});

// Add watch on src prop to reload audio data when it changes
watch(() => props.src, () => {
  if (props.src) {
    loadAudioData();
  }
});

onUnmounted(() => {
  // Clean up Intersection Observer
  if (intersectionObserver) {
    intersectionObserver.disconnect();
    intersectionObserver = null;
  }

  // Clean up pause listener
  if (cleanupPauseListener) {
    cleanupPauseListener();
  }

  if (audioRef.value) {
    audioRef.value.pause();
    audioRef.value.src = '';
  }
  if (audioContext.value) {
    audioContext.value.close();
  }

  // Revoke blob URL to free memory
  if (blobUrl.value) {
    URL.revokeObjectURL(blobUrl.value);
    blobUrl.value = null;
  }

  // Remove global mouse event listeners
  document.removeEventListener('mousemove', handleMouseMove);
  document.removeEventListener('mouseup', handleMouseUp);

  // Remove global touch event listeners
  document.removeEventListener('touchmove', handleTouchMove);
  document.removeEventListener('touchend', handleTouchEnd);
  document.removeEventListener('touchcancel', handleTouchEnd);
});
</script>

<template>
  <div ref="containerRef" class="audio-player w-full">
    <!-- Hidden audio element - src is set programmatically from blob URL after fetch -->
    <audio
      ref="audioRef"
      preload="none"
      class="hidden"
    />

    <!-- Player controls - single row compact layout -->
    <div class="flex items-center gap-3 w-full">
      <!-- Play/Pause button -->
      <button
        type="button"
        @click="togglePlayPause"
        :disabled="isLoading || !!error"
        class="flex-shrink-0 w-8 h-8 flex items-center justify-center rounded-lg bg-accent hover:opacity-90 disabled:bg-surface-hover disabled:cursor-not-allowed transition-colors"
        :aria-label="isPlaying ? 'Pause' : 'Play'"
      >
        <template v-if="isLoading">
          <svg class="animate-spin h-4 w-4 text-white" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
            <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
          </svg>
        </template>
        <template v-else-if="error">
          <svg class="w-4 h-4 text-white" viewBox="0 0 20 20" fill="currentColor">
            <path fill-rule="evenodd" d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7 4a1 1 0 11-2 0 1 1 0 012 0zm-1-9a1 1 0 00-1 1v4a1 1 0 102 0V6a1 1 0 00-1-1z" clip-rule="evenodd" />
          </svg>
        </template>
        <template v-else>
          <svg v-if="!isPlaying" class="w-4 h-4 text-white ml-0.5" viewBox="0 0 24 24" fill="currentColor">
            <path d="M8 5v14l11-7z" />
          </svg>
          <svg v-else class="w-4 h-4 text-white" viewBox="0 0 24 24" fill="currentColor">
            <path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z" />
          </svg>
        </template>
      </button>

      <!-- Current time -->
      <span class="text-xs font-mono text-secondary tabular-nums flex-shrink-0 w-8">{{ formattedCurrentTime }}</span>

      <!-- Waveform -->
      <div
        ref="progressBarRef"
        @mousedown="handleMouseDown"
        @touchstart="handleTouchStart"
        class="flex-1 h-10 bg-surface rounded-lg cursor-pointer overflow-hidden relative select-none min-w-0 touch-none"
      >
        <div v-if="isLoading" class="absolute inset-0 flex items-center justify-center">
          <span class="text-xs text-tertiary">Loading...</span>
        </div>
        <div v-else-if="error" class="absolute inset-0 flex items-center justify-center">
          <span class="text-xs text-status-error px-2 text-center truncate">{{ error }}</span>
        </div>
        <canvas
          v-else
          ref="waveformCanvasRef"
          class="absolute inset-0 w-full h-full"
        ></canvas>
      </div>

      <!-- Duration -->
      <span class="text-xs font-mono text-tertiary tabular-nums flex-shrink-0 w-8">{{ formattedDuration }}</span>
    </div>

    <!-- Transcription display -->
    <div v-if="props.transcription" class="mt-2">
      <details class="group">
        <summary class="text-xs text-tertiary hover:text-secondary cursor-pointer select-none flex items-center gap-1 py-1">
          <svg class="w-3 h-3 transition-transform group-open:rotate-90" viewBox="0 0 20 20" fill="currentColor">
            <path fill-rule="evenodd" d="M7.293 14.707a1 1 0 010-1.414L10.586 10 7.293 6.707a1 1 0 011.414-1.414l4 4a1 1 0 010 1.414l-4 4a1 1 0 01-1.414 0z" clip-rule="evenodd" />
          </svg>
          Transcription
        </summary>
        <p class="mt-1 text-xs text-secondary italic p-2 bg-surface rounded-md">
          {{ props.transcription }}
        </p>
      </details>
    </div>
  </div>
</template>

<style scoped>
.progress-bar {
  position: relative;
  height: 32px;
  background: var(--bg-surface);
  border-radius: 0.375rem;
  overflow: hidden;
}
</style>