/**
 * Utility functions for generating status and priority indicator SVGs
 * Used by both Vue components and ProseMirror plugins to avoid DRY violations
 */

type TicketStatus = 'open' | 'in-progress' | 'closed'
type TicketPriority = 'low' | 'medium' | 'high'

/**
 * Generate a unique ID for SVG elements (clip paths, etc.)
 */
function generateUniqueId(prefix: string): string {
  return `${prefix}-${Math.random().toString(36).substr(2, 9)}`
}

/**
 * Generate status indicator SVG for colorblind mode
 * - Open: hollow circle (outline only)
 * - In Progress: bullseye (ring with center dot)
 * - Closed: filled circle
 */
export function getStatusIndicatorSvg(status: TicketStatus, colorClass?: string): string {
  const color = colorClass || `text-status-${status}`

  switch (status) {
    case 'open':
      return `<svg class="indicator-icon" viewBox="0 0 10 10" fill="none">
        <circle cx="5" cy="5" r="4" stroke="currentColor" stroke-width="1.5" class="${color}"/>
      </svg>`
    case 'in-progress':
      return `<svg class="indicator-icon" viewBox="0 0 10 10">
        <circle cx="5" cy="5" r="4" stroke="currentColor" stroke-width="1.5" fill="none" class="${color}"/>
        <circle cx="5" cy="5" r="2" fill="currentColor" class="${color}"/>
      </svg>`
    case 'closed':
      return `<svg class="indicator-icon" viewBox="0 0 10 10">
        <circle cx="5" cy="5" r="4" fill="currentColor" class="${color}"/>
      </svg>`
    default:
      return ''
  }
}

/**
 * Generate priority indicator SVG for colorblind mode
 * - Low: empty rounded square (outline only)
 * - Medium: half-filled rounded square
 * - High: fully filled rounded square
 */
export function getPriorityIndicatorSvg(priority: TicketPriority, colorClass?: string): string {
  const color = colorClass || `text-priority-${priority}`

  switch (priority) {
    case 'low':
      return `<svg class="indicator-icon" viewBox="0 0 10 10" fill="none">
        <rect x="1" y="1" width="8" height="8" rx="2" stroke="currentColor" stroke-width="1.5" class="${color}"/>
      </svg>`
    case 'medium': {
      const clipPathId = generateUniqueId('priority-half')
      return `<svg class="indicator-icon" viewBox="0 0 10 10">
        <defs>
          <clipPath id="${clipPathId}">
            <rect x="0" y="5" width="10" height="5"/>
          </clipPath>
        </defs>
        <rect x="1" y="1" width="8" height="8" rx="2" stroke="currentColor" stroke-width="1.5" fill="none" class="${color}"/>
        <rect x="1" y="1" width="8" height="8" rx="2" fill="currentColor" clip-path="url(#${clipPathId})" class="${color}"/>
      </svg>`
    }
    case 'high':
      return `<svg class="indicator-icon" viewBox="0 0 10 10">
        <rect x="1" y="1" width="8" height="8" rx="2" fill="currentColor" class="${color}"/>
      </svg>`
    default:
      return ''
  }
}

/**
 * Check if colorblind mode is enabled
 * Uses localStorage for non-reactive contexts (like ProseMirror plugins)
 */
export function isColorBlindMode(): boolean {
  return localStorage.getItem('colorBlindMode') === 'true'
}
