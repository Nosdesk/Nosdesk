import apiClient from './apiConfig';
import { logger } from '@/utils/logger';
import {
  startRegistration,
  startAuthentication,
  browserSupportsWebAuthn,
  browserSupportsWebAuthnAutofill,
} from '@simplewebauthn/browser';
import type {
  PublicKeyCredentialCreationOptionsJSON,
  PublicKeyCredentialRequestOptionsJSON,
  RegistrationResponseJSON,
  AuthenticationResponseJSON,
} from '@simplewebauthn/types';

// Passkey info returned from API
export interface PasskeyInfo {
  id: string;
  name: string;
  created_at: string;
  last_used_at: string | null;
  transports: string[];
  backup_eligible: boolean;
}

// API response types
export interface PasskeyListResponse {
  passkeys: PasskeyInfo[];
}

export interface PasskeyRegistrationResult {
  success: boolean;
  passkey: {
    id: string;
    name: string;
    created_at: string;
  };
}

export interface PasskeyLoginResult {
  success: boolean;
  csrf_token: string;
  user: {
    uuid: string;
    name: string;
    email: string;
    role: string;
  };
}

class PasskeyService {
  /**
   * Check if WebAuthn is supported in the current browser
   */
  isSupported(): boolean {
    return browserSupportsWebAuthn();
  }

  /**
   * Check if conditional UI (autofill) is supported
   */
  async isConditionalUISupported(): Promise<boolean> {
    try {
      return await browserSupportsWebAuthnAutofill();
    } catch {
      return false;
    }
  }

  /**
   * List all passkeys for the current user
   */
  async listPasskeys(): Promise<PasskeyInfo[]> {
    try {
      const response = await apiClient.get<PasskeyListResponse>('/auth/passkeys');
      return response.data.passkeys;
    } catch (error) {
      logger.error('Failed to list passkeys', { error });
      throw error;
    }
  }

  /**
   * Start passkey registration - gets challenge from server
   */
  async startRegistration(passkey_name?: string): Promise<PublicKeyCredentialCreationOptionsJSON> {
    try {
      const response = await apiClient.post<PublicKeyCredentialCreationOptionsJSON>(
        '/auth/passkeys/register/start',
        { passkey_name }
      );
      return response.data;
    } catch (error) {
      logger.error('Failed to start passkey registration', { error });
      throw error;
    }
  }

  /**
   * Complete passkey registration - sends credential to server
   */
  async finishRegistration(
    credential: RegistrationResponseJSON,
    passkey_name?: string
  ): Promise<PasskeyRegistrationResult> {
    try {
      const response = await apiClient.post<PasskeyRegistrationResult>(
        '/auth/passkeys/register/finish',
        {
          ...credential,
          passkey_name,
        }
      );
      return response.data;
    } catch (error) {
      logger.error('Failed to finish passkey registration', { error });
      throw error;
    }
  }

  /**
   * Full passkey registration flow
   */
  async registerPasskey(name?: string): Promise<PasskeyRegistrationResult> {
    try {
      // Get registration options from server
      const options = await this.startRegistration(name);

      // Log the raw options for debugging
      logger.debug('Registration options received from server', {
        options,
        optionsKeys: Object.keys(options || {}),
        hasPublicKey: 'publicKey' in (options || {}),
        hasPublic_key: 'public_key' in (options || {}),
      });

      // webauthn-rs returns the options wrapped in { publicKey: ... } or { public_key: ... }
      // SimpleWebAuthn expects the PublicKeyCredentialCreationOptions contents directly
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const rawOptions = options as any;
      const publicKeyOptions: PublicKeyCredentialCreationOptionsJSON =
        rawOptions.publicKey || rawOptions.public_key || options;

      logger.debug('PublicKey options for registration', {
        publicKeyOptions,
        hasChallenge: 'challenge' in (publicKeyOptions || {}),
        hasRp: 'rp' in (publicKeyOptions || {}),
        hasUser: 'user' in (publicKeyOptions || {}),
      });

      // Prompt user to create credential
      const credential = await startRegistration({ optionsJSON: publicKeyOptions });

      // Send credential to server for verification
      const result = await this.finishRegistration(credential, name);

      logger.info('Passkey registered successfully', { id: result.passkey.id });
      return result;
    } catch (error) {
      logger.error('Passkey registration failed', { error });
      throw error;
    }
  }

  /**
   * Start passkey login - gets challenge from server
   * For usernameless (discoverable) login, don't pass email
   */
  async startLogin(email?: string): Promise<PublicKeyCredentialRequestOptionsJSON & { sessionId?: string }> {
    try {
      const response = await apiClient.post<PublicKeyCredentialRequestOptionsJSON & { sessionId?: string }>(
        '/auth/passkeys/login/start',
        email ? { email } : {}
      );
      return response.data;
    } catch (error) {
      logger.error('Failed to start passkey login', { error });
      throw error;
    }
  }

  /**
   * Complete passkey login - sends assertion to server
   */
  async finishLogin(credential: AuthenticationResponseJSON, sessionId?: string): Promise<PasskeyLoginResult> {
    try {
      const response = await apiClient.post<PasskeyLoginResult>(
        '/auth/passkeys/login/finish',
        {
          ...credential,
          session_id: sessionId,
        }
      );
      return response.data;
    } catch (error) {
      logger.error('Failed to finish passkey login', { error });
      throw error;
    }
  }

  /**
   * Full passkey login flow
   * - If email is provided: Uses email-based lookup (works with all passkeys)
   * - If no email: Uses discoverable auth (requires resident key passkeys)
   */
  async loginWithPasskey(email?: string): Promise<PasskeyLoginResult> {
    try {
      // Get authentication options from server
      const options = await this.startLogin(email);

      // Log the raw options for debugging
      logger.debug('Authentication options received from server', {
        options,
        optionsKeys: Object.keys(options || {}),
        hasSessionId: 'sessionId' in (options || {}),
        emailProvided: !!email,
      });

      // Extract session ID for discoverable auth (only present when no email)
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const sessionId = (options as any).sessionId;

      // webauthn-rs returns the options, SimpleWebAuthn expects PublicKeyCredentialRequestOptions
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const rawOptions = options as any;
      const publicKeyOptions: PublicKeyCredentialRequestOptionsJSON =
        rawOptions.publicKey || rawOptions.public_key || options;

      // Remove sessionId from options before passing to startAuthentication
      if ('sessionId' in publicKeyOptions) {
        delete (publicKeyOptions as Record<string, unknown>).sessionId;
      }

      logger.debug('PublicKey options for authentication', {
        publicKeyOptions,
        hasChallenge: 'challenge' in (publicKeyOptions || {}),
        hasAllowCredentials: 'allowCredentials' in (publicKeyOptions || {}),
        sessionId,
      });

      // Prompt user to authenticate with passkey
      const credential = await startAuthentication({ optionsJSON: publicKeyOptions });

      // Send assertion to server for verification (include sessionId for discoverable auth)
      const result = await this.finishLogin(credential, sessionId);

      logger.info('Passkey login successful', { userUuid: result.user.uuid });
      return result;
    } catch (error) {
      logger.error('Passkey login failed', { error });
      throw error;
    }
  }

  /**
   * Rename a passkey
   */
  async renamePasskey(credentialId: string, name: string): Promise<boolean> {
    try {
      const response = await apiClient.patch(`/auth/passkeys/${encodeURIComponent(credentialId)}`, {
        name,
      });
      return response.data.success;
    } catch (error) {
      logger.error('Failed to rename passkey', { error });
      throw error;
    }
  }

  /**
   * Delete a passkey (requires password verification)
   */
  async deletePasskey(credentialId: string, password: string): Promise<boolean> {
    try {
      const response = await apiClient.delete(`/auth/passkeys/${encodeURIComponent(credentialId)}`, {
        data: { password },
      });
      return response.data.success;
    } catch (error) {
      logger.error('Failed to delete passkey', { error });
      throw error;
    }
  }
}

export const passkeyService = new PasskeyService();
export default passkeyService;
