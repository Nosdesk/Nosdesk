/**
 * Shared timezone-options builder for any SearchableDropdown that
 * picks an IANA zone. Returns a computed list of
 * `DropdownOption`s with city as label and "region · offset · local
 * time" as description so the picker reads at a glance instead of
 * expecting the user to know their IANA path.
 *
 * Re-evaluates once per minute so the displayed local times don't
 * drift while the picker is open. Cheap (~600 options × one Intl
 * call); the interval is shared across all consumers of the
 * composable by Vue's reactivity automatically — each callsite gets
 * its own computed but the underlying tick is owned by this module.
 *
 * Used by LocalizationSettings (with a "Use device timezone"
 * sentinel prepended) and by SlaAdminView (raw IANA list).
 */
import { computed, onBeforeUnmount, onMounted, ref, type ComputedRef } from 'vue'
import { useDateStore } from '@nosdesk/core/stores/dateStore'
import type { DropdownOption } from '@/components/common/BaseDropdown.vue'

export function useTimezoneOptions(): ComputedRef<DropdownOption[]> {
  const dateStore = useDateStore()

  const ianaTimezones = computed<string[]>(() => {
    if (typeof Intl !== 'undefined' && 'supportedValuesOf' in Intl) {
      try {
        return (Intl as unknown as { supportedValuesOf: (k: string) => string[] })
          .supportedValuesOf('timeZone')
      } catch {
        return []
      }
    }
    return []
  })

  function splitIana(name: string): { city: string; region: string } {
    const parts = name.split('/')
    const city = parts[parts.length - 1].replace(/_/g, ' ')
    const region = parts[0].replace(/_/g, ' ')
    return { city, region }
  }

  function offsetFor(tz: string, now: Date): string {
    try {
      const parts = new Intl.DateTimeFormat('en', {
        timeZone: tz,
        timeZoneName: 'longOffset',
      }).formatToParts(now)
      const raw = parts.find((p) => p.type === 'timeZoneName')?.value ?? ''
      return raw.replace(/^GMT/, 'UTC')
    } catch {
      return ''
    }
  }

  function localTimeFor(tz: string, now: Date): string {
    try {
      return new Intl.DateTimeFormat(dateStore.locale, {
        timeZone: tz,
        hour: 'numeric',
        minute: '2-digit',
      }).format(now)
    } catch {
      return ''
    }
  }

  const tick = ref(0)
  let tickInterval: ReturnType<typeof setInterval> | undefined
  onMounted(() => {
    tickInterval = setInterval(() => {
      tick.value++
    }, 60_000)
  })
  onBeforeUnmount(() => {
    if (tickInterval) clearInterval(tickInterval)
  })

  return computed<DropdownOption[]>(() => {
    // Read tick so this re-evaluates on the minute.
    void tick.value
    const now = new Date()
    return ianaTimezones.value.map((name) => {
      const { city, region } = splitIana(name)
      const offset = offsetFor(name, now)
      const local = localTimeFor(name, now)
      return {
        value: name,
        label: city,
        description: [region, offset, local].filter(Boolean).join(' · '),
      }
    })
  })
}
