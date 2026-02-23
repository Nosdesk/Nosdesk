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

/**
 * webauthn-rs may return options already unwrapped (primary path) or wrapped
 * in { publicKey: ... } (fallback path). These types represent both formats.
 */
interface WrappedCreationOptions {
  publicKey: PublicKeyCredentialCreationOptionsJSON;
}

interface WrappedRequestOptions {
  publicKey: PublicKeyCredentialRequestOptionsJSON;
}

type CreationOptionsResponse =
  | PublicKeyCredentialCreationOptionsJSON
  | WrappedCreationOptions;

type RequestOptionsResponse =
  | (PublicKeyCredentialRequestOptionsJSON & { sessionId?: string })
  | (WrappedRequestOptions & { sessionId?: string });

/** Extract unwrapped creation options from either response format */
function unwrapCreationOptions(
  response: CreationOptionsResponse,
): PublicKeyCredentialCreationOptionsJSON {
  if ('publicKey' in response && response.publicKey && typeof response.publicKey === 'object') {
    return response.publicKey;
  }
  return response as PublicKeyCredentialCreationOptionsJSON;
}

/** Extract unwrapped request options from either response format */
function unwrapRequestOptions(
  response: RequestOptionsResponse,
): { options: PublicKeyCredentialRequestOptionsJSON; sessionId?: string } {
  const sessionId = 'sessionId' in response ? response.sessionId : undefined;
  let options: PublicKeyCredentialRequestOptionsJSON;

  if ('publicKey' in response && response.publicKey && typeof response.publicKey === 'object') {
    options = response.publicKey;
  } else {
    // Already unwrapped — clone to avoid mutating the original
    const { sessionId: _, ...rest } = response as PublicKeyCredentialRequestOptionsJSON & { sessionId?: string };
    options = rest;
  }

  return { options, sessionId };
}

class PasskeyService {
  /**
   * Check if WebAuthn is supported in the current browser
   */
  isSupported(): boolean {
    const supported = browserSupportsWebAuthn();

    // Debug logging to help diagnose support issues
    if (!supported) {
      logger.warn('WebAuthn not supported', {
        hasPublicKeyCredential: typeof window?.PublicKeyCredential !== 'undefined',
        isSecureContext: window?.isSecureContext,
        protocol: window?.location?.protocol,
        hostname: window?.location?.hostname,
      });
    }

    return supported;
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
  async startRegistration(passkey_name?: string): Promise<CreationOptionsResponse> {
    try {
      const response = await apiClient.post<CreationOptionsResponse>(
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

      // Unwrap options — backend usually sends unwrapped, but handle wrapped fallback
      const publicKeyOptions = unwrapCreationOptions(options);

      logger.debug('PublicKey options for registration', {
        publicKeyOptions,
        hasChallenge: 'challenge' in publicKeyOptions,
        hasRp: 'rp' in publicKeyOptions,
        hasUser: 'user' in publicKeyOptions,
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
  async startLogin(email?: string): Promise<RequestOptionsResponse> {
    try {
      const response = await apiClient.post<RequestOptionsResponse>(
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

      // Unwrap options and extract sessionId (present for discoverable auth)
      const { options: publicKeyOptions, sessionId } = unwrapRequestOptions(options);

      logger.debug('PublicKey options for authentication', {
        publicKeyOptions,
        hasChallenge: 'challenge' in publicKeyOptions,
        hasAllowCredentials: 'allowCredentials' in publicKeyOptions,
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

// =============================================================================
// MFA Setup Flow Methods (credential-based, no JWT required)
// =============================================================================

export interface PasskeySetupCredentials {
  email: string;
  password: string;
}

export interface PasskeySetupResult {
  success: boolean;
  csrf_token: string;
  user: {
    uuid: string;
    name: string;
    email: string;
    role: string;
  };
  passkey: {
    id: string;
    name: string;
  };
  backup_codes?: string[];
}

class PasskeySetupService {
  /**
   * Start passkey registration during MFA setup flow
   * Uses email+password instead of JWT authentication
   */
  async startRegistration(
    credentials: PasskeySetupCredentials,
    passkeyName?: string
  ): Promise<CreationOptionsResponse> {
    try {
      const response = await apiClient.post<CreationOptionsResponse>(
        '/auth/passkey-setup-login/start',
        {
          email: credentials.email,
          password: credentials.password,
          passkey_name: passkeyName,
        }
      );
      return response.data;
    } catch (error) {
      logger.error('Failed to start passkey setup registration', { error });
      throw error;
    }
  }

  /**
   * Complete passkey registration during MFA setup and log user in
   */
  async finishRegistration(
    credentials: PasskeySetupCredentials,
    credential: RegistrationResponseJSON,
    passkeyName?: string
  ): Promise<PasskeySetupResult> {
    try {
      const response = await apiClient.post<PasskeySetupResult>(
        '/auth/passkey-setup-login/finish',
        {
          email: credentials.email,
          password: credentials.password,
          ...credential,
          passkey_name: passkeyName,
        }
      );
      return response.data;
    } catch (error) {
      logger.error('Failed to finish passkey setup registration', { error });
      throw error;
    }
  }

  /**
   * Full passkey registration flow during MFA setup
   */
  async registerPasskey(
    credentials: PasskeySetupCredentials,
    name?: string
  ): Promise<PasskeySetupResult> {
    try {
      // Get registration options from server
      const options = await this.startRegistration(credentials, name);

      // Unwrap options — backend usually sends unwrapped, but handle wrapped fallback
      const publicKeyOptions = unwrapCreationOptions(options);

      logger.debug('PublicKey options for registration (setup flow)', {
        publicKeyOptions,
        hasChallenge: 'challenge' in publicKeyOptions,
        hasRp: 'rp' in publicKeyOptions,
        hasUser: 'user' in publicKeyOptions,
      });

      // Prompt user to create credential
      const credential = await startRegistration({ optionsJSON: publicKeyOptions });

      // Send credential to server for verification and login
      const result = await this.finishRegistration(credentials, credential, name);

      logger.info('Passkey registered successfully during MFA setup', { id: result.passkey.id });
      return result;
    } catch (error) {
      logger.error('Passkey registration failed during MFA setup', { error });
      throw error;
    }
  }
}

export const passkeyService = new PasskeyService();
export const passkeySetupService = new PasskeySetupService();
export default passkeyService;
