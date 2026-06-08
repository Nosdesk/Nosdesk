import type { MenuItem } from '@/components/common/MenuList.vue'

export const PROJECT_STATUSES = ['active', 'completed', 'archived'] as const

type Translate = (key: string) => string

/** Menu items shared by the project actions popover and list context menu. */
export function buildProjectMenuItems(
  status: string,
  t: Translate,
  options?: { forContextMenu?: boolean },
): MenuItem[] {
  const items: MenuItem[] = []

  if (options?.forContextMenu) {
    items.push({ id: 'open', label: t('project-context-open') })
  }

  items.push({
    id: 'rename',
    label: t('project-actions-rename'),
    divider: options?.forContextMenu,
  })

  PROJECT_STATUSES.forEach((s, i) => {
    items.push({
      id: `status:${s}`,
      label: t(`project-actions-status-${s}`),
      checked: status === s,
      divider: !options?.forContextMenu && i === 0,
    })
  })

  items.push({
    id: 'delete',
    label: t('project-actions-delete'),
    danger: true,
    divider: true,
  })

  return items
}
