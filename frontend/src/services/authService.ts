import axios from 'axios';

const API_BASE_URL = import.meta.env.VITE_API_BASE_URL || '';

export interface OnboardingStatus {
  requires_setup: boolean;
  user_count: number;
}

export interface AdminSetupRequest {
  name: string;
  email: string;
  password: string;
}

export interface AdminSetupResponse {
  success: boolean;
  message: string;
  user?: any;
}

export interface LoginCredentials {
  email: string;
  password: string;
}

export interface LoginResponse {
  token: string;
  user: any;
}

class AuthService {
  private setupStatusCache: {
    data: OnboardingStatus | null;
    timestamp: number;
    ttl: number; // time to live in milliseconds
  } = {
    data: null,
    timestamp: 0,
    ttl: 5 * 60 * 1000 // Cache for 5 minutes
  };
  
  // Security: Prevent multiple simultaneous setup checks
  private setupCheckInProgress: boolean = false;
  private setupCheckPromise: Promise<OnboardingStatus> | null = null;

  /**
   * Check if the system requires initial setup
   * Security: Prevents multiple simultaneous checks and includes rate limiting
   */
  async checkSetupStatus(): Promise<OnboardingStatus> {
    // Security: Check if we have cached data that's still valid
    const now = Date.now();
    if (this.setupStatusCache.data && 
        (now - this.setupStatusCache.timestamp) < this.setupStatusCache.ttl) {
      return this.setupStatusCache.data;
    }

    // Security: Prevent multiple simultaneous setup checks
    if (this.setupCheckInProgress && this.setupCheckPromise) {
      console.log('🔄 AuthService: Setup check already in progress, waiting...');
      return this.setupCheckPromise;
    }

    // Security: Rate limiting - prevent excessive calls
    const timeSinceLastCheck = now - this.setupStatusCache.timestamp;
    if (timeSinceLastCheck < 1000) { // Minimum 1 second between checks
      console.warn('⚠️  AuthService: Rate limiting setup status checks');
      if (this.setupStatusCache.data) {
        return this.setupStatusCache.data;
      }
      // Wait for the minimum interval
      await new Promise(resolve => setTimeout(resolve, 1000 - timeSinceLastCheck));
    }

    // Set up the promise for this check
    this.setupCheckInProgress = true;
    this.setupCheckPromise = this._performSetupStatusCheck();
    
    try {
      const result = await this.setupCheckPromise;
      return result;
    } finally {
      this.setupCheckInProgress = false;
      this.setupCheckPromise = null;
    }
  }

  /**
   * Private method to perform the actual setup status check
   * Security: Separated to prevent race conditions
   */
  private async _performSetupStatusCheck(): Promise<OnboardingStatus> {
    try {
      const response = await axios.get(`${API_BASE_URL}/api/auth/setup/status`, {
        timeout: 10000, // 10 second timeout for security
        headers: {
          'X-Requested-With': 'XMLHttpRequest', // CSRF protection
        }
      });
      
      // Security: Validate response data
      if (!response.data || typeof response.data.requires_setup !== 'boolean') {
        console.error('Invalid setup status response format');
        throw new Error('Invalid setup status response format');
      }
      
      // Cache the response
      this.setupStatusCache.data = response.data;
      this.setupStatusCache.timestamp = Date.now();
      
      console.log('🔄 AuthService: Setup status checked:', response.data);
      return response.data;
    } catch (error: any) {
      console.error('Error checking setup status:', error);
      
      // Security: If it's a network error or server error, assume setup is required
      // This ensures new users can still access onboarding even if there are temporary issues
      if (error.code === 'NETWORK_ERROR' || error.code === 'ERR_NETWORK' || 
          (error.response && error.response.status >= 500)) {
        console.warn('Network/server error, assuming setup required for safety');
        return {
          requires_setup: true,
          user_count: 0
        };
      }
      
      throw error;
    }
  }

  /**
   * Setup the initial admin user
   */
  async setupInitialAdmin(adminData: AdminSetupRequest): Promise<AdminSetupResponse> {
    try {
      const response = await axios.post(`${API_BASE_URL}/api/auth/setup/admin`, adminData);
      return response.data;
    } catch (error) {
      console.error('Error setting up initial admin:', error);
      throw error;
    }
  }

  /**
   * Login with email and password
   */
  async login(credentials: LoginCredentials): Promise<LoginResponse> {
    try {
      const response = await axios.post(`${API_BASE_URL}/api/auth/login`, credentials);
      return response.data;
    } catch (error) {
      console.error('Login error:', error);
      throw error;
    }
  }

  /**
   * Get the current authenticated user
   */
  async getCurrentUser(): Promise<any> {
    try {
      const token = localStorage.getItem('authToken');
      if (!token) {
        throw new Error('No auth token found');
      }

      const response = await axios.get(`${API_BASE_URL}/api/auth/me`, {
        headers: {
          'Authorization': `Bearer ${token}`
        }
      });
      return response.data;
    } catch (error) {
      console.error('Error getting current user:', error);
      throw error;
    }
  }

  /**
   * Logout the current user
   */
  logout(): void {
    localStorage.removeItem('authToken');
  }

  /**
   * Check if user is authenticated
   */
  isAuthenticated(): boolean {
    return !!localStorage.getItem('authToken');
  }

  /**
   * Get the stored auth token
   */
  getToken(): string | null {
    return localStorage.getItem('authToken');
  }

  /**
   * Store the auth token
   */
  setToken(token: string): void {
    localStorage.setItem('authToken', token);
  }

  /**
   * Clear the setup status cache (useful after admin setup completes)
   */
  clearSetupStatusCache(): void {
    this.setupStatusCache.data = null;
    this.setupStatusCache.timestamp = 0;
    console.log('🔄 AuthService: Setup status cache cleared');
  }
}

export const authService = new AuthService();
export default authService; 