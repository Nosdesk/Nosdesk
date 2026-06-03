/**
 * Audit-log annotation overlay state for the dashboard.
 *
 * Reads URL state (`?annotations=on`) and the active time range,
 * fetches the matching audit markers from
 * `/api/dashboard/audit-annotations`, and returns a reactive
 * `markers` list the chart components overlay on their x-axis.
 *
 * The overlay is a single shared query (one fetch per dashboard
 * load, shared by every chart on the page) because the marker set
 * is small and time-range-bound; per-chart per-instance fetches
 * would dwarf the actual data.
 */
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import { useQuery } from '@pinia/colada'
import { useTimeRange } from './useTimeRange'
import { analyticsService, type AnnotationMarker } from '@/services/analyticsService'

export function useAnnotations() {
  const route = useRoute()
  const { window: timeWindow } = useTimeRange()

  const enabled = computed<boolean>(() => route.query.annotations === 'on')

  const query = useQuery({
    key: () => [
      'dashboard',
      'audit-annotations',
      timeWindow.value.from,
      timeWindow.value.to,
    ],
    query: () =>
      analyticsService.auditAnnotations({
        from: timeWindow.value.from,
        to: timeWindow.value.to,
      }),
    enabled: () => enabled.value,
  })

  const markers = computed<AnnotationMarker[]>(() =>
    enabled.value ? query.data.value?.markers ?? [] : [],
  )

  return {
    enabled,
    markers,
  }
}
