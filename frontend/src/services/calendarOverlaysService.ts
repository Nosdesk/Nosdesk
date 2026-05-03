import apiClient from './apiConfig'

export interface CalendarOverlayEntry {
  kind: 'warranty_expiry' | 'maintenance' | 'os_cutoff'
  date: string
  device_id: number
  device_name: string
  label: string
}

export const calendarOverlaysService = {
  /**
   * Fetch every overlay entry whose date falls inside [start, end].
   * Today this returns warranty expiries; OS-cutoff and scheduled-
   * maintenance kinds light up here as their data sources land.
   */
  async list(start: string, end: string): Promise<CalendarOverlayEntry[]> {
    const { data } = await apiClient.get<CalendarOverlayEntry[]>(
      '/devices/calendar-overlay',
      { params: { start, end } },
    )
    return data
  },
}
