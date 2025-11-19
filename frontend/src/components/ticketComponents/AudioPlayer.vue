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

  const width = canvas.width;
  const height = canvas.height;
  const step = Math.ceil(audioData.value.length / width);
  const amp = height * 0.9;

  // Solid background color using semantic variable
  ctx.fillStyle = 'var(--bg-surface)';
  ctx.fillRect(0, 0, width, height);

  const waveGradient = ctx.createLinearGradient(0, 0, 0, height);
  waveGradient.addColorStop(0, '#60A5FA');
  waveGradient.addColorStop(1, '#3B82F6');

  ctx.beginPath();
  ctx.strokeStyle = waveGradient;
  ctx.lineWidth = 2;

  ctx.beginPath();
  ctx.moveTo(0, height / 2);

  for (let i = 0; i < width; i++) {
    const index = i * step;
    const windowSize = Math.min(step * 2, audioData.value.length - index);
    let sum = 0;
    for (let j = 0; j < windowSize; j++) {
      sum += Math.abs(audioData.value[index + j] || 0);
    }
    const value = sum / windowSize;
    
    const x = i;
    const y = height / 2 - (value * amp / 2);
    
    if (i === 0) {
      ctx.moveTo(x, y);
    } else {
      const prevX = i - 1;
      const prevY = height / 2 - (Math.abs(audioData.value[prevX * step]) * amp / 2);
      const cpX = x - (x - prevX) / 2;
      const cpY = prevY;
      ctx.quadraticCurveTo(cpX, cpY, x, y);
    }
  }

  for (let i = width - 1; i >= 0; i--) {
    const index = i * step;
    const windowSize = Math.min(step * 2, audioData.value.length - index);
    let sum = 0;
    for (let j = 0; j < windowSize; j++) {
      sum += Math.abs(audioData.value[index + j] || 0);
    }
    const value = sum / windowSize;
    
    const x = i;
    const y = height / 2 + (value * amp / 2);
    
    if (i === width - 1) {
      ctx.lineTo(x, y);
    } else {
      const nextX = i + 1;
      const nextY = height / 2 + (Math.abs(audioData.value[nextX * step]) * amp / 2);
      const cpX = x + (nextX - x) / 2;
      const cpY = nextY;
      ctx.quadraticCurveTo(cpX, cpY, x, y);
    }
  }

  ctx.closePath();
  ctx.fillStyle = waveGradient;
  ctx.fill();

  ctx.shadowColor = '#60A5FA';
  ctx.shadowBlur = 5;
  ctx.strokeStyle = '#93C5FD';
  ctx.lineWidth = 1;
  ctx.stroke();
  ctx.shadowBlur = 0;

  // Only draw progress if we have a valid duration
  if (durationLoaded.value && duration.value > 0 && isFinite(duration.value)) {
    const progress = (currentTime.value / duration.value) * width;
    
    const progressGradient = ctx.createLinearGradient(0, 0, 0, height);
    progressGradient.addColorStop(0, '#2563EB');
    progressGradient.addColorStop(1, '#3B82F6');
    
    ctx.fillStyle = progressGradient;
    ctx.fillRect(0, 0, progress, height);
    
    ctx.save();
    ctx.beginPath();
    ctx.rect(0, 0, progress, height);
    ctx.clip();
    
    ctx.beginPath();
    for (let i = 0; i < width; i++) {
      const index = i * step;
      const windowSize = Math.min(step * 2, audioData.value.length - index);
      let sum = 0;
      for (let j = 0; j < windowSize; j++) {
        sum += Math.abs(audioData.value[index + j] || 0);
      }
      const value = sum / windowSize;
      
      const x = i;
      const y = height / 2 - (value * amp / 2);
      
      if (i === 0) {
        ctx.moveTo(x, y);
      } else {
        const prevX = i - 1;
        const prevY = height / 2 - (Math.abs(audioData.value[prevX * step]) * amp / 2);
        const cpX = x - (x - prevX) / 2;
        const cpY = prevY;
        ctx.quadraticCurveTo(cpX, cpY, x, y);
      }
    }
    
    for (let i = width - 1; i >= 0; i--) {
      const index = i * step;
      const windowSize = Math.min(step * 2, audioData.value.length - index);
      let sum = 0;
      for (let j = 0; j < windowSize; j++) {
        sum += Math.abs(audioData.value[index + j] || 0);
      }
      const value = sum / windowSize;
      
      const x = i;
      const y = height / 2 + (value * amp / 2);
      
      if (i === width - 1) {
        ctx.lineTo(x, y);
      } else {
        const nextX = i + 1;
        const nextY = height / 2 + (Math.abs(audioData.value[nextX * step]) * amp / 2);
        const cpX = x + (nextX - x) / 2;
        const cpY = nextY;
        ctx.quadraticCurveTo(cpX, cpY, x, y);
      }
    }
    
    ctx.closePath();
    ctx.fillStyle = '#93C5FD';
    ctx.fill();
    
    ctx.shadowColor = '#60A5FA';
    ctx.shadowBlur = 8;
    ctx.strokeStyle = '#BFDBFE';
    ctx.lineWidth = 1;
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
  <div class="flex flex-col gap-2 bg-surface-alt rounded-lg w-full">
    <!-- Audio player -->
    <audio
      ref="audioRef"
      :src="props.src"
      preload="auto"
      class="hidden"
    />

    <div class="flex items-center gap-3">
      <button
        type="button"
        @click="togglePlayPause"
        :disabled="isLoading || !!error"
        class="w-8 h-8 flex items-center justify-center rounded-full bg-blue-500 hover:bg-blue-600 disabled:bg-surface-hover disabled:cursor-not-allowed transition-colors"
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

      <div class="flex-1 flex items-center gap-2">
        <span class="text-sm text-secondary min-w-[40px]">{{ formattedCurrentTime }}</span>
        <div
          ref="progressBarRef"
          @mousedown="handleMouseDown"
          class="flex-1 h-8 bg-surface rounded cursor-pointer overflow-hidden relative select-none"
        >
          <div v-if="isLoading" class="absolute inset-0 flex items-center justify-center">
            <span class="text-sm text-tertiary">Loading...</span>
          </div>
          <div v-else-if="error" class="absolute inset-0 flex items-center justify-center">
            <span class="text-sm text-red-400">{{ error }}</span>
          </div>
          <canvas
            v-else
            ref="waveformCanvasRef"
            class="absolute inset-0 w-full h-full"
            width="1000"
            height="32"
          ></canvas>
        </div>
        <span class="text-sm text-tertiary min-w-[40px]">{{ formattedDuration }}</span>
      </div>
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