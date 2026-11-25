<script setup lang="ts">
/**
 * Plugin icon with graceful 404 fallback. Accepts either a `uuid`
 * (renders the installed plugin's bundled icon at
 * `/api/plugins/{uuid}/icon`) or a `src` (an arbitrary URL, used by
 * the registry view for `icon_url`). Either way the component owns
 * the failure state so callers never see the 404.
 */
import AssetImg from '@/components/common/AssetImg.vue';
import Icon from '@/components/common/Icon.vue';
import { computed, ref, watch } from 'vue';

interface Props {
  uuid?: string;
  src?: string;
  alt: string;
  /** `xs` matches the 16px leading glyph a `SectionCard` header expects; the
   *  larger sizes are for list rows and detail headers. */
  size?: 'xs' | 'sm' | 'md' | 'lg';
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
  xs: 'h-4 w-4 rounded-[3px]',
  sm: 'h-8 w-8 rounded-md',
  md: 'h-10 w-10 rounded-lg',
  lg: 'h-20 w-20 rounded-2xl',
};

/** Registry icon size for the fallback glyph at each badge size. */
const glyphSizes: Record<NonNullable<Props['size']>, 'xs' | 'sm' | 'md' | 'xl'> = {
  xs: 'xs',
  sm: 'sm',
  md: 'md',
  lg: 'xl',
};
</script>

<template>
  <div
    class="flex flex-shrink-0 items-center justify-center overflow-hidden bg-accent/10"
    :class="sizeClasses[props.size]"
  >
    <AssetImg
      v-if="resolvedSrc && !failed"
      :src="resolvedSrc"
      :alt="alt"
      class="h-full w-full object-cover"
      referrerpolicy="no-referrer"
      @error="failed = true"
    />
    <Icon
      v-else
      name="puzzle"
      :size="glyphSizes[props.size]"
      class="text-accent-ui"
    />
  </div>
</template>
