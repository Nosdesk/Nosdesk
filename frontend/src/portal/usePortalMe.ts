import { computed } from 'vue'
import { useQuery } from '@pinia/colada'

import { getMe } from './service'

/** The signed-in person, and whether their session came from an email link. */
export function usePortalMe() {
  const me = useQuery({ key: ['portal', 'me'], query: getMe })
  /** Opened from a request's or an approval's email link: that request only,
   *  until they sign in. Pages that need a full sign-in leave themselves out. */
  const linkSession = computed(() => me.data.value?.scope?.kind === 'ticket')
  /** Known to be a full sign-in: false until `/me` has answered. */
  const fullSession = computed(() => !!me.data.value && !linkSession.value)
  return { me, linkSession, fullSession }
}
