<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed } from 'vue';

const emit = defineEmits<{
  (e: 'recordingComplete', value: { blob: Blob; duration: number }): void;
  (e: 'cancel'): void;
}>();

// Define states for recording
const isRecording = ref(false);
const recordingTime = ref(0);
const mediaRecorder = ref<MediaRecorder | null>(null);
const audioChunks = ref<Blob[]>([]);
const recordingTimer = ref<number | null>(null);

// Visualization state
const audioContext = ref<AudioContext | null>(null);
const analyser = ref<AnalyserNode | null>(null);
const canvasRef = ref<HTMLCanvasElement | null>(null);
const animationFrameId = ref<number | null>(null);

// Track theme for canvas colors
const isDarkMode = ref(false);

// Format recording time
const formatTime = (seconds: number): string => {
  const mins = Math.floor(seconds / 60);
  const secs = seconds % 60;
  return `${mins}:${secs.toString().padStart(2, "0")}`;
};

// Get CSS variable value
const getCSSVar = (name: string): string => {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
};

// Setup waveform visualization
const setupVisualization = (stream: MediaStream) => {
  audioContext.value = new AudioContext();
  analyser.value = audioContext.value.createAnalyser();
  const source = audioContext.value.createMediaStreamSource(stream);
  source.connect(analyser.value);

  analyser.value.fftSize = 256;
  analyser.value.smoothingTimeConstant = 0.6;
  analyser.value.minDecibels = -90;
  analyser.value.maxDecibels = -10;

  // Check dark mode
  isDarkMode.value = document.documentElement.classList.contains('dark');

  drawWaveform();
};

const drawWaveform = () => {
  if (!canvasRef.value || !analyser.value) return;

  const canvas = canvasRef.value;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  // Set canvas resolution for retina displays
  const dpr = window.devicePixelRatio || 1;
  const rect = canvas.getBoundingClientRect();
  canvas.width = rect.width * dpr;
  canvas.height = rect.height * dpr;
  ctx.scale(dpr, dpr);

  const width = rect.width;
  const height = rect.height;
  const centerY = height / 2;
  const bufferLength = analyser.value.frequencyBinCount;
  const dataArray = new Uint8Array(bufferLength);

  // Smoothed values for organic motion
  const numBars = 48;
  let smoothedValues = new Array(numBars).fill(0);

  // Colors from design system
  const brandBlue = '#2C80FF';
  const brandPink = '#FF66B3';
  const brandPurple = '#8B5CF6';

  const draw = () => {
    animationFrameId.value = requestAnimationFrame(draw);
    analyser.value!.getByteFrequencyData(dataArray);

    // Get background color based on theme
    const bgColor = isDarkMode.value ? '#1a1f2e' : '#f8fafc';

    // Clear canvas
    ctx.fillStyle = bgColor;
    ctx.fillRect(0, 0, width, height);

    // Calculate smoothed amplitude values - pure audio, no artificial animation
    for (let i = 0; i < numBars; i++) {
      // Sample from different frequency ranges for each bar
      const dataIndex = Math.floor((i / numBars) * bufferLength * 0.85);
      const rawValue = dataArray[dataIndex] / 255;
      // Boost quieter sounds with power curve
      const boostedValue = Math.pow(rawValue, 0.6) * 1.4;
      const targetValue = Math.min(1, boostedValue);
      // Smooth transitions
      smoothedValues[i] += (targetValue - smoothedValues[i]) * 0.25;
    }

    const barWidth = width / numBars;
    const gap = 2;
    const maxBarHeight = height * 0.42;

    // Draw abstract frequency bars - mirrored from center
    for (let i = 0; i < numBars; i++) {
      const x = i * barWidth;
      const amplitude = smoothedValues[i];
      const barHeight = amplitude * maxBarHeight;

      // Color interpolation based on frequency position
      // Low frequencies: purple, mid: pink, high: blue
      const t = i / numBars;
      let color: string;
      let glowColor: string;

      if (t < 0.33) {
        // Purple to pink
        const blend = t / 0.33;
        color = blendColors(brandPurple, brandPink, blend);
        glowColor = brandPurple;
      } else if (t < 0.66) {
        // Pink to blue
        const blend = (t - 0.33) / 0.33;
        color = blendColors(brandPink, brandBlue, blend);
        glowColor = brandPink;
      } else {
        // Blue
        color = brandBlue;
        glowColor = brandBlue;
      }

      // Draw top bar (above center)
      ctx.save();
      ctx.fillStyle = color;
      ctx.globalAlpha = 0.7 + amplitude * 0.3;

      // Add glow based on amplitude
      if (amplitude > 0.3) {
        ctx.shadowColor = glowColor;
        ctx.shadowBlur = amplitude * 12;
      }

      // Rounded rectangle for each bar
      const cornerRadius = Math.min(barWidth - gap, 4);
      drawRoundedRect(ctx, x + gap / 2, centerY - barHeight, barWidth - gap, barHeight, cornerRadius);
      ctx.fill();
      ctx.restore();

      // Draw bottom bar (below center) - mirrored
      ctx.save();
      ctx.fillStyle = color;
      ctx.globalAlpha = 0.7 + amplitude * 0.3;

      if (amplitude > 0.3) {
        ctx.shadowColor = glowColor;
        ctx.shadowBlur = amplitude * 12;
      }

      drawRoundedRect(ctx, x + gap / 2, centerY, barWidth - gap, barHeight, cornerRadius);
      ctx.fill();
      ctx.restore();
    }

    // Subtle center line
    ctx.save();
    ctx.strokeStyle = brandBlue;
    ctx.lineWidth = 1;
    ctx.globalAlpha = 0.15;
    ctx.beginPath();
    ctx.moveTo(0, centerY);
    ctx.lineTo(width, centerY);
    ctx.stroke();
    ctx.restore();
  };

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
  const drawRoundedRect = (ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) => {
    ctx.beginPath();
    ctx.moveTo(x + r, y);
    ctx.lineTo(x + w - r, y);
    ctx.quadraticCurveTo(x + w, y, x + w, y + r);
    ctx.lineTo(x + w, y + h - r);
    ctx.quadraticCurveTo(x + w, y + h, x + w - r, y + h);
    ctx.lineTo(x + r, y + h);
    ctx.quadraticCurveTo(x, y + h, x, y + h - r);
    ctx.lineTo(x, y + r);
    ctx.quadraticCurveTo(x, y, x + r, y);
    ctx.closePath();
  };

  draw();
};

// Start recording
const startRecording = async () => {
  try {
    const stream = await navigator.mediaDevices.getUserMedia({
      audio: {
        echoCancellation: true,
        noiseSuppression: true,
        autoGainControl: true
      }
    });

    mediaRecorder.value = new MediaRecorder(stream);
    audioChunks.value = [];
    isRecording.value = true;
    recordingTime.value = 0;

    recordingTimer.value = window.setInterval(() => {
      recordingTime.value++;
    }, 1000);

    mediaRecorder.value.ondataavailable = (event) => {
      if (event.data.size > 0) {
        audioChunks.value.push(event.data);
      }
    };

    mediaRecorder.value.onstop = () => {
      const audioBlob = new Blob(audioChunks.value, { type: "audio/webm" });

      if (audioBlob.size < 100) {
        console.error("Recording too small");
        emit('cancel');
        return;
      }

      emit('recordingComplete', {
        blob: audioBlob,
        duration: recordingTime.value
      });
    };

    mediaRecorder.value.start(250);
    setupVisualization(stream);
  } catch (error) {
    console.error("Error accessing microphone:", error);
    alert("Could not access microphone. Please check your permissions.");
    emit('cancel');
  }
};

// Stop recording
const stopRecording = () => {
  if (mediaRecorder.value && isRecording.value) {
    mediaRecorder.value.stop();
    cleanup();
  }
};

// Cancel recording
const cancelRecording = () => {
  if (mediaRecorder.value && isRecording.value) {
    mediaRecorder.value.stop();
    cleanup();
    emit('cancel');
  }
};

const cleanup = () => {
  isRecording.value = false;
  if (recordingTimer.value) {
    clearInterval(recordingTimer.value);
    recordingTimer.value = null;
  }
  if (animationFrameId.value) {
    cancelAnimationFrame(animationFrameId.value);
    animationFrameId.value = null;
  }
  if (audioContext.value) {
    audioContext.value.close().catch(console.error);
    audioContext.value = null;
  }
};

// Start recording on mount
onMounted(() => {
  startRecording();
});

// Cleanup on unmount
onUnmounted(() => {
  cleanup();
  if (mediaRecorder.value && isRecording.value) {
    mediaRecorder.value.stop();
  }
});
</script>

<template>
  <div class="voice-recorder">
    <!-- Recording indicator and time -->
    <div class="recorder-header">
      <div class="recording-indicator">
        <span class="pulse-dot"></span>
        <span class="recording-label">Recording</span>
      </div>
      <span class="recording-time">{{ formatTime(recordingTime) }}</span>
    </div>

    <!-- Waveform visualization -->
    <div class="waveform-container">
      <canvas ref="canvasRef" class="waveform-canvas"></canvas>
    </div>

    <!-- Controls -->
    <div class="recorder-controls">
      <button
        type="button"
        @click="cancelRecording"
        class="btn-cancel"
      >
        <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
        </svg>
        Cancel
      </button>
      <button
        type="button"
        @click="stopRecording"
        class="btn-stop"
      >
        <svg class="w-4 h-4" fill="currentColor" viewBox="0 0 24 24">
          <rect x="6" y="6" width="12" height="12" rx="2" />
        </svg>
        Stop Recording
      </button>
    </div>
  </div>
</template>

<style scoped>
.voice-recorder {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  padding: 1rem;
  background: var(--color-surface-alt);
  border-radius: 0.75rem;
  border: 1px solid var(--color-default);
}

.recorder-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.recording-indicator {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.pulse-dot {
  width: 10px;
  height: 10px;
  background: #EF4444;
  border-radius: 50%;
  animation: pulse 1.5s ease-in-out infinite;
  box-shadow: 0 0 0 0 rgba(239, 68, 68, 0.4);
}

@keyframes pulse {
  0%, 100% {
    transform: scale(1);
    box-shadow: 0 0 0 0 rgba(239, 68, 68, 0.4);
  }
  50% {
    transform: scale(1.1);
    box-shadow: 0 0 0 8px rgba(239, 68, 68, 0);
  }
}

.recording-label {
  font-size: 0.875rem;
  font-weight: 500;
  color: var(--color-primary);
}

.recording-time {
  font-size: 0.875rem;
  font-weight: 600;
  font-family: ui-monospace, monospace;
  color: var(--color-secondary);
  background: var(--color-surface);
  padding: 0.25rem 0.75rem;
  border-radius: 9999px;
  border: 1px solid var(--color-default);
}

.waveform-container {
  position: relative;
  width: 100%;
  height: 80px;
  border-radius: 0.5rem;
  overflow: hidden;
  background: var(--color-surface);
  border: 1px solid var(--color-default);
}

.waveform-canvas {
  width: 100%;
  height: 100%;
  display: block;
}

.recorder-controls {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 0.5rem;
}

.btn-cancel {
  display: flex;
  align-items: center;
  gap: 0.375rem;
  padding: 0.5rem 0.875rem;
  font-size: 0.875rem;
  font-weight: 500;
  color: var(--color-secondary);
  background: transparent;
  border: 1px solid var(--color-default);
  border-radius: 0.5rem;
  cursor: pointer;
  transition: all 0.15s ease;
}

.btn-cancel:hover {
  color: var(--color-primary);
  background: var(--color-surface-hover);
  border-color: var(--color-strong);
}

.btn-stop {
  display: flex;
  align-items: center;
  gap: 0.375rem;
  padding: 0.5rem 1rem;
  font-size: 0.875rem;
  font-weight: 500;
  color: white;
  background: var(--color-brand-blue);
  border: none;
  border-radius: 0.5rem;
  cursor: pointer;
  transition: all 0.15s ease;
}

.btn-stop:hover {
  opacity: 0.9;
  transform: translateY(-1px);
}

.btn-stop:active {
  transform: translateY(0);
}

/* Dark mode adjustments */
:global(.dark) .waveform-container {
  background: #1a1f2e;
}
</style>
