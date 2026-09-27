import { inject, type InjectionKey } from 'vue'

/**
 * Provided (true) by the requester portal app. The guest pages render in both
 * apps; inside the portal, requester pages (sign in, my requests) are routes of
 * the same app, so links route in place instead of leaving for the server.
 */
export const IN_PORTAL: InjectionKey<boolean> = Symbol('nosdesk:in-portal')

export function useInPortal(): boolean {
  return inject(IN_PORTAL, false)
}
