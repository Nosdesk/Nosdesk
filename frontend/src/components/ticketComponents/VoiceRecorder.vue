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

  // Number of sample points for the flowing wave
  const numPoints = 64;
  let smoothedValues = new Array(numPoints).fill(0);

  // Get theme colors from CSS variables
  const accentColor = getCSSVar('--color-accent') || '#2C80FF';

  // Parse accent color to RGB for gradient creation
  const parseColor = (color: string): { r: number; g: number; b: number } => {
    if (color.startsWith('#')) {
      const hex = color.slice(1);
      return {
        r: parseInt(hex.slice(0, 2), 16),
        g: parseInt(hex.slice(2, 4), 16),
        b: parseInt(hex.slice(4, 6), 16)
      };
    }
    // Fallback for rgb() format
    const match = color.match(/(\d+)/g);
    if (match && match.length >= 3) {
      return { r: parseInt(match[0]), g: parseInt(match[1]), b: parseInt(match[2]) };
    }
    return { r: 44, g: 128, b: 255 }; // Default blue
  };

  const draw = () => {
    animationFrameId.value = requestAnimationFrame(draw);
    analyser.value!.getByteFrequencyData(dataArray);

    // Get background color from CSS variables
    const bgColor = getCSSVar('--color-surface') || (isDarkMode.value ? '#1a1f2e' : '#f8fafc');

    // Clear canvas
    ctx.fillStyle = bgColor;
    ctx.fillRect(0, 0, width, height);

    // Calculate smoothed amplitude values
    // Voice frequencies are concentrated in the lower-mid range
    // We map the full waveform width to the first 50% of frequency bins
    const voiceFrequencyRatio = 0.5;
    for (let i = 0; i < numPoints; i++) {
      const dataIndex = Math.floor((i / numPoints) * bufferLength * voiceFrequencyRatio);
      const rawValue = dataArray[dataIndex] / 255;
      const boostedValue = Math.pow(rawValue, 0.5) * 1.5;
      const targetValue = Math.min(1, boostedValue);
      // Smooth transitions for organic motion
      smoothedValues[i] += (targetValue - smoothedValues[i]) * 0.2;
    }

    const maxAmplitude = height * 0.4;
    const rgb = parseColor(accentColor);

    // Create gradient for the filled area
    const gradient = ctx.createLinearGradient(0, centerY - maxAmplitude, 0, centerY + maxAmplitude);
    gradient.addColorStop(0, `rgba(${rgb.r}, ${rgb.g}, ${rgb.b}, 0.6)`);
    gradient.addColorStop(0.5, `rgba(${rgb.r}, ${rgb.g}, ${rgb.b}, 0.8)`);
    gradient.addColorStop(1, `rgba(${rgb.r}, ${rgb.g}, ${rgb.b}, 0.6)`);

    // Helper: get smooth Y value using cubic interpolation
    const getY = (index: number): number => {
      const i = Math.floor(index);
      const t = index - i;
      const p0 = smoothedValues[Math.max(0, i - 1)] || 0;
      const p1 = smoothedValues[i] || 0;
      const p2 = smoothedValues[Math.min(numPoints - 1, i + 1)] || 0;
      const p3 = smoothedValues[Math.min(numPoints - 1, i + 2)] || 0;

      // Catmull-Rom spline interpolation for smooth curves
      const t2 = t * t;
      const t3 = t2 * t;
      let amplitude = 0.5 * (
        (2 * p1) +
        (-p0 + p2) * t +
        (2 * p0 - 5 * p1 + 4 * p2 - p3) * t2 +
        (-p0 + 3 * p1 - 3 * p2 + p3) * t3
      );

      // Fade edges to zero for smooth left/right transitions
      const edgeFadeWidth = 4; // Number of points to fade over
      const actualIndex = i + t;
      if (actualIndex < edgeFadeWidth) {
        amplitude *= actualIndex / edgeFadeWidth;
      } else if (actualIndex > numPoints - 1 - edgeFadeWidth) {
        amplitude *= (numPoints - 1 - actualIndex) / edgeFadeWidth;
      }

      return amplitude;
    };

    // Draw flowing filled area (mirrored from center)
    ctx.save();
    ctx.fillStyle = gradient;
    ctx.beginPath();

    const step = width / (numPoints - 1);

    // Start at top-left corner with first amplitude
    const firstAmplitude = getY(0);
    const firstY = centerY - firstAmplitude * maxAmplitude;
    ctx.moveTo(0, firstY);

    // Draw top edge with smooth curves
    for (let i = 1; i < numPoints; i++) {
      const x = i * step;
      const amplitude = getY(i);
      const y = centerY - amplitude * maxAmplitude;

      // Use quadratic curves for smoother lines
      const prevX = (i - 1) * step;
      const prevAmplitude = getY(i - 1);
      const prevY = centerY - prevAmplitude * maxAmplitude;
      const cpX = (prevX + x) / 2;
      ctx.quadraticCurveTo(prevX, prevY, cpX, (prevY + y) / 2);
    }

    // Finish the last segment to the right edge
    const lastAmplitude = getY(numPoints - 1);
    const lastTopY = centerY - lastAmplitude * maxAmplitude;
    ctx.lineTo(width, lastTopY);

    // Draw bottom edge (mirrored) - start from right
    const lastBottomY = centerY + lastAmplitude * maxAmplitude;
    ctx.lineTo(width, lastBottomY);

    // Draw bottom edge going left
    for (let i = numPoints - 2; i >= 0; i--) {
      const x = i * step;
      const amplitude = getY(i);
      const y = centerY + amplitude * maxAmplitude;

      const nextX = (i + 1) * step;
      const nextAmplitude = getY(i + 1);
      const nextY = centerY + nextAmplitude * maxAmplitude;
      const cpX = (nextX + x) / 2;
      ctx.quadraticCurveTo(nextX, nextY, cpX, (nextY + y) / 2);
    }

    // Close back to start
    const firstBottomY = centerY + firstAmplitude * maxAmplitude;
    ctx.lineTo(0, firstBottomY);

    ctx.closePath();
    ctx.fill();
    ctx.restore();

    // Draw glowing center line that pulses with audio
    const avgAmplitude = smoothedValues.reduce((a, b) => a + b, 0) / numPoints;
    ctx.save();
    ctx.strokeStyle = accentColor;
    ctx.lineWidth = 1.5 + avgAmplitude * 2;
    ctx.globalAlpha = 0.4 + avgAmplitude * 0.4;

    if (avgAmplitude > 0.1) {
      ctx.shadowColor = accentColor;
      ctx.shadowBlur = avgAmplitude * 15;
    }

    ctx.beginPath();

    // Start at left edge with first amplitude's wave offset
    const firstWaveOffset = getY(0) * 3;
    ctx.moveTo(0, centerY + firstWaveOffset);

    // Draw flowing center line
    for (let i = 1; i < numPoints; i++) {
      const x = i * step;
      const amplitude = getY(i);
      const waveOffset = amplitude * 3;

      const prevX = (i - 1) * step;
      const prevAmplitude = getY(i - 1);
      const prevWaveOffset = prevAmplitude * 3;
      const cpX = (prevX + x) / 2;
      ctx.quadraticCurveTo(prevX, centerY + prevWaveOffset, cpX, centerY + (prevWaveOffset + waveOffset) / 2);
    }

    // Finish to right edge
    const lastWaveOffset = getY(numPoints - 1) * 3;
    ctx.lineTo(width, centerY + lastWaveOffset);

    ctx.stroke();
    ctx.restore();
  };

  draw();
};

// Get the best supported audio MIME type for recording
const getSupportedMimeType = (): string => {
  // Prefer formats in order of compatibility and quality
  const types = [
    'audio/webm;codecs=opus',  // Best quality, Chrome/Firefox/Edge
    'audio/webm',               // Fallback WebM
    'audio/mp4',                // iOS Safari
    'audio/aac',                // iOS fallback
    'audio/mpeg',               // Broad compatibility
    'audio/ogg;codecs=opus',    // Firefox
  ];

  for (const type of types) {
    if (MediaRecorder.isTypeSupported(type)) {
      return type;
    }
  }

  // Return empty string to let browser choose default
  return '';
};

// Track the MIME type used for recording
const recordingMimeType = ref<string>('');

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

    // Detect and use the best supported MIME type
    recordingMimeType.value = getSupportedMimeType();
    const recorderOptions: MediaRecorderOptions = {};
    if (recordingMimeType.value) {
      recorderOptions.mimeType = recordingMimeType.value;
    }

    mediaRecorder.value = new MediaRecorder(stream, recorderOptions);
    // Store the actual MIME type being used (browser may adjust it)
    recordingMimeType.value = mediaRecorder.value.mimeType || recordingMimeType.value || 'audio/webm';

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
      // Use the actual MIME type from recording, not hardcoded
      const audioBlob = new Blob(audioChunks.value, { type: recordingMimeType.value });

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
  background: var(--color-accent);
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

/* Waveform container uses theme surface color automatically */
</style>
