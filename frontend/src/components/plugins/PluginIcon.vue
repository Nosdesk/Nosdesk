<script setup lang="ts">
/**
 * Plugin icon with graceful 404 fallback. Accepts either a `uuid`
 * (renders the installed plugin's bundled icon at
 * `/api/plugins/{uuid}/icon`) or a `src` (an arbitrary URL, used by
 * the registry view for `icon_url`). Either way the component owns
 * the failure state so callers never see the 404.
 */
import { computed, ref, watch } from 'vue';

interface Props {
  uuid?: string;
  src?: string;
  alt: string;
  size?: 'sm' | 'md' | 'lg';
}

const props = withDefaults(defineProps<Props>(), { size: 'md' });

const failed = ref(false);

const resolvedSrc = computed<string | null>(() => {
  if (props.src) return props.src;
  if (props.uuid) return `/api/plugins/${props.uuid}/icon`;
  return null;
});

watch(resolvedSrc, () => {
  failed.value = false;
});

const sizeClasses: Record<NonNullable<Props['size']>, string> = {
  sm: 'h-8 w-8 rounded-md',
  md: 'h-10 w-10 rounded-lg',
  lg: 'h-20 w-20 rounded-2xl',
};

const glyphSizes: Record<NonNullable<Props['size']>, string> = {
  sm: 'h-4 w-4',
  md: 'h-5 w-5',
  lg: 'h-10 w-10',
};
</script>

<template>
  <div
    class="flex flex-shrink-0 items-center justify-center overflow-hidden bg-accent/10"
    :class="sizeClasses[props.size]"
  >
    <img
      v-if="resolvedSrc && !failed"
      :src="resolvedSrc"
      :alt="alt"
      class="h-full w-full object-cover"
      referrerpolicy="no-referrer"
      @error="failed = true"
    />
    <svg
      v-else
      xmlns="http://www.w3.org/2000/svg"
      :class="glyphSizes[props.size]"
      class="text-accent"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      stroke-width="2"
      aria-hidden="true"
    >
      <path
        stroke-linecap="round"
        stroke-linejoin="round"
        d="M11 4a2 2 0 114 0v1a1 1 0 001 1h3a1 1 0 011 1v3a1 1 0 01-1 1h-1a2 2 0 100 4h1a1 1 0 011 1v3a1 1 0 01-1 1h-3a1 1 0 01-1-1v-1a2 2 0 10-4 0v1a1 1 0 01-1 1H7a1 1 0 01-1-1v-3a1 1 0 00-1-1H4a2 2 0 110-4h1a1 1 0 001-1V7a1 1 0 011-1h3a1 1 0 001-1V4z"
      />
    </svg>
  </div>
</template>
