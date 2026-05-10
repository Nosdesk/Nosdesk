<script setup lang="ts">
/**
 * BrandIcon — multi-color third-party brand marks.
 *
 * Distinct from the main `Icon` registry, which only handles
 * monochrome stroke / fill icons via `currentColor`. Brand marks
 * (Google's four-color mosaic, etc.) carry their own canonical
 * colors as part of trademark guidelines and don't fit the
 * single-`d` + currentColor pattern.
 *
 * Microsoft is intentionally NOT here — its monochrome 4-square
 * representation lives in the main Icon registry under
 * `microsoft` because we use it as a generic Microsoft-services
 * marker (Entra, Intune, Graph), not as the official 4-color
 * brand mark. If a true brand-spec Microsoft glyph is ever
 * needed, add it here.
 */
defineProps<{
  brand: 'google'
  /** Tailwind size class. Defaults to `h-5 w-5`. */
  sizeClass?: string
}>()
</script>

<template>
  <svg
    v-if="brand === 'google'"
    xmlns="http://www.w3.org/2000/svg"
    :class="sizeClass ?? 'h-5 w-5'"
    viewBox="0 0 24 24"
    aria-label="Google"
    role="img"
  >
    <path
      fill="#4285F4"
      d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92a5.1 5.1 0 0 1-2.2 3.35v2.74h3.56c2.08-1.92 3.28-4.74 3.28-8.1z"
    />
    <path
      fill="#34A853"
      d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77a6.45 6.45 0 0 1-3.71 1.07 6.41 6.41 0 0 1-6-4.18H2.4v2.82A11.46 11.46 0 0 0 12 23z"
    />
    <path
      fill="#FBBC05"
      d="M5.99 14.54a6.6 6.6 0 0 1-.38-2.23c0-.77.14-1.52.38-2.22V7.27H2.4a11.54 11.54 0 0 0 0 10.46l3.59-3.19z"
    />
    <path
      fill="#EA4335"
      d="M12 5.38c1.62 0 3.06.56 4.2 1.64l3.15-3.15A10.56 10.56 0 0 0 12 1C7.33 1 3.28 3.83 1.32 7.75l3.66 2.84A6.53 6.53 0 0 1 12 5.38z"
    />
  </svg>
</template>
