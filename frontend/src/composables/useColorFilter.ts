import { computed } from 'vue';
import { useThemeStore } from '@/stores/theme';

/**
 * Composable for applying theme-appropriate color filters
 * Red Horizon theme applies a warm sepia filter to match the gas-plasma aesthetic
 */
export function useColorFilter() {
  const themeStore = useThemeStore();

  const isRedHorizon = computed(() => themeStore.effectiveTheme.meta.id === 'red-horizon');

  // CSS filter string for Red Horizon theme
  const colorFilterStyle = computed(() =>
    isRedHorizon.value
      ? { filter: 'sepia(1) saturate(2) hue-rotate(-10deg) brightness(0.9)' }
      : {}
  );

  // CSS class approach (requires global CSS)
  const colorFilterClass = computed(() =>
    isRedHorizon.value ? 'red-horizon-color-filter' : ''
  );

  return {
    isRedHorizon,
    colorFilterStyle,
    colorFilterClass
  };
}
