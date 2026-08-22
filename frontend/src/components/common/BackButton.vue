<script setup lang="ts">
import Icon from '@/components/common/Icon.vue';
import { useBackNavigation } from '@/router/navigation';
import { useMobileDetection } from '@/composables/useMobileDetection';

const props = defineProps<{
  // Explicit fallback target used only when there is no in-app history (deep
  // link / cold start). Overrides the route's meta.parent / derived parent.
  fallbackRoute?: string;
  // Custom label for the button (defaults to "Go back")
  label?: string;
  // Compact mode - smaller text, tighter spacing
  compact?: boolean;
  // Icon-only: a square chevron with no visible text, for headers whose title
  // sits immediately beside it and has no room for a label. `label` is still
  // required in that case and becomes the accessible name.
  iconOnly?: boolean;
}>();

// One intelligent back action: pop the in-app stack when there is a real
// previous view, else navigate to the hierarchical parent. Replaces the old
// `window.history.length > 1` heuristic (which misfired on deep-link/cold-start).
const { goBack } = useBackNavigation();
const handleBack = () => goBack(props.fallbackRoute);

// Desktop only: on mobile the leading back-arrow in SiteHeader is the single
// back affordance, so this inline button hides to avoid two controls per screen.
const { isMobile } = useMobileDetection('sm');
</script>

<template>
  <!--
    `h-8` / `h-6` (2rem / 1.5rem) matches the height of the other
    toolbar buttons in the app (icon buttons are `p-1.5` + a 1.25rem
    svg → 2rem). Declaring it here means every toolbar that contains
    a BackButton has a stable intrinsic row height without needing a
    `min-h` or fixed `h-12` on the row itself — the row sizes to its
    children, which is how flexbox is meant to be used.
  -->
  <button
    v-if="!isMobile && iconOnly"
    type="button"
    @click="handleBack"
    :title="label || 'Go back'"
    :aria-label="label || 'Go back'"
    class="p-1.5 -ml-1.5 rounded-md text-tertiary hover:text-primary hover:bg-surface-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-accent transition-colors shrink-0"
  >
    <Icon name="chevronLeft" size="md" />
  </button>

  <button
    v-else-if="!isMobile"
    @click="handleBack"
    class="text-secondary hover:text-primary inline-flex items-center gap-1 group px-1 rounded"
    :class="compact ? 'h-6 text-xs' : 'h-8 text-sm'"
  >
    <span class="group-hover:-translate-x-0.5 transition-transform inline-flex">
      <Icon name="chevronLeft" :size="compact ? 'xs' : 'sm'" />
    </span>
    {{ label || 'Go back' }}
  </button>
</template> 