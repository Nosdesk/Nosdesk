<script setup lang="ts">
/**
 * Accent colour picker on Reka's colour primitives: a hue slider with a
 * preview swatch and a hex field, and a disclosure with preset swatches,
 * saturation and lightness sliders. Reka owns the parts a hand-rolled
 * one gets wrong: each slider is a `role=slider` thumb with arrow, Home
 * and End keys, `aria-valuetext` in words; the hex field parses hex,
 * rgb() and hsl() and commits on blur or Enter; the presets are a radio
 * group of named swatches (not Reka's ColorSwatchPicker: its listbox
 * moves focus to a swatch whenever the value changes from outside,
 * which here is every slider step).
 *
 * Value contract is unchanged: a lowercase six-digit hex, emitted only
 * on a change. A value the picker cannot parse is shown as the default
 * accent and left alone. Alpha is dropped at the boundary.
 *
 * The hue track shows the colours the user would actually get at the
 * current saturation and lightness rather than the full-strength
 * rainbow, so the accent presets read true.
 */
import { computed, ref, useId, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import {
  CollapsibleContent,
  CollapsibleRoot,
  CollapsibleTrigger,
  ColorFieldInput,
  ColorFieldRoot,
  ColorSliderRoot,
  ColorSliderThumb,
  ColorSliderTrack,
  ColorSwatch,
  RadioGroupItem,
  RadioGroupRoot,
  colorToString,
  convertToHsl,
  parseColor,
} from 'reka-ui';
import { useThemeStore } from '@/stores/theme';
import { ACCENT_LIGHTNESS, ACCENT_SATURATION, accentColorFromHue, hslToHex } from '@nosdesk/core/utils/accentColor';

const props = withDefaults(defineProps<{
  modelValue: string;
  label?: string;
  /** Inline = swatch, slider, and hex on one row (sm+). Stacked = hex on its own row. */
  layout?: 'inline' | 'stacked';
  /** Hide the swatch when another live preview (e.g. CollectionIcon) is shown nearby. */
  hideSwatch?: boolean;
}>(), {
  layout: 'inline',
  hideSwatch: false,
});

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void;
}>();

const themeStore = useThemeStore();
const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const generatedId = useId();
const labelId = computed(() => (props.label ? `color-picker-${generatedId}-label` : undefined));
const panelId = `color-picker-${generatedId}-panel`;

// ---- Value -----------------------------------------------------------

// Lowercase six-digit hex from anything Reka can parse; alpha dropped.
function normalizeHex(value: string | undefined): string | undefined {
  if (!value) return undefined;
  try {
    return colorToString(parseColor(value.trim()), 'hex').slice(0, 7);
  } catch {
    return undefined;
  }
}

const FALLBACK = accentColorFromHue(0);

const hex = ref(normalizeHex(props.modelValue) ?? FALLBACK);
watch(
  () => props.modelValue,
  (value) => {
    const next = normalizeHex(value);
    if (next && next !== hex.value) hex.value = next;
  },
);

// Every part writes through here with a hex string.
function onUpdate(value: unknown): void {
  if (typeof value !== 'string') return;
  const next = normalizeHex(value);
  if (!next || next === hex.value) return;
  hex.value = next;
  emit('update:modelValue', next);
}

const hsl = computed(() => {
  const { h, s, l } = convertToHsl(parseColor(hex.value));
  return { h: Math.round(h), s: Math.round(s), l: Math.round(l) };
});

const isDefaultTone = computed(() => hsl.value.s === ACCENT_SATURATION && hsl.value.l === ACCENT_LIGHTNESS);

function resetToDefaults(): void {
  onUpdate(hslToHex(hsl.value.h, ACCENT_SATURATION, ACCENT_LIGHTNESS));
}

// ---- Names -----------------------------------------------------------

// Human-readable colour name from hue; spoken with every slider value
// and shown for themes whose track does not represent real colours.
const HUE_NAMES: Array<[number, string]> = [
  [15, 'color-red'],
  [45, 'color-orange'],
  [70, 'color-yellow'],
  [150, 'color-green'],
  [190, 'color-cyan'],
  [260, 'color-blue'],
  [290, 'color-purple'],
  [330, 'color-pink'],
  [360, 'color-red'],
];

const hueName = (h: number): string => t(HUE_NAMES.find(([limit]) => h < limit)?.[1] ?? 'color-red');

const colorName = computed(() => hueName(hsl.value.h));

const themeId = computed(() => themeStore.effectiveTheme.meta.id);
const showColorName = computed(
  () => themeId.value === 'epaper' || themeId.value === 'red-horizon' || themeStore.effectiveColorBlindMode,
);

// One preset per named hue at the accent tone.
const PRESET_HUES = [0, 30, 55, 120, 180, 220, 275, 320];
const presets = computed(() => PRESET_HUES.map((h) => ({ hex: accentColorFromHue(h), name: hueName(h) })));

// ---- Tracks ----------------------------------------------------------

// Reka's hue track is the full-strength rainbow; ours is drawn at the
// current tone so it shows the colours on offer.
const hueGradient = computed(() => {
  const { s, l } = hsl.value;
  const stops = [0, 60, 120, 180, 240, 300, 360].map((h) => `hsl(${h}, ${s}%, ${l}%)`);
  return `linear-gradient(to right, ${stops.join(', ')})`;
});

const isExpanded = ref(false);

const thumbClass =
  'block rounded-full bg-white border-gray-800 shadow-lg outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-1';
</script>

<template>
  <div class="flex flex-col gap-2" role="group" :aria-labelledby="labelId">
    <!-- Label row with optional color name -->
    <div v-if="label" class="flex items-center justify-between">
      <span :id="labelId" class="text-sm font-medium text-primary">{{ label }}</span>
      <span v-if="showColorName" class="text-sm text-secondary" aria-hidden="true">
        {{ colorName }}
      </span>
    </div>

    <CollapsibleRoot v-model:open="isExpanded" class="flex flex-col gap-2">
      <!-- Main row -->
      <div
        class="flex gap-3"
        :class="layout === 'stacked' ? 'flex-col items-stretch' : 'flex-col sm:flex-row items-stretch sm:items-center'"
      >
        <div class="flex items-center gap-3 flex-1 min-w-0">
          <!-- Preview swatch, and the disclosure for the finer controls. -->
          <CollapsibleTrigger v-if="!hideSwatch" as-child>
            <button
              type="button"
              :aria-controls="panelId"
              :aria-label="isExpanded ? t('color-picker-collapse') : t('color-picker-expand')"
              class="relative w-10 h-10 rounded-lg border border-default shadow-sm shrink-0 cursor-pointer hover:ring-2 hover:ring-accent hover:ring-offset-1 transition-shadow group data-[state=open]:ring-2 data-[state=open]:ring-accent data-[state=open]:ring-offset-1 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-1"
            >
              <!-- Reka exposes the colour as a CSS variable; painting is ours. -->
              <ColorSwatch
                :color="hex"
                :label="colorName"
                aria-hidden="true"
                class="absolute inset-0 rounded-lg bg-[var(--reka-color-swatch-color)]"
              />
              <span
                class="absolute -bottom-0.5 -right-0.5 w-4 h-4 bg-surface border border-default rounded-full flex items-center justify-center shadow-sm transition-transform"
                :class="isExpanded ? 'rotate-180' : 'group-hover:scale-110'"
                aria-hidden="true"
              >
                <svg class="w-2.5 h-2.5 text-secondary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
                </svg>
              </span>
            </button>
          </CollapsibleTrigger>

          <!-- Hue slider -->
          <ColorSliderRoot
            :model-value="hex"
            channel="hue"
            class="relative flex flex-1 min-w-[5.5rem] h-10 items-center touch-none select-none"
            @update:model-value="onUpdate"
          >
            <ColorSliderTrack
              class="relative h-full w-full rounded-lg border border-default"
              :style="{ background: hueGradient }"
            />
            <ColorSliderThumb
              :class="[thumbClass, 'w-2 h-8 border-2']"
              :aria-label="t('color-slider-hue')"
              :aria-valuetext="t('color-slider-hue-value', { name: colorName, degrees: hsl.h })"
            />
          </ColorSliderRoot>
        </div>

        <!-- Hex field: parses hex, rgb() and hsl(); commits on blur or Enter. -->
        <ColorFieldRoot
          :model-value="hex"
          disable-wheel-change
          :class="layout === 'stacked' ? 'w-full' : 'w-full sm:w-24'"
          @update:model-value="onUpdate"
        >
          <ColorFieldInput
            :aria-label="t('color-hex-label')"
            placeholder="#6366f1"
            class="w-full px-3 py-2 bg-surface-alt border border-default rounded-lg text-primary text-sm font-mono focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent uppercase"
            :class="layout === 'stacked' ? 'text-left' : 'text-center sm:text-left'"
          />
        </ColorFieldRoot>
      </div>

      <!-- Finer controls -->
      <CollapsibleContent as-child>
        <div :id="panelId" class="flex flex-col gap-3 p-3 bg-surface-alt border border-default rounded-lg">
          <!-- Presets: one per named hue at the accent tone. None is
               checked while the colour is off-preset. -->
          <RadioGroupRoot
            :model-value="hex"
            orientation="horizontal"
            loop
            :aria-label="t('color-presets-label')"
            class="flex flex-wrap gap-2"
            @update:model-value="onUpdate"
          >
            <RadioGroupItem
              v-for="preset in presets"
              :key="preset.hex"
              :value="preset.hex"
              :aria-label="preset.name"
              class="w-7 h-7 rounded-lg border border-default cursor-pointer outline-none ring-offset-1 hover:ring-2 hover:ring-accent data-[state=checked]:ring-2 data-[state=checked]:ring-accent focus-visible:ring-2 focus-visible:ring-accent"
            >
              <ColorSwatch
                :color="preset.hex"
                :label="preset.name"
                aria-hidden="true"
                class="block w-full h-full rounded-lg bg-[var(--reka-color-swatch-color)]"
              />
            </RadioGroupItem>
          </RadioGroupRoot>

          <!-- Saturation slider -->
          <div class="flex items-center gap-3">
            <span class="text-xs text-secondary w-6 shrink-0" aria-hidden="true">S</span>
            <ColorSliderRoot
              :model-value="hex"
              channel="saturation"
              class="relative flex flex-1 h-6 items-center touch-none select-none"
              @update:model-value="onUpdate"
            >
              <ColorSliderTrack class="relative h-full w-full rounded border border-default" />
              <ColorSliderThumb
                :class="[thumbClass, 'w-1.5 h-5 border']"
                :aria-label="t('color-slider-saturation')"
                :aria-valuetext="t('color-slider-percent-value', { value: hsl.s })"
              />
            </ColorSliderRoot>
            <span class="text-xs text-tertiary w-8 text-right font-mono" aria-hidden="true">{{ hsl.s }}%</span>
          </div>

          <!-- Lightness slider -->
          <div class="flex items-center gap-3">
            <span class="text-xs text-secondary w-6 shrink-0" aria-hidden="true">L</span>
            <ColorSliderRoot
              :model-value="hex"
              channel="lightness"
              class="relative flex flex-1 h-6 items-center touch-none select-none"
              @update:model-value="onUpdate"
            >
              <ColorSliderTrack class="relative h-full w-full rounded border border-default" />
              <ColorSliderThumb
                :class="[thumbClass, 'w-1.5 h-5 border']"
                :aria-label="t('color-slider-lightness')"
                :aria-valuetext="t('color-slider-percent-value', { value: hsl.l })"
              />
            </ColorSliderRoot>
            <span class="text-xs text-tertiary w-8 text-right font-mono" aria-hidden="true">{{ hsl.l }}%</span>
          </div>

          <button
            type="button"
            :disabled="isDefaultTone"
            class="text-xs text-secondary hover:text-primary disabled:opacity-50 disabled:cursor-not-allowed transition-colors self-end"
            @click="resetToDefaults"
          >
            {{ t('color-picker-reset') }}
          </button>
        </div>
      </CollapsibleContent>
    </CollapsibleRoot>
  </div>
</template>
