/**
 * Choices the server names in English, in the viewer's language by their
 * value: an asset's status, a person's role, a ticket's priority. Any
 * other choice, and a value the app has no label for, keeps the server's
 * label.
 */
import type { ImportJobType } from '@nosdesk/core/services/importService'

import { assetStatusLabel } from '@/utils/assetStatusMeta'

type Translate = (key: string, args?: Record<string, string | number>) => string

const ROLES = new Set(['admin', 'technician', 'user', 'audit_reviewer'])
const PRIORITIES = new Set(['urgent', 'high', 'medium', 'low', 'none'])

/** Whether the app words this field's choices itself. */
export function localisesChoices(type: ImportJobType | undefined, field: string): boolean {
  return (
    (type === 'assets' && field === 'status') ||
    (type === 'users' && field === 'role') ||
    (type === 'tickets' && field === 'priority')
  )
}

/** A choice's label for `value` of `field`, else `fallback`. */
export function choiceLabel(
  t: Translate,
  type: ImportJobType | undefined,
  field: string,
  value: string,
  fallback: string,
): string {
  if (type === 'assets' && field === 'status') return assetStatusLabel(t, value)
  if (type === 'users' && field === 'role' && ROLES.has(value)) return t(`user-mgmt-role-${value}`)
  if (type === 'tickets' && field === 'priority' && PRIORITIES.has(value)) return t(`priority-${value}`)
  return fallback
}
