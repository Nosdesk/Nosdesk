/**
 * MFA Setup Store - Secure temporary credential storage
 *
 * This store handles temporary credential storage for the MFA setup flow.
 * Credentials are stored in memory only (not sessionStorage/localStorage)
 * and are automatically cleared after a timeout or when no longer needed.
 *
 * Security features:
 * - Credentials stored in memory only (not accessible via browser devtools storage)
 * - Automatic expiration after 5 minutes
 * - Cleared on logout, navigation away, or successful setup
 * - No sensitive data logged
 */

import { defineStore } from 'pinia';
import { ref, computed } from 'vue';

// Credential expiration time (5 minutes)
const CREDENTIAL_EXPIRY_MS = 5 * 60 * 1000;

interface MfaSetupCredentials {
  email: string;
  password: string;
  source: 'login' | 'onboarding';
  createdAt: number;
}

export const useMfaSetupStore = defineStore('mfaSetup', () => {
  // Private credential storage - in memory only
  const credentials = ref<MfaSetupCredentials | null>(null);
  const expiryTimer = ref<ReturnType<typeof setTimeout> | null>(null);

  // Check if credentials are still valid (not expired)
  const hasValidCredentials = computed(() => {
    if (!credentials.value) return false;
    const age = Date.now() - credentials.value.createdAt;
    return age < CREDENTIAL_EXPIRY_MS;
  });

  // Get credentials if valid, null otherwise
  const getCredentials = computed(() => {
    if (!hasValidCredentials.value) {
      // Auto-clear expired credentials
      if (credentials.value) {
        clearCredentials();
      }
      return null;
    }
    return credentials.value;
  });

  /**
   * Store credentials for MFA setup flow
   * Credentials will be automatically cleared after expiry time
   */
  function setCredentials(email: string, password: string, source: 'login' | 'onboarding') {
    // Clear any existing timer
    if (expiryTimer.value) {
      clearTimeout(expiryTimer.value);
    }

    // Store credentials
    credentials.value = {
      email,
      password,
      source,
      createdAt: Date.now()
    };

    // Set auto-expiry timer
    expiryTimer.value = setTimeout(() => {
      clearCredentials();
    }, CREDENTIAL_EXPIRY_MS);
  }

  /**
   * Clear all stored credentials immediately
   * Call this on:
   * - Successful MFA setup completion
   * - User navigates away from MFA setup
   * - Logout
   * - Any security-sensitive operation
   */
  function clearCredentials() {
    // Clear the timer
    if (expiryTimer.value) {
      clearTimeout(expiryTimer.value);
      expiryTimer.value = null;
    }

    // Clear credentials securely
    if (credentials.value) {
      // Overwrite sensitive data before clearing
      credentials.value.password = '';
      credentials.value.email = '';
      credentials.value = null;
    }
  }

  /**
   * Get email for display purposes only (doesn't expose password)
   */
  const email = computed(() => credentials.value?.email || null);

  /**
   * Get the source of credentials
   */
  const source = computed(() => credentials.value?.source || null);

  /**
   * Check remaining time before expiry (in seconds)
   */
  const remainingTime = computed(() => {
    if (!credentials.value) return 0;
    const remaining = CREDENTIAL_EXPIRY_MS - (Date.now() - credentials.value.createdAt);
    return Math.max(0, Math.floor(remaining / 1000));
  });

  return {
    // State (computed for read-only access)
    hasValidCredentials,
    getCredentials,
    email,
    source,
    remainingTime,

    // Actions
    setCredentials,
    clearCredentials
  };
});
