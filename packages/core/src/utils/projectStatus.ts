/** Dot colour for a project's lifecycle status, shared by the
 *  projects list row and card. Presentation only. */
export function projectStatusDot(status: string): string {
  if (status === 'active') return 'bg-status-open'
  if (status === 'completed') return 'bg-status-success'
  return 'bg-tertiary'
}
