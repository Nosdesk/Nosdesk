<script setup lang="ts">
import { computed } from 'vue'
import type { Theme } from '@/themes'
import { useThemeStore } from '@/stores/theme'

const props = defineProps<{
  theme?: Theme
  isSystem?: boolean
  selected?: boolean
  disabled?: boolean
}>()

const themeStore = useThemeStore()

// Check if we should apply the CRT scanline effect (when current theme is red-horizon)
const showCrtEffect = computed(() => {
  const currentThemeId = themeStore.effectiveTheme?.meta?.id
  return currentThemeId === 'red-horizon' && !props.selected
})

const emit = defineEmits<{
  select: []
}>()

// Get preview colors from theme or use system split preview
const getPreviewColors = () => {
  if (props.isSystem) {
    return null // System uses split preview
  }
  if (!props.theme) {
    return null
  }
  return {
    app: props.theme.colors.app,
    surface: props.theme.colors.surface,
    surfaceAlt: props.theme.colors.surfaceAlt,
    accent: props.theme.colors.accent,
    primary: props.theme.colors.primary,
    secondary: props.theme.colors.secondary,
  }
}

const colors = getPreviewColors()
</script>

<template>
  <button
    type="button"
    :disabled="disabled"
    class="group relative flex flex-col items-center gap-2 p-2 rounded-lg border-2 transition-all duration-200 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-accent"
    :class="[
      selected
        ? 'border-accent bg-accent-muted'
        : 'border-default hover:border-strong bg-surface hover:bg-surface-hover',
      disabled ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer',
    ]"
    @click="!disabled && emit('select')"
  >
    <!-- Preview Box -->
    <div
      class="w-full aspect-[4/3] rounded-md overflow-hidden border border-subtle shadow-sm"
      :class="{ 'crt-effect': showCrtEffect }"
    >
      <!-- System Theme - Split Preview -->
      <template v-if="isSystem">
        <div class="flex h-full">
          <!-- Light half -->
          <div class="w-1/2 bg-[#f3f4f6] flex flex-col p-1.5">
            <div class="h-2 w-full bg-[#e5e7eb] rounded-sm mb-1"></div>
            <div class="flex-1 bg-white rounded-sm flex flex-col gap-0.5 p-1">
              <div class="h-1.5 w-3/4 bg-[#1f2937] rounded-sm opacity-80"></div>
              <div class="h-1 w-1/2 bg-[#6b7280] rounded-sm opacity-60"></div>
            </div>
            <div class="h-1.5 w-8 bg-[#2C80FF] rounded-sm mt-1"></div>
          </div>
          <!-- Dark half -->
          <div class="w-1/2 bg-[#0f172a] flex flex-col p-1.5">
            <div class="h-2 w-full bg-[#334155] rounded-sm mb-1"></div>
            <div class="flex-1 bg-[#1e293b] rounded-sm flex flex-col gap-0.5 p-1">
              <div class="h-1.5 w-3/4 bg-[#f9fafb] rounded-sm opacity-80"></div>
              <div class="h-1 w-1/2 bg-[#94a3b8] rounded-sm opacity-60"></div>
            </div>
            <div class="h-1.5 w-8 bg-[#2C80FF] rounded-sm mt-1"></div>
          </div>
        </div>
      </template>

      <!-- Theme Preview -->
      <template v-else-if="colors">
        <div
          class="h-full flex flex-col p-1.5"
          :style="{ backgroundColor: colors.app }"
        >
          <!-- Header bar -->
          <div
            class="h-2 w-full rounded-sm mb-1"
            :style="{ backgroundColor: colors.surfaceAlt }"
          ></div>
          <!-- Content area -->
          <div
            class="flex-1 rounded-sm flex flex-col gap-0.5 p-1"
            :style="{ backgroundColor: colors.surface }"
          >
            <div
              class="h-1.5 w-3/4 rounded-sm opacity-80"
              :style="{ backgroundColor: colors.primary }"
            ></div>
            <div
              class="h-1 w-1/2 rounded-sm opacity-60"
              :style="{ backgroundColor: colors.secondary }"
            ></div>
          </div>
          <!-- Accent button -->
          <div
            class="h-1.5 w-8 rounded-sm mt-1"
            :style="{ backgroundColor: colors.accent }"
          ></div>
        </div>
      </template>
    </div>

    <!-- Theme Name -->
    <span
      class="text-xs font-medium truncate max-w-full"
      :class="selected ? 'text-accent' : 'text-secondary group-hover:text-primary'"
    >
      {{ isSystem ? 'System' : theme?.meta.name }}
    </span>

    <!-- Selected Indicator -->
    <div
      v-if="selected"
      class="absolute top-1 right-1 w-4 h-4 rounded-full bg-accent flex items-center justify-center"
    >
      <svg class="w-2.5 h-2.5 text-white" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="3" d="M5 13l4 4L19 7" />
      </svg>
    </div>
  </button>
</template>
