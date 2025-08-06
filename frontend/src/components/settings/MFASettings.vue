<script setup lang="ts">
import { ref, computed } from 'vue';
import ToggleSwitch from '@/components/common/ToggleSwitch.vue';
import { useAuthStore } from '@/stores/auth';
import axios from 'axios';

// MFA state
const mfaEnabled = ref(false);
const mfaStep = ref<'setup' | 'verify' | 'enabled' | 'success'>('setup');
const qrCodeUrl = ref('');
const verificationCode = ref('');
const loading = ref(false);
const verifying = ref(false); // Separate loading state for verification
const backupCodes = ref<string[]>([]);
const mfaSecret = ref(''); // Store the secret temporarily during setup
const showSecret = ref(false); // Toggle for showing the secret text
const secretCopied = ref(false); // Track if secret was just copied

const authStore = useAuthStore();

// Props for different modes
const props = defineProps<{
  isLoginSetup?: boolean; // When true, this is for login-time MFA setup
}>();

// Emits for notifications
const emit = defineEmits<{
  (e: 'success', message: string): void;
  (e: 'error', message: string): void;
}>();

// Computed properties
const showSetupSteps = computed(() => !mfaEnabled.value && mfaStep.value === 'setup');
const showVerificationStep = computed(() => !mfaEnabled.value && mfaStep.value === 'verify');
const showEnabledState = computed(() => mfaEnabled.value && !props.isLoginSetup);
const showSuccessState = computed(() => mfaStep.value === 'success');

// Check current MFA status
const checkMFAStatus = async () => {
  try {
    const response = await fetch('/api/auth/mfa/status', {
      headers: {
        'Authorization': `Bearer ${authStore.token}`,
        'Content-Type': 'application/json'
      }
    });

    if (response.ok) {
      const data = await response.json();
      mfaEnabled.value = data.enabled || false;
      if (mfaEnabled.value) {
        mfaStep.value = 'enabled';
      }
    }
  } catch (err) {
    console.error('Error checking MFA status:', err);
  }
};

// Async setup function for login mode
const setupMFAData = async () => {
  if (props.isLoginSetup) {
    // In login setup mode, wait for context to be available
    const waitForContext = async (): Promise<any> => {
      return new Promise((resolve, reject) => {
        let attempts = 0;
        const maxAttempts = 30; // 3 seconds max wait
        
        const checkForContext = () => {
          const context = sessionStorage.getItem('mfaLoginSetupContext');
          if (context) {
            try {
              const parsed = JSON.parse(context);
              if (parsed.email && parsed.password) {
                resolve(parsed);
              } else {
                attempts++;
                if (attempts >= maxAttempts) {
                  reject(new Error('Timeout waiting for MFA setup context'));
                } else {
                  setTimeout(checkForContext, 100);
                }
              }
            } catch (error) {
              console.error('Failed to load MFA setup context:', error);
              reject(new Error('Invalid MFA setup context'));
            }
          } else {
            attempts++;
            if (attempts >= maxAttempts) {
              reject(new Error('Timeout waiting for MFA setup context'));
            } else {
              setTimeout(checkForContext, 100);
            }
          }
        };
        
        checkForContext();
      });
    };

    try {
      const context = await waitForContext();
      
      // Call the backend to get setup data
      const setupData = await authStore.startMfaSetupLogin(
        context.email,
        context.password
      );
      
      if (!setupData) {
        throw new Error('Failed to start MFA setup');
      }
      
      // Set up the component state
      qrCodeUrl.value = setupData.qr_code;
      mfaSecret.value = setupData.secret;
      backupCodes.value = setupData.backup_codes || [];
      mfaStep.value = 'verify';
      
      // Update the context with setup data
      sessionStorage.setItem('mfaLoginSetupContext', JSON.stringify({
        ...context,
        setupData,
        timestamp: Date.now()
      }));
      
      emit('success', 'MFA setup initiated. Please scan the QR code with your authenticator app.');
      
      return setupData;
    } catch (error) {
      emit('error', error instanceof Error ? error.message : 'Failed to load MFA setup data');
      throw error;
    }
  } else {
    // In normal settings mode, check current status
    await checkMFAStatus();
    return null;
  }
};

// Initialize based on mode
if (props.isLoginSetup) {
  // In login setup mode, make this async so Suspense works
  await setupMFAData();
} else {
  // For settings mode, check status asynchronously
  await checkMFAStatus();
}

// MFA functions
const toggleMFA = async (newValue: boolean) => {
  // Store the current state before any changes
  const currentState = mfaEnabled.value;
  
  if (currentState) {
    // Currently enabled, user wants to disable
    await disableMFA();
  } else {
    // Currently disabled, user wants to enable
    await startMFASetup();
  }
};

const startMFASetup = async () => {
  console.log('🔐 startMFASetup called, isLoginSetup:', props.isLoginSetup, 'qrCodeUrl exists:', !!qrCodeUrl.value);
  
  loading.value = true;
  mfaStep.value = 'verify'; // Set step to show verification UI
  
  try {
    let response;
    
    if (props.isLoginSetup) {
      // In login setup mode, QR code should already be set up from component initialization
      console.log('🔐 Login setup mode - QR code should already be available');
      if (!qrCodeUrl.value) {
        throw new Error('MFA setup not initialized properly');
      }
    } else {
      // Normal authenticated setup mode
      response = await fetch('/api/auth/mfa/setup', {
        method: 'POST',
        headers: {
          'Authorization': `Bearer ${authStore.token}`,
          'Content-Type': 'application/json'
        }
      });

      if (response.ok) {
        const data = await response.json();
        
        // Add a small delay to show the loading skeleton for better UX
        await new Promise(resolve => setTimeout(resolve, 600));
        
        qrCodeUrl.value = data.qr_code;
        mfaSecret.value = data.secret;
        backupCodes.value = data.backup_codes || [];
        emit('success', 'MFA setup initiated. Please scan the QR code with your authenticator app.');
      } else {
        const errorData = await response.json().catch(() => ({}));
        throw new Error(errorData.message || 'Failed to setup MFA');
      }
    }
  } catch (err) {
    emit('error', 'Failed to setup MFA');
    console.error('Error setting up MFA:', err);
    // Reset the toggle if setup failed
    mfaEnabled.value = false;
    // Clear any partial state
    qrCodeUrl.value = '';
    mfaSecret.value = '';
    backupCodes.value = [];
    showSecret.value = false;
    secretCopied.value = false;
  } finally {
    loading.value = false;
  }
};

const verifyMFA = async () => {
  console.log('🔐 verifyMFA called, isLoginSetup:', props.isLoginSetup);
  
  if (verificationCode.value.length !== 6) {
    emit('error', 'Please enter a valid 6-digit code');
    return;
  }

  if (!mfaSecret.value) {
    emit('error', 'MFA secret is missing. Please restart the setup process.');
    return;
  }

  // Clear any previous error messages when starting a new verification
  emit('success', ''); // Clear success messages
  emit('error', ''); // Clear error messages
  
  verifying.value = true;
  try {
    console.log('🔐 Verifying MFA with:', {
      token: verificationCode.value,
      secretLength: mfaSecret.value.length
    });

    let response;
    if (props.isLoginSetup) {
      // In login setup mode, use unauthenticated verification with credentials
      const context = sessionStorage.getItem('mfaLoginSetupContext');
      if (!context) {
        throw new Error('MFA setup context not found');
      }
      
      const { email, password } = JSON.parse(context);
      
      response = await fetch('/api/auth/mfa-setup-login', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json'
        },
        body: JSON.stringify({
          email,
          password,
          token: verificationCode.value,
          secret: mfaSecret.value
        })
      });
    } else {
      // Normal authenticated verification
      response = await fetch('/api/auth/mfa/verify-setup', {
        method: 'POST',
        headers: {
          'Authorization': `Bearer ${authStore.token}`,
          'Content-Type': 'application/json'
        },
        body: JSON.stringify({
          token: verificationCode.value, // Backend expects 'token' field
          secret: mfaSecret.value // Backend expects 'secret' field
        })
      });
    }

    console.log('🔐 Verify setup response status:', response.status);

    if (response.ok) {
      const verifyData = await response.json();
      console.log('🔐 Verify setup successful:', verifyData);

      // Update backup codes if returned from verification
      if (verifyData.backup_codes && verifyData.backup_codes.length > 0) {
        backupCodes.value = verifyData.backup_codes;
      }

      // Now enable MFA after successful verification
      console.log('🔐 Enabling MFA...');
      
      let enableResponse;
      if (props.isLoginSetup) {
        // In login setup mode, use the special login-enable endpoint
        const context = sessionStorage.getItem('mfaLoginSetupContext');
        if (!context) {
          throw new Error('MFA setup context not found');
        }
        
        const { email, password } = JSON.parse(context);
        
        enableResponse = await fetch('/api/auth/mfa-enable-login', {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json'
          },
          body: JSON.stringify({
            email,
            password,
            token: verificationCode.value,
            secret: mfaSecret.value,
            backup_codes: backupCodes.value
          })
        });
      } else {
        // Normal authenticated enable
        enableResponse = await fetch('/api/auth/mfa/enable', {
          method: 'POST',
          headers: {
            'Authorization': `Bearer ${authStore.token}`,
            'Content-Type': 'application/json'
          },
          body: JSON.stringify({
            token: verificationCode.value,
            secret: mfaSecret.value,
            backup_codes: backupCodes.value
          })
        });
      }

      console.log('🔐 Enable MFA response status:', enableResponse.status);

      if (enableResponse.ok) {
        const enableData = await enableResponse.json();
        console.log('🔐 MFA enabled successfully:', enableData);
        
        if (props.isLoginSetup && enableData.token) {
          // In login setup mode, the backend returns a login response
          // The verification already enabled MFA, so we just need to handle the login response
          if (enableData.token && enableData.user) {
            // Set the auth data directly since MFA is already enabled
            authStore.token = enableData.token;
            authStore.user = enableData.user;
            authStore.mfaSetupRequired = false;
            authStore.mfaUserUuid = '';
            
            // Set axios headers
            axios.defaults.headers.common['Authorization'] = `Bearer ${enableData.token}`;
            axios.defaults.headers.common['X-Auth-Provider'] = 'local';
            
            // Store in localStorage
            localStorage.setItem('token', enableData.token);
            localStorage.setItem('authProvider', 'local');
            
            // Set to success state instead of emitting success immediately
            mfaStep.value = 'success';
            mfaEnabled.value = true;
            
            // Don't emit success yet - let the user see the success screen first
          } else {
            emit('error', 'MFA enabled but login response was incomplete');
          }
        } else {
          // Normal settings mode
          mfaEnabled.value = true;
          mfaStep.value = 'enabled';
          verificationCode.value = '';
          emit('success', 'MFA enabled successfully! Your account is now more secure.');
        }
      } else {
        const enableErrorData = await enableResponse.json().catch(() => ({}));
        console.error('🔐 Enable MFA failed:', enableErrorData);
        throw new Error(enableErrorData.message || 'Failed to enable MFA after verification');
      }
    } else {
      const errorData = await response.json().catch(() => ({}));
      console.error('🔐 Verify setup failed:', errorData);
      throw new Error(errorData.message || 'Invalid verification code');
    }
  } catch (err) {
    console.error('🔐 MFA verification error:', err);
    emit('error', err instanceof Error ? err.message : 'Invalid verification code. Please try again.');
  } finally {
    verifying.value = false;
  }
};

const disableMFA = async () => {
  // Prompt for password confirmation
  const password = prompt('Please enter your password to disable MFA:');
  if (!password) {
    // User cancelled
    return;
  }

  loading.value = true;
  try {
    const response = await fetch('/api/auth/mfa/disable', {
      method: 'POST',
      headers: {
        'Authorization': `Bearer ${authStore.token}`,
        'Content-Type': 'application/json'
      },
      body: JSON.stringify({
        password: password
      })
    });

    if (response.ok) {
      mfaEnabled.value = false;
      mfaStep.value = 'setup';
      qrCodeUrl.value = '';
      verificationCode.value = '';
      backupCodes.value = [];
      mfaSecret.value = '';
      showSecret.value = false;
      secretCopied.value = false;
      emit('success', 'MFA disabled successfully');
    } else {
      const errorData = await response.json().catch(() => ({}));
      throw new Error(errorData.message || 'Failed to disable MFA');
    }
  } catch (err) {
    emit('error', err instanceof Error ? err.message : 'Failed to disable MFA');
    console.error('Error disabling MFA:', err);
    // Reset the toggle if disable failed
    mfaEnabled.value = true;
  } finally {
    loading.value = false;
  }
};

const cancelMFASetup = () => {
  mfaStep.value = 'setup';
  qrCodeUrl.value = '';
  verificationCode.value = '';
  mfaSecret.value = '';
  backupCodes.value = [];
  showSecret.value = false;
  secretCopied.value = false;
};

const completeSetup = () => {
  // Clean up session storage
  sessionStorage.removeItem('mfaLoginSetupContext');
  sessionStorage.removeItem('mfaSetupCredentials');
  
  // Emit success to trigger navigation
  emit('success', 'setup-complete');
};

// Format secret with spaces for better readability
const formatSecret = (secret: string) => {
  if (!secret) return '';
  // Add spaces every 4 characters for better readability
  return secret.replace(/(.{4})/g, '$1 ').trim();
};

// Copy secret to clipboard
const copySecret = async () => {
  if (!mfaSecret.value || secretCopied.value) return;
  
  try {
    await navigator.clipboard.writeText(mfaSecret.value);
    secretCopied.value = true;
    
    // Reset the button text after 2 seconds
    setTimeout(() => {
      secretCopied.value = false;
    }, 2000);
  } catch (err) {
    console.error('Failed to copy secret:', err);
    emit('error', 'Failed to copy to clipboard');
  }
};

// Expose methods for parent component access
defineExpose({
  startMFASetup
});
</script>

<style scoped>
/* Smooth fade-in animation for loaded content */
.fade-in {
  animation: fadeIn 0.6s ease-in-out;
}

@keyframes fadeIn {
  from {
    opacity: 0;
    transform: scale(0.95);
  }
  to {
    opacity: 1;
    transform: scale(1);
  }
}

/* Enhanced skeleton loading animations */
@keyframes shimmer {
  0% {
    transform: translateX(-100%);
  }
  100% {
    transform: translateX(100%);
  }
}

.skeleton-shimmer {
  position: relative;
  overflow: hidden;
}

.skeleton-shimmer::after {
  content: '';
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
  left: 0;
  background: linear-gradient(
    90deg,
    transparent,
    rgba(255, 255, 255, 0.1),
    transparent
  );
  animation: shimmer 1.5s infinite;
}

/* QR Code pattern animation */
@keyframes qrPattern {
  0%, 100% {
    opacity: 0.3;
    transform: scale(1);
  }
  50% {
    opacity: 0.6;
    transform: scale(1.02);
  }
}
</style>

<template>
  <div class="bg-slate-800 rounded-xl border border-slate-700/50 hover:border-slate-600/50 transition-colors">
    <div class="px-4 py-3 bg-slate-700/30 border-b border-slate-700/50">
      <h2 class="text-lg font-medium text-white">Two-Factor Authentication</h2>
      <p class="text-sm text-slate-400 mt-1">Add an extra layer of security to your account</p>
    </div>
    
    <div class="p-6">
      <div class="flex flex-col gap-4">
        <!-- MFA Toggle / Status (hidden in login setup mode) -->
        <div v-if="!props.isLoginSetup" class="bg-slate-700/50 rounded-lg border border-slate-600/30 hover:border-slate-500/50 transition-colors p-4">
          <ToggleSwitch
            :modelValue="mfaEnabled"
            :disabled="loading"
            label="Enable Two-Factor Authentication"
            :description="mfaEnabled ? 'Your account is protected with 2FA' : 'Secure your account with an authenticator app'"
            @update:modelValue="toggleMFA"
          />
        </div>

        <!-- Setup Steps (hidden in login setup mode) -->
        <div v-if="showSetupSteps && !props.isLoginSetup" class="bg-slate-700/30 rounded-lg border border-slate-600/20 p-4">
          <h3 class="text-sm font-medium text-white mb-4">How to set up 2FA:</h3>
          <ol class="flex flex-col gap-3 text-sm text-slate-300">
            <li class="flex items-start gap-3">
              <span class="bg-blue-600 text-white rounded-full w-5 h-5 flex items-center justify-center text-xs font-medium mt-0.5 flex-shrink-0">1</span>
              <span>Download an authenticator app like Google Authenticator or Authy</span>
            </li>
            <li class="flex items-start gap-3">
              <span class="bg-blue-600 text-white rounded-full w-5 h-5 flex items-center justify-center text-xs font-medium mt-0.5 flex-shrink-0">2</span>
              <span>Click the toggle above to start the setup process</span>
            </li>
            <li class="flex items-start gap-3">
              <span class="bg-blue-600 text-white rounded-full w-5 h-5 flex items-center justify-center text-xs font-medium mt-0.5 flex-shrink-0">3</span>
              <span>Scan the QR code with your authenticator app</span>
            </li>
            <li class="flex items-start gap-3">
              <span class="bg-blue-600 text-white rounded-full w-5 h-5 flex items-center justify-center text-xs font-medium mt-0.5 flex-shrink-0">4</span>
              <span>Enter the 6-digit code to complete setup</span>
            </li>
          </ol>
        </div>

              <!-- Verification Step -->
        <div v-if="showVerificationStep" class="bg-slate-700/30 rounded-lg border border-slate-600/20 p-6">
          <div class="flex flex-col gap-6">
            <!-- Header Section -->
            <div>
              <h3 class="text-sm font-medium text-white mb-2">Scan QR Code</h3>
              <p class="text-sm text-slate-400">Scan this QR code with your authenticator app:</p>
            </div>

            <!-- Loading State with Skeletons -->
            <div v-if="(loading || !qrCodeUrl) && !verifying" class="grid grid-cols-1 lg:grid-cols-[auto_1fr] gap-8 items-start">
              <!-- QR Code Loading Skeleton -->
              <div class="flex justify-center lg:justify-start">
                <div class="bg-white p-4 rounded-lg shadow-lg">
                  <div class="w-48 h-48 lg:w-44 lg:h-44 bg-gradient-to-br from-gray-50 to-gray-100 rounded-lg flex items-center justify-center relative overflow-hidden">
                    <!-- Shimmer Effect -->
                    <div class="absolute inset-0 skeleton-shimmer"></div>
                    
                    <!-- Simulated QR Code Pattern -->
                    <div class="absolute inset-2 grid grid-cols-10 gap-px opacity-20">
                      <div v-for="i in 100" :key="i" 
                           class="bg-gray-400 rounded-[1px]" 
                           :class="{ 'opacity-0': Math.random() > 0.4 }"
                           :style="{ 
                             animationDelay: `${i * 15}ms`,
                             animationName: 'qrPattern',
                             animationDuration: '2s',
                             animationIterationCount: 'infinite'
                           }">
                      </div>
                    </div>
                    
                    <!-- Three corner markers simulation -->
                    <div class="absolute top-2 left-2 w-6 h-6 border-2 border-gray-400 opacity-30 animate-pulse">
                      <div class="w-2 h-2 bg-gray-400 m-auto mt-1"></div>
                    </div>
                    <div class="absolute top-2 right-2 w-6 h-6 border-2 border-gray-400 opacity-30 animate-pulse" style="animation-delay: 0.5s">
                      <div class="w-2 h-2 bg-gray-400 m-auto mt-1"></div>
                    </div>
                    <div class="absolute bottom-2 left-2 w-6 h-6 border-2 border-gray-400 opacity-30 animate-pulse" style="animation-delay: 1s">
                      <div class="w-2 h-2 bg-gray-400 m-auto mt-1"></div>
                    </div>
                    
                    <!-- Loading Text -->
                    <div class="absolute bottom-4 left-1/2 transform -translate-x-1/2">
                      <div class="flex items-center gap-2 text-gray-500 text-xs">
                        <svg class="w-3 h-3 animate-spin" fill="none" viewBox="0 0 24 24">
                          <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                          <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                        </svg>
                        <span>Generating...</span>
                      </div>
                    </div>
                  </div>
                </div>
              </div>

              <!-- Loading Verification Components -->
              <div class="flex flex-col gap-6">
                <!-- Manual Secret Entry Skeleton -->
                <div class="bg-slate-800/50 rounded-lg border border-slate-600/20 p-4">
                  <div class="flex items-center gap-2">
                    <div class="w-4 h-4 bg-slate-600 rounded animate-pulse skeleton-shimmer"></div>
                    <div class="h-4 bg-slate-600 rounded flex-1 animate-pulse skeleton-shimmer" style="animation-delay: 0.2s"></div>
                  </div>
                  
                  <!-- Expandable content skeleton (collapsed state) -->
                  <div class="mt-3 space-y-2 opacity-50">
                    <div class="h-3 bg-slate-700/50 rounded w-5/6 animate-pulse skeleton-shimmer" style="animation-delay: 0.4s"></div>
                    <div class="bg-slate-700/30 rounded-lg p-3 border border-slate-600/10">
                      <div class="flex items-center justify-between gap-3">
                        <div class="h-3 bg-slate-700/70 rounded flex-1 font-mono skeleton-shimmer" style="animation-delay: 0.6s"></div>
                        <div class="w-12 h-6 bg-slate-700/70 rounded skeleton-shimmer" style="animation-delay: 0.8s"></div>
                      </div>
                    </div>
                  </div>
                </div>
                
                <!-- Verification Input Skeleton -->
                <div class="flex flex-col gap-4">
                  <div class="space-y-2">
                    <div class="h-4 bg-slate-600 rounded w-3/4 animate-pulse skeleton-shimmer" style="animation-delay: 1s"></div>
                    <div class="h-4 bg-slate-600 rounded w-full animate-pulse skeleton-shimmer" style="animation-delay: 1.2s"></div>
                  </div>
                  
                  <div class="flex gap-3">
                    <div class="bg-slate-700/50 rounded-lg border border-slate-600/30 flex-1 p-3">
                      <!-- Simulated 6-digit input placeholder -->
                      <div class="flex justify-center gap-2">
                        <div v-for="i in 6" :key="i" 
                             class="w-6 h-6 bg-slate-600 rounded animate-pulse skeleton-shimmer" 
                             :style="{ animationDelay: `${1.4 + i * 0.1}s` }">
                        </div>
                      </div>
                    </div>
                    <div class="px-6 py-3 bg-blue-600/50 rounded-lg animate-pulse skeleton-shimmer" style="animation-delay: 2s">
                      <div class="w-12 h-6 bg-blue-500/50 rounded"></div>
                    </div>
                  </div>
                  
                  <div class="h-3 bg-slate-600 rounded w-24 animate-pulse skeleton-shimmer" style="animation-delay: 2.2s"></div>
                </div>
                
                <!-- Loading Message -->
                <div class="text-center py-4">
                  <div class="flex items-center justify-center gap-2 text-slate-400">
                    <svg class="w-4 h-4 animate-spin" fill="none" viewBox="0 0 24 24">
                      <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                      <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                    </svg>
                    <span class="text-sm">Preparing your MFA setup...</span>
                  </div>
                </div>
              </div>
            </div>

            <!-- Verification Loading State -->
            <div v-if="verifying" class="flex flex-col gap-6">
              <div class="flex items-center justify-center py-8">
                <div class="flex flex-col items-center gap-4">
                  <div class="bg-blue-600 rounded-full p-4">
                    <svg class="w-8 h-8 text-white animate-spin" fill="none" viewBox="0 0 24 24">
                      <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                      <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                    </svg>
                  </div>
                  <div class="text-center">
                    <h3 class="text-lg font-medium text-white mb-2">Verifying Code</h3>
                    <p class="text-sm text-slate-400">Please wait while we verify your authenticator code...</p>
                  </div>
                </div>
              </div>
            </div>

            <!-- Actual QR Code and Verification Grid -->
            <div v-else class="grid grid-cols-1 lg:grid-cols-[auto_1fr] gap-8 items-start">
              <!-- QR Code Section -->
              <div class="flex justify-center lg:justify-start">
                <div class="bg-white p-4 rounded-lg">
                  <img 
                    :src="qrCodeUrl" 
                    alt="MFA QR Code" 
                    class="w-48 h-48 lg:w-44 lg:h-44 fade-in"
                  />
                </div>
              </div>

              <!-- Verification Components -->
              <div class="flex flex-col gap-6">
              <!-- Manual Secret Entry Option -->
              <div class="bg-slate-800/50 rounded-lg border border-slate-600/20 p-4">
                <button
                  @click="showSecret = !showSecret"
                  class="flex items-center gap-2 text-sm text-slate-400 hover:text-white transition-colors"
                >
                  <svg 
                    xmlns="http://www.w3.org/2000/svg" 
                    class="h-4 w-4 transition-transform"
                    :class="{ 'rotate-90': showSecret }"
                    fill="none" 
                    viewBox="0 0 24 24" 
                    stroke="currentColor"
                  >
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
                  </svg>
                  Can't scan? Enter the code manually
                </button>
                
                <div v-if="showSecret" class="mt-4 flex flex-col gap-3">
                  <p class="text-sm text-slate-400">Enter this secret key in your authenticator app:</p>
                  <div class="bg-slate-700/50 rounded-lg p-3 border border-slate-600/30">
                    <div class="flex items-center justify-between gap-3">
                      <code class="text-sm font-mono text-green-400 select-all flex-1 break-all">{{ formatSecret(mfaSecret) }}</code>
                      <button
                        @click="copySecret"
                        :disabled="secretCopied"
                        class="px-3 py-1 text-xs rounded transition-all duration-200 flex-shrink-0"
                        :class="secretCopied 
                          ? 'bg-green-600 text-white cursor-default' 
                          : 'bg-slate-600 text-white hover:bg-slate-500 cursor-pointer'"
                        :title="secretCopied ? 'Copied to clipboard!' : 'Copy to clipboard'"
                      >
                        {{ secretCopied ? 'Copied!' : 'Copy' }}
                      </button>
                    </div>
                  </div>
                </div>
              </div>
              
              <!-- Verification Input Section -->
              <div class="flex flex-col gap-4">
                <div>
                  <h4 class="text-sm font-medium text-white mb-2">Enter Verification Code</h4>
                  <p class="text-sm text-slate-400">Enter the 6-digit code from your authenticator app:</p>
                </div>
                
                <div class="flex gap-3">
                  <div class="bg-slate-700/50 rounded-lg border border-slate-600/30 hover:border-slate-500/50 transition-colors flex-1">
                    <input
                      v-model="verificationCode"
                      type="text"
                      maxlength="6"
                      class="w-full px-4 py-3 bg-transparent text-white rounded-lg focus:ring-2 focus:ring-blue-500 focus:outline-none text-center tracking-widest"
                      placeholder="000000"
                    />
                  </div>
                  <button
                    @click="verifyMFA"
                    :disabled="verificationCode.length !== 6 || verifying"
                    class="px-6 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 disabled:opacity-50 disabled:cursor-not-allowed disabled:bg-slate-600 flex items-center transition-colors"
                  >
                    <span v-if="verifying" class="animate-spin h-4 w-4 mr-2">
                      <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                        <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                        <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 0 1 8-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 0 1 4 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                      </svg>
                    </span>
                    {{ verifying ? 'Verifying...' : 'Verify' }}
                  </button>
                </div>
                
                <button
                  @click="cancelMFASetup"
                  class="self-start text-sm text-slate-400 hover:text-white transition-colors"
                >
                  Cancel setup
                </button>
              </div>
            </div>
          </div>
        </div>
        </div>

        <!-- Backup Codes Display -->
        <div v-if="backupCodes.length > 0 && mfaStep === 'verify'" class="bg-amber-600/10 border border-amber-600/20 rounded-lg p-4">
          <div class="flex flex-col gap-4">
            <div>
              <h4 class="text-sm font-medium text-amber-400 mb-2">Backup Codes</h4>
              <p class="text-sm text-slate-300">Save these backup codes in a secure location. You can use them to access your account if you lose your authenticator device:</p>
            </div>
            <div class="bg-slate-700/50 rounded-lg p-4 font-mono text-sm text-white">
              <div class="grid grid-cols-2 gap-2">
                <div v-for="code in backupCodes" :key="code" class="text-center p-2 bg-slate-800/50 rounded">{{ code }}</div>
              </div>
            </div>
            <p class="text-xs text-amber-400 flex items-center gap-2">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L4.082 16.5c-.77.833.192 2.5 1.732 2.5z" />
              </svg>
              Each backup code can only be used once.
            </p>
          </div>
        </div>

        <!-- Success State (for login setup) -->
        <div v-if="showSuccessState" class="bg-green-600/10 border border-green-600/20 rounded-lg p-6">
          <div class="flex flex-col gap-4">
            <div class="flex items-center gap-3">
              <div class="bg-green-600 rounded-full p-2">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-6 w-6 text-white" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
                </svg>
              </div>
              <div>
                <h3 class="text-lg font-medium text-green-400">Two-Factor Authentication Enabled!</h3>
                <p class="text-sm text-slate-300">Your account is now protected with 2FA. You'll need to enter a code from your authenticator app when signing in.</p>
              </div>
            </div>
            
            <!-- Backup Codes Section -->
            <div v-if="backupCodes.length > 0" class="bg-slate-700/50 rounded-lg p-4">
              <div class="flex flex-col gap-3">
                <div class="flex items-center gap-2">
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 text-amber-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L4.082 16.5c-.77.833.192 2.5 1.732 2.5z" />
                  </svg>
                  <h4 class="text-sm font-medium text-amber-400">Save Your Backup Codes</h4>
                </div>
                <p class="text-sm text-slate-300">Store these backup codes in a secure location. You can use them to access your account if you lose your authenticator device:</p>
                <div class="bg-slate-800/50 rounded-lg p-4 font-mono text-sm text-white">
                  <div class="grid grid-cols-2 gap-2">
                    <div v-for="code in backupCodes" :key="code" class="text-center p-2 bg-slate-700/50 rounded">{{ code }}</div>
                  </div>
                </div>
                <p class="text-xs text-amber-400 flex items-center gap-2">
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L4.082 16.5c-.77.833.192 2.5 1.732 2.5z" />
                  </svg>
                  Each backup code can only be used once.
                </p>
              </div>
            </div>
            
            <!-- Action Button -->
            <div class="flex justify-center pt-2">
              <button
                @click="completeSetup"
                class="px-8 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 transition-colors font-medium"
              >
                Start Using Nosdesk!
              </button>
            </div>
          </div>
        </div>

        <!-- Enabled State -->
        <div v-if="showEnabledState" class="bg-green-600/10 border border-green-600/20 rounded-lg p-4">
          <div class="flex flex-col gap-1">
            <div class="flex items-center gap-1">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 text-green-400 flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
              </svg>
              <h3 class="text-sm font-medium text-green-400">2FA is enabled</h3>
            </div>
            <p class="text-sm text-slate-300">Your account is protected with two-factor authentication. You'll need to enter a code from your authenticator app when signing in.</p>
          </div>
        </div>
      </div>
    </div>
  </div>
</template> 