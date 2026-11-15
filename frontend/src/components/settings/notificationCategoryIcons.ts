import type { IconName } from '@/components/common/icons';

/**
 * Registry icon per notification category. Shared by the user's
 * NotificationSettings and the admin NotificationDefaultsView so both
 * matrices lead each category group with the same glyph. Labels and
 * descriptions come from i18n. Unknown categories fall back to `ticket`.
 */
export const NOTIFICATION_CATEGORY_ICON: Record<string, IconName> = {
  ticket: 'ticket',
  comment: 'comment',
  mention: 'at',
  documentation: 'document',
  asset: 'device',
};
