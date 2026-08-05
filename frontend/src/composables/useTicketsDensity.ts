/**
 * Row density toggle for the tickets table. Three steps follow
 * the Pencil & Paper enterprise table guide. User-controlled,
 * persisted to localStorage so the choice survives reloads.
 */
import { computed, ref, type ComputedRef, type Ref } from 'vue'

export type Density = 'compact' | 'cosy' | 'comfortable'

function loadDensity(storageKey: string): Density {
  // Default to compact for power-user personas (MSP techs,
  // sysadmins) who live in the queue all day and want maximum
  // rows per viewport. The toggle to cosy/comfortable is one
  // click in the toolbar for users who prefer breathing room.
  if (typeof localStorage === 'undefined') return 'compact'
  const v = localStorage.getItem(storageKey)
  return v === 'cosy' || v === 'comfortable' ? v : 'compact'
}

export interface UseTicketsDensity {
  density: Ref<Density>
  setDensity: (value: Density) => void
  rowClass: ComputedRef<string>
  cellPadding: ComputedRef<string>
}

export function useListDensity(storageKey: string): UseTicketsDensity {
  const density = ref<Density>(loadDensity(storageKey))

  function setDensity(value: Density): void {
    density.value = value
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem(storageKey, value)
    }
  }

  const rowClass = computed<string>(() => {
    if (density.value === 'compact') return 'h-7'
    if (density.value === 'comfortable') return 'h-12'
    return 'h-10'
  })

  const cellPadding = computed<string>(() =>
    density.value === 'compact'
      ? 'px-2 py-0.5'
      : density.value === 'comfortable'
        ? 'px-3 py-2.5'
        : 'px-3 py-1.5',
  )

  return { density, setDensity, rowClass, cellPadding }
}

export function useTicketsDensity(): UseTicketsDensity {
  return useListDensity('tickets-list-density')
}

export function useProjectsDensity(): UseTicketsDensity {
  return useListDensity('projects-list-density')
}

export function useAssetsDensity(): UseTicketsDensity {
  return useListDensity('assets-list-density')
}
