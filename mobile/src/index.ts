/**
 * Public surface of the mobile host layer.
 *
 * `bootstrapMobile` wires the @nosdesk/core seams at app start. The server
 * picker (`validateServer` / `setServer` / `getStoredServer` / `DEFAULT_SERVER`)
 * backs the connect screen so a self-hoster can point the app at their own
 * instance. `setSession` / `clearSession` are called by the login and sign-out
 * flows. `SecureStore` is the keychain contract the native shell implements
 * (`memorySecureStore` for dev/tests).
 */
export { bootstrapMobile, type MobileBootstrapOptions } from './bootstrap'
export { setSession, clearSession, setServer } from './transport'
export { loginWithOidc } from './oidc'
export {
  registerForPush,
  unregisterForPush,
  getPendingNotificationRoute,
  onNotificationOpened,
} from './push'
export {
  getStoredServer,
  clearStoredServer,
  validateServer,
  DEFAULT_SERVER,
  type ServerValidation,
} from './serverConfig'
export { memorySecureStore, tauriSecureStore, type SecureStore } from './secureStore'
export type { MobileLoggerOptions } from './loggerSetup'
