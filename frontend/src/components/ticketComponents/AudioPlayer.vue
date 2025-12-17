<!-- AudioPlayer.vue -->
<script setup lang="ts">
import { formatDate, formatDateTime } from '@/utils/dateUtils';
import { ref, onMounted, onUnmounted, computed, watch } from 'vue';
import UserAvatar from "@/components/UserAvatar.vue";

const props = defineProps<{
  src: string;
}>();

const emit = defineEmits<{
  (e: 'delete'): void;
}>();

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

const formatTime = (seconds: number): string => {
  if (!seconds || isNaN(seconds) || !isFinite(seconds)) return "0:00";
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  return `${mins}:${secs.toString().padStart(2, "0")}`;
};

const formattedCurrentTime = computed(() => formatTime(currentTime.value));
const formattedDuration = computed(() => {
  // Only return formatted duration if we have a valid duration
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

  // Brand colors
  const brandBlue = '#2C80FF';
  const brandPink = '#FF66B3';
  const brandPurple = '#8B5CF6';

  // Check theme
  const isDark = document.documentElement.classList.contains('dark');
  const bgColor = isDark ? '#1a1f2e' : '#f1f5f9';
  const unplayedColor = isDark ? 'rgba(255,255,255,0.15)' : 'rgba(0,0,0,0.1)';

  // Clear background
  ctx.fillStyle = bgColor;
  ctx.fillRect(0, 0, width, height);

  // Calculate progress
  const progress = durationLoaded.value && duration.value > 0
    ? currentTime.value / duration.value
    : 0;
  const progressX = progress * width;

  // Bar configuration - responsive bar count based on width
  const numBars = Math.max(30, Math.min(80, Math.floor(width / 6)));
  const barWidth = width / numBars;
  const gap = Math.max(1, barWidth * 0.15);
  const maxBarHeight = height * 0.4;

  // Helper: blend two hex colors
  const blendColors = (color1: string, color2: string, t: number): string => {
    const r1 = parseInt(color1.slice(1, 3), 16);
    const g1 = parseInt(color1.slice(3, 5), 16);
    const b1 = parseInt(color1.slice(5, 7), 16);
    const r2 = parseInt(color2.slice(1, 3), 16);
    const g2 = parseInt(color2.slice(3, 5), 16);
    const b2 = parseInt(color2.slice(5, 7), 16);
    const r = Math.round(r1 + (r2 - r1) * t);
    const g = Math.round(g1 + (g2 - g1) * t);
    const b = Math.round(b1 + (b2 - b1) * t);
    return `rgb(${r}, ${g}, ${b})`;
  };

  // Helper: draw rounded rectangle
  const drawRoundedRect = (x: number, y: number, w: number, h: number, r: number) => {
    const radius = Math.min(r, w / 2, Math.abs(h) / 2);
    ctx.beginPath();
    ctx.moveTo(x + radius, y);
    ctx.lineTo(x + w - radius, y);
    ctx.quadraticCurveTo(x + w, y, x + w, y + radius);
    ctx.lineTo(x + w, y + h - radius);
    ctx.quadraticCurveTo(x + w, y + h, x + w - radius, y + h);
    ctx.lineTo(x + radius, y + h);
    ctx.quadraticCurveTo(x, y + h, x, y + h - radius);
    ctx.lineTo(x, y + radius);
    ctx.quadraticCurveTo(x, y, x + radius, y);
    ctx.closePath();
  };

  // Draw bars
  for (let i = 0; i < numBars; i++) {
    const x = i * barWidth;
    const barCenterX = x + barWidth / 2;

    // Sample audio data for this bar
    const dataIndex = Math.floor((i / numBars) * audioData.value.length);
    const amplitude = Math.pow(audioData.value[dataIndex] || 0, 0.8);
    const barHeight = Math.max(2, amplitude * maxBarHeight);

    // Determine if this bar is played
    const isPlayed = barCenterX <= progressX;

    // Color based on frequency position (purple -> pink -> blue)
    const t = i / numBars;
    let playedColor: string;
    if (t < 0.33) {
      playedColor = blendColors(brandPurple, brandPink, t / 0.33);
    } else if (t < 0.66) {
      playedColor = blendColors(brandPink, brandBlue, (t - 0.33) / 0.33);
    } else {
      playedColor = brandBlue;
    }

    const color = isPlayed ? playedColor : unplayedColor;

    ctx.save();
    ctx.fillStyle = color;
    ctx.globalAlpha = isPlayed ? 0.9 : 0.6;

    // Draw top bar
    drawRoundedRect(x + gap / 2, centerY - barHeight, barWidth - gap, barHeight, 2);
    ctx.fill();

    // Draw bottom bar (mirrored)
    drawRoundedRect(x + gap / 2, centerY, barWidth - gap, barHeight, 2);
    ctx.fill();

    ctx.restore();
  }

  // Draw playhead line
  if (progress > 0 && progress < 1) {
    ctx.save();
    ctx.strokeStyle = brandBlue;
    ctx.lineWidth = 2;
    ctx.shadowColor = brandBlue;
    ctx.shadowBlur = 6;
    ctx.beginPath();
    ctx.moveTo(progressX, 2);
    ctx.lineTo(progressX, height - 2);
    ctx.stroke();
    ctx.restore();
  }

  // Subtle center line
  ctx.save();
  ctx.strokeStyle = isDark ? 'rgba(255,255,255,0.1)' : 'rgba(0,0,0,0.08)';
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(0, centerY);
  ctx.lineTo(width, centerY);
  ctx.stroke();
  ctx.restore();
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

    // Initialize audio context first
    initAudioContext();

    const response = await fetch(props.src);
    if (!response.ok) {
      throw new Error(`Failed to load audio: ${response.statusText}`);
    }

    const arrayBuffer = await response.arrayBuffer();
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

const updateProgress = (event: MouseEvent) => {
  if (!progressBarRef.value || !audioRef.value || !durationLoaded.value || !duration.value) return;
  
  const rect = progressBarRef.value.getBoundingClientRect();
  const x = Math.max(0, Math.min(event.clientX - rect.left, rect.width));
  const percentage = x / rect.width;
  
  audioRef.value.currentTime = percentage * duration.value;
  currentTime.value = audioRef.value.currentTime;
  drawWaveform();
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

const togglePlayPause = async () => {
  if (!audioRef.value || isLoading.value || error.value) return;
  
  try {
    if (isPlaying.value) {
      audioRef.value.pause();
    } else {
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

onMounted(async () => {
  if (!audioRef.value) return;

  // Initialize audio context on mount
  initAudioContext();

  // Load audio data immediately
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
  audioRef.value.addEventListener('play', () => isPlaying.value = true);
  audioRef.value.addEventListener('pause', () => isPlaying.value = false);
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

  // Add global mouse event listeners
  document.addEventListener('mousemove', handleMouseMove);
  document.addEventListener('mouseup', handleMouseUp);
  
  // Try to get duration after a short delay as a fallback
  setTimeout(() => {
    checkAndUpdateDuration();
  }, 500);
});

// Add watch on src prop to reload audio data when it changes
watch(() => props.src, () => {
  if (props.src) {
    loadAudioData();
  }
});

onUnmounted(() => {
  if (audioRef.value) {
    audioRef.value.pause();
    audioRef.value.src = '';
  }
  if (audioContext.value) {
    audioContext.value.close();
  }
  // Remove global mouse event listeners
  document.removeEventListener('mousemove', handleMouseMove);
  document.removeEventListener('mouseup', handleMouseUp);
});
</script>

<template>
  <div class="audio-player w-full">
    <!-- Hidden audio element -->
    <audio
      ref="audioRef"
      :src="props.src"
      preload="auto"
      class="hidden"
    />

    <!-- Player controls - single row compact layout -->
    <div class="flex items-center gap-3 w-full">
      <!-- Play/Pause button -->
      <button
        type="button"
        @click="togglePlayPause"
        :disabled="isLoading || !!error"
        class="flex-shrink-0 w-8 h-8 flex items-center justify-center rounded-lg bg-brand-blue hover:opacity-90 disabled:bg-surface-hover disabled:cursor-not-allowed transition-colors"
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
        class="flex-1 h-10 bg-surface rounded-lg cursor-pointer overflow-hidden relative select-none min-w-0"
      >
        <div v-if="isLoading" class="absolute inset-0 flex items-center justify-center">
          <span class="text-xs text-tertiary">Loading...</span>
        </div>
        <div v-else-if="error" class="absolute inset-0 flex items-center justify-center">
          <span class="text-xs text-red-400 px-2 text-center truncate">{{ error }}</span>
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