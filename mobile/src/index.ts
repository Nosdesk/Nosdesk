/**
 * Public surface of the mobile host layer.
 *
 * `bootstrapMobile` wires the @nosdesk/core seams at app start; `setSession` /
 * `clearSession` are called by the login and sign-out flows; `SecureStore` is
 * the keychain contract the native shell implements (with `memorySecureStore`
 * available for dev/tests).
 */
export { bootstrapMobile, type MobileBootstrapOptions } from './bootstrap'
export { setSession, clearSession, type MobileTransportOptions } from './transport'
export { memorySecureStore, type SecureStore } from './secureStore'
export type { MobileLoggerOptions } from './loggerSetup'
