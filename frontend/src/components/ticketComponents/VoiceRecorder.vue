<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';

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

// Oscilloscope visualization state
const audioContext = ref<AudioContext | null>(null);
const analyser = ref<AnalyserNode | null>(null);
const canvasRef = ref<HTMLCanvasElement | null>(null);
const animationFrameId = ref<number | null>(null);

// Format recording time
const formatTime = (seconds: number): string => {
  const mins = Math.floor(seconds / 60);
  const secs = seconds % 60;
  return `${mins}:${secs.toString().padStart(2, "0")}`;
};

// Setup oscilloscope visualization for recording
const setupOscilloscope = (stream: MediaStream) => {
  audioContext.value = new AudioContext();
  analyser.value = audioContext.value.createAnalyser();
  const source = audioContext.value.createMediaStreamSource(stream);
  source.connect(analyser.value);
  
  analyser.value.fftSize = 2048;
  analyser.value.smoothingTimeConstant = 0.6;
  
  drawOscilloscope();
};

const drawOscilloscope = () => {
  if (!canvasRef.value || !analyser.value) return;

  const canvas = canvasRef.value;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  const width = canvas.width;
  const height = canvas.height;
  const bufferLength = analyser.value.frequencyBinCount;
  const numBars = 32;
  const barWidth = width / numBars;
  const dataArray = new Uint8Array(bufferLength);
  
  let prevBarHeights = new Array(numBars).fill(0);

  const draw = () => {
    animationFrameId.value = requestAnimationFrame(draw);
    analyser.value!.getByteTimeDomainData(dataArray);

    ctx.fillStyle = "#1D293D";
    ctx.fillRect(0, 0, width, height);

    const step = Math.floor(bufferLength / numBars);
    const amplitudeMultiplier = 2.5;
    
    for (let i = 0; i < numBars; i++) {
      let rms = 0;
      const startIndex = i * step;
      for (let j = 0; j < step; j++) {
        const value = (dataArray[startIndex + j] - 128) / 128;
        rms += value * value;
      }
      rms = Math.sqrt(rms / step);
      
      const amplitude = Math.pow(rms, 0.7) * amplitudeMultiplier;
      
      let targetHeight = amplitude * height;
      targetHeight = Math.min(targetHeight, height * 0.95);
      
      const easingFactor = amplitude > prevBarHeights[i] ? 0.3 : 0.15;
      prevBarHeights[i] = prevBarHeights[i] + (targetHeight - prevBarHeights[i]) * easingFactor;
      
      const barHeight = prevBarHeights[i];
      const x = i * barWidth;
      const y = height - barHeight;

      const gradient = ctx.createLinearGradient(x, y, x, height);
      gradient.addColorStop(0, '#60A5FA');
      gradient.addColorStop(0.6, '#3B82F6');
      gradient.addColorStop(1, '#1D4ED8');
      
      ctx.fillStyle = gradient;
      ctx.beginPath();
      ctx.moveTo(x + 2, height);
      ctx.lineTo(x + 2, y + 4);
      ctx.quadraticCurveTo(x + 2, y, x + barWidth/2, y);
      ctx.quadraticCurveTo(x + barWidth - 2, y, x + barWidth - 2, y + 4);
      ctx.lineTo(x + barWidth - 2, height);
      ctx.fill();

      ctx.shadowColor = '#60A5FA';
      ctx.shadowBlur = 8;
      ctx.shadowOffsetX = 0;
      ctx.shadowOffsetY = 0;
      ctx.stroke();
      ctx.shadowBlur = 0;
    }
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
    setupOscilloscope(stream);
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
  <div class="flex flex-col items-center gap-4 p-3 bg-surface-alt rounded-lg">
    <div class="flex items-center gap-2 w-full">
      <span class="animate-pulse text-red-500">●</span>
      <span class="text-primary">{{ formatTime(recordingTime) }}</span>
    </div>
    <canvas ref="canvasRef" class="w-full h-16 rounded-lg bg-surface" width="400" height="64"></canvas>
    <div class="flex items-center gap-2 ml-auto">
      <button
        type="button"
        @click="cancelRecording"
        class="px-3 py-1.5 text-secondary hover:text-primary transition-colors"
      >
        Cancel
      </button>
      <button
        type="button"
        @click="stopRecording"
        class="px-3 py-1.5 bg-blue-500 text-white text-sm rounded hover:bg-blue-600 transition-colors"
      >
        Stop Recording
      </button>
    </div>
  </div>
</template>

<style scoped>
canvas {
  border-radius: 0.5rem;
  background: var(--bg-surface);
  width: 100%;
  height: 64px;
}
</style>