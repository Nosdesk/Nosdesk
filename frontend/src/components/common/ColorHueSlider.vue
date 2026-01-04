<script setup lang="ts">
import { ref, computed, watch, onMounted } from 'vue';
import { useThemeStore } from '@/stores/theme';

const props = defineProps<{
  modelValue: string;
  label?: string;
}>();

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void;
}>();

const themeStore = useThemeStore();

// Theme-specific color constraints
const themeId = computed(() => themeStore.effectiveTheme.meta.id);
const isEpaper = computed(() => themeId.value === 'epaper');
const isRedHorizon = computed(() => themeId.value === 'red-horizon');

// Preset S/L values for accessible, theme-appropriate colors
// S:70% = vibrant but balanced, L:45% = WCAG AA compliant with white text
const THEME_SATURATION = 70;
const THEME_LIGHTNESS = 45;

// Show color name label for themes where the gradient doesn't represent actual colors
const showColorName = computed(() =>
  isEpaper.value || isRedHorizon.value || themeStore.effectiveColorBlindMode
);

// Get human-readable color name from hue
function getColorName(h: number): string {
  if (h < 15) return 'Red';
  if (h < 45) return 'Orange';
  if (h < 70) return 'Yellow';
  if (h < 150) return 'Green';
  if (h < 190) return 'Cyan';
  if (h < 260) return 'Blue';
  if (h < 290) return 'Purple';
  if (h < 330) return 'Pink';
  return 'Red';
}

const colorName = computed(() => getColorName(hue.value));

// Color state in HSL
const hue = ref(0);
const saturation = ref(THEME_SATURATION);
const lightness = ref(THEME_LIGHTNESS);

const isDragging = ref(false);
const isExpanded = ref(false);

// Convert hex to HSL (returns theme defaults if invalid)
function hexToHsl(hex: string): { h: number; s: number; l: number } {
  const result = /^#?([a-f\d]{2})([a-f\d]{2})([a-f\d]{2})$/i.exec(hex);
  if (!result) return { h: 0, s: THEME_SATURATION, l: THEME_LIGHTNESS };

  const r = parseInt(result[1], 16) / 255;
  const g = parseInt(result[2], 16) / 255;
  const b = parseInt(result[3], 16) / 255;

  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;

  if (max === min) return { h: 0, s: 0, l: Math.round(l * 100) };

  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);

  let h = 0;
  if (max === r) h = ((g - b) / d + (g < b ? 6 : 0)) / 6;
  else if (max === g) h = ((b - r) / d + 2) / 6;
  else h = ((r - g) / d + 4) / 6;

  return { h: Math.round(h * 360), s: Math.round(s * 100), l: Math.round(l * 100) };
}

// Convert HSL to hex
function hslToHex(h: number, s: number, l: number): string {
  s /= 100;
  l /= 100;

  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs((h / 60) % 2 - 1));
  const m = l - c / 2;

  let r = 0, g = 0, b = 0;
  if (h < 60) { r = c; g = x; }
  else if (h < 120) { r = x; g = c; }
  else if (h < 180) { g = c; b = x; }
  else if (h < 240) { g = x; b = c; }
  else if (h < 300) { r = x; b = c; }
  else { r = c; b = x; }

  const toHex = (n: number) => Math.round((n + m) * 255).toString(16).padStart(2, '0');
  return `#${toHex(r)}${toHex(g)}${toHex(b)}`;
}

const hexValue = computed(() => hslToHex(hue.value, saturation.value, lightness.value));
const huePosition = computed(() => (hue.value / 360) * 100);

// Dynamic hue gradient based on current S/L values
const hueGradient = computed(() => {
  const s = saturation.value;
  const l = lightness.value;
  // Full hue spectrum for all themes (ePaper may support color, Red Horizon uses CSS filter)
  return `linear-gradient(to right, hsl(0, ${s}%, ${l}%), hsl(60, ${s}%, ${l}%), hsl(120, ${s}%, ${l}%), hsl(180, ${s}%, ${l}%), hsl(240, ${s}%, ${l}%), hsl(300, ${s}%, ${l}%), hsl(360, ${s}%, ${l}%))`;
});

// Sync from prop
onMounted(() => {
  if (props.modelValue) {
    const hsl = hexToHsl(props.modelValue);
    hue.value = hsl.h;
    saturation.value = hsl.s;
    lightness.value = hsl.l;
  }
});

watch(() => props.modelValue, (val) => {
  if (val && !isDragging.value) {
    const hsl = hexToHsl(val);
    if (Math.abs(hsl.h - hue.value) > 5 || Math.abs(hsl.s - saturation.value) > 5 || Math.abs(hsl.l - lightness.value) > 5) {
      hue.value = hsl.h;
      saturation.value = hsl.s;
      lightness.value = hsl.l;
    }
  }
});

// Emit on change
watch([hue, saturation, lightness], () => emit('update:modelValue', hexValue.value));

function onHueInput(e: Event) {
  hue.value = parseInt((e.target as HTMLInputElement).value);
}

function onSaturationInput(e: Event) {
  saturation.value = parseInt((e.target as HTMLInputElement).value);
}

function onLightnessInput(e: Event) {
  lightness.value = parseInt((e.target as HTMLInputElement).value);
}

function onHexInput(e: Event) {
  const val = (e.target as HTMLInputElement).value;
  if (/^#[0-9A-Fa-f]{6}$/.test(val)) {
    const hsl = hexToHsl(val);
    hue.value = hsl.h;
    saturation.value = hsl.s;
    lightness.value = hsl.l;
  }
}

function toggleExpanded() {
  isExpanded.value = !isExpanded.value;
}

function resetToDefaults() {
  saturation.value = THEME_SATURATION;
  lightness.value = THEME_LIGHTNESS;
}

function startDrag() {
  isDragging.value = true;
}

function endDrag() {
  isDragging.value = false;
}
</script>

<template>
  <div class="flex flex-col gap-2">
    <!-- Label row with optional color name -->
    <div v-if="label" class="flex items-center justify-between">
      <label class="text-sm font-medium text-primary">{{ label }}</label>
      <span v-if="showColorName" class="text-sm text-secondary">
        {{ colorName }}
      </span>
    </div>

    <!-- Main row -->
    <div class="flex flex-col sm:flex-row items-stretch sm:items-center gap-3">
      <div class="flex items-center gap-3 flex-1 min-w-0">
        <!-- Color preview (clickable to expand) -->
        <button
          type="button"
          @click="toggleExpanded"
          class="relative w-10 h-10 rounded-lg border border-default shadow-sm shrink-0 cursor-pointer hover:ring-2 hover:ring-accent hover:ring-offset-1 transition-shadow group"
          :class="isExpanded ? 'ring-2 ring-accent ring-offset-1' : ''"
          :style="{ backgroundColor: hexValue }"
          :title="isExpanded ? 'Collapse color picker' : 'Expand for more colors'"
        >
          <!-- Expand/collapse indicator -->
          <span
            class="absolute -bottom-0.5 -right-0.5 w-4 h-4 bg-surface border border-default rounded-full flex items-center justify-center shadow-sm transition-transform"
            :class="isExpanded ? 'rotate-180' : 'group-hover:scale-110'"
          >
            <svg class="w-2.5 h-2.5 text-secondary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
            </svg>
          </span>
        </button>

        <!-- Hue slider track -->
        <div class="flex-1 relative h-10 slider-container">
          <div
            class="absolute inset-0 rounded-lg border border-default"
            :style="{ background: hueGradient }"
          />

          <!-- Thumb wrapper -->
          <div
            class="absolute inset-y-1 left-0 right-2 pointer-events-none"
            :class="isDragging ? '' : 'transition-transform duration-75 ease-out'"
            :style="{ transform: `translateX(${huePosition}%)` }"
          >
            <div class="w-2 h-full rounded-full bg-white border-2 border-gray-800 shadow-lg" />
          </div>

          <!-- Range input -->
          <input
            type="range"
            min="0"
            max="360"
            :value="hue"
            @input="onHueInput"
            @pointerdown="startDrag"
            @pointerup="endDrag"
            @pointercancel="endDrag"
            class="absolute inset-0 w-full h-full opacity-0 cursor-pointer touch-none"
          />
        </div>
      </div>

      <!-- Hex input -->
      <input
        :value="hexValue"
        @input="onHexInput"
        type="text"
        maxlength="7"
        placeholder="#6366f1"
        class="w-full sm:w-24 px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary text-sm font-mono focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent uppercase text-center sm:text-left"
      />
    </div>

    <!-- Expanded controls -->
    <div
      v-if="isExpanded"
      class="flex flex-col gap-3 p-3 bg-surface-alt border border-default rounded-lg"
    >
      <!-- Saturation slider -->
      <div class="flex items-center gap-3">
        <span class="text-xs text-secondary w-6 shrink-0">S</span>
        <div class="flex-1 relative h-6 slider-container">
          <div
            class="absolute inset-0 rounded border border-default"
            :style="{ background: `linear-gradient(to right, ${hslToHex(hue, 0, lightness)}, ${hslToHex(hue, 100, lightness)})` }"
          />
          <div
            class="absolute inset-y-0.5 left-0 right-1.5 pointer-events-none"
            :class="isDragging ? '' : 'transition-transform duration-75 ease-out'"
            :style="{ transform: `translateX(${saturation}%)` }"
          >
            <div class="w-1.5 h-full rounded-full bg-white border border-gray-800 shadow" />
          </div>
          <input
            type="range"
            min="0"
            max="100"
            :value="saturation"
            @input="onSaturationInput"
            @pointerdown="startDrag"
            @pointerup="endDrag"
            @pointercancel="endDrag"
            class="absolute inset-0 w-full h-full opacity-0 cursor-pointer touch-none"
          />
        </div>
        <span class="text-xs text-tertiary w-8 text-right font-mono">{{ saturation }}%</span>
      </div>

      <!-- Lightness slider -->
      <div class="flex items-center gap-3">
        <span class="text-xs text-secondary w-6 shrink-0">L</span>
        <div class="flex-1 relative h-6 slider-container">
          <div
            class="absolute inset-0 rounded border border-default"
            :style="{ background: `linear-gradient(to right, #000, ${hslToHex(hue, saturation, 50)}, #fff)` }"
          />
          <div
            class="absolute inset-y-0.5 left-0 right-1.5 pointer-events-none"
            :class="isDragging ? '' : 'transition-transform duration-75 ease-out'"
            :style="{ transform: `translateX(${lightness}%)` }"
          >
            <div class="w-1.5 h-full rounded-full bg-white border border-gray-800 shadow" />
          </div>
          <input
            type="range"
            min="0"
            max="100"
            :value="lightness"
            @input="onLightnessInput"
            @pointerdown="startDrag"
            @pointerup="endDrag"
            @pointercancel="endDrag"
            class="absolute inset-0 w-full h-full opacity-0 cursor-pointer touch-none"
          />
        </div>
        <span class="text-xs text-tertiary w-8 text-right font-mono">{{ lightness }}%</span>
      </div>

      <!-- Reset button -->
      <button
        type="button"
        @click="resetToDefaults"
        :disabled="saturation === THEME_SATURATION && lightness === THEME_LIGHTNESS"
        class="text-xs text-secondary hover:text-primary disabled:opacity-50 disabled:cursor-not-allowed transition-colors self-end"
      >
        Reset to defaults
      </button>
    </div>
  </div>
</template>

<style scoped>
.slider-container {
  contain: layout style;
}

.slider-container > div:nth-child(2) {
  will-change: transform;
  backface-visibility: hidden;
}

input[type="range"] {
  -webkit-appearance: none;
  appearance: none;
}
</style>
