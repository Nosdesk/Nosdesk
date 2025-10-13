<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import ToggleSwitch from '@/components/common/ToggleSwitch.vue';
import { useAuthStore } from '@/stores/auth';
import axios from 'axios';

// MFA state
const mfaEnabled = ref(false);
const mfaStep = ref<'setup' | 'verify' | 'enabled' | 'success'>('setup');
const qrCodeUrl = ref('');
const verificationCode = ref('');
const loading = ref(false);
const verifying = ref(false);
const backupCodes = ref<string[]>([]);
const mfaSecret = ref('');
const showSecret = ref(false);
const secretCopied = ref(false);

const authStore = useAuthStore();

// Props for different modes
const props = defineProps<{
  isLoginSetup?: boolean;
}>();

// Emits for notifications
const emit = defineEmits<{
  (e: 'success', message: string): void;
  (e: 'error', message: string): void;
}>();

// Computed properties - simplified and optimized
const showSetupSteps = computed(() => !mfaEnabled.value && mfaStep.value === 'setup');
const showVerificationStep = computed(() => !mfaEnabled.value && mfaStep.value === 'verify');
const showEnabledState = computed(() => mfaEnabled.value && !props.isLoginSetup);
const showSuccessState = computed(() => mfaStep.value === 'success');
const isInSuccessState = computed(() => mfaStep.value === 'success');

// Computed for conditional rendering
const shouldShowSetupInterface = computed(() => {
  // For login setup mode, show interface immediately
  if (props.isLoginSetup) {
    return !mfaEnabled.value;
  }
  // For normal mode, only show when in verify step (after user clicks toggle)
  return !mfaEnabled.value && mfaStep.value === 'verify';
});

// Static data for setup steps and QR markers
const setupSteps = [
  'Download an authenticator app like Google Authenticator or Authy',
  'Click the toggle above to start the setup process',
  'Scan the QR code with your authenticator app',
  'Enter the 6-digit code to complete setup'
];

const qrMarkers = [
  { position: 'top-2 left-2' },
  { position: 'top-2 right-2' },
  { position: 'bottom-2 left-2' }
];

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
    const context = await waitForContext();
    const setupData = await authStore.startMfaSetupLogin(context.email, context.password);
    
    if (!setupData) {
      throw new Error('Failed to start MFA setup');
    }
    
    // Set up the component state
    qrCodeUrl.value = setupData.qr_code;
    mfaSecret.value = setupData.secret;
    backupCodes.value = [];
    mfaStep.value = 'verify';
    
    // Update the context with setup data
    sessionStorage.setItem('mfaLoginSetupContext', JSON.stringify({
      ...context,
      setupData,
      timestamp: Date.now()
    }));
    
    emit('success', 'MFA setup initiated. Please scan the QR code with your authenticator app.');
    return setupData;
  } else {
    await checkMFAStatus();
    return null;
  }
};

// Helper function for waiting for context
const waitForContext = async (): Promise<any> => {
  return new Promise((resolve, reject) => {
    let attempts = 0;
    const maxAttempts = 30;
    
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

// Initialize based on mode - moved to onMounted to avoid top-level await
onMounted(async () => {
  if (props.isLoginSetup) {
    try {
      await setupMFAData();
    } catch (error) {
      console.error('Failed to initialize MFA setup:', error);
      emit('error', 'Failed to initialize MFA setup');
    }
  } else {
    await checkMFAStatus();
  }
});

// MFA functions
const toggleMFA = async (newValue: boolean) => {
  const currentState = mfaEnabled.value;
  
  if (currentState) {
    await disableMFA();
  } else {
    await startMFASetup();
  }
};

const startMFASetup = async () => {
  console.log('🔐 startMFASetup called, isLoginSetup:', props.isLoginSetup, 'qrCodeUrl exists:', !!qrCodeUrl.value);
  
  loading.value = true;
  mfaStep.value = 'verify';
  
  try {
    if (props.isLoginSetup) {
      if (!qrCodeUrl.value) {
        throw new Error('MFA setup not initialized properly');
      }
    } else {
      const response = await fetch('/api/auth/mfa/setup', {
        method: 'POST',
        headers: {
          'Authorization': `Bearer ${authStore.token}`,
          'Content-Type': 'application/json'
        }
      });

      if (response.ok) {
        const data = await response.json();
        await new Promise(resolve => setTimeout(resolve, 600));
        
        qrCodeUrl.value = data.qr_code;
        mfaSecret.value = data.secret;
        backupCodes.value = [];
        emit('success', 'MFA setup initiated. Please scan the QR code with your authenticator app.');
      } else {
        const errorData = await response.json().catch(() => ({}));
        throw new Error(errorData.message || 'Failed to setup MFA');
      }
    }
  } catch (err) {
    emit('error', 'Failed to setup MFA');
    console.error('Error setting up MFA:', err);
    resetMFASetup();
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

  emit('success', '');
  emit('error', '');
  
  verifying.value = true;
  try {
    const response = await performVerification();
    if (response.ok) {
      await enableMFA();
    } else {
      const errorData = await response.json().catch(() => ({}));
      throw new Error(errorData.message || 'Invalid verification code');
    }
  } catch (err) {
    console.error('🔐 MFA verification error:', err);
    emit('error', err instanceof Error ? err.message : 'Invalid verification code. Please try again.');
  } finally {
    verifying.value = false;
  }
};

const performVerification = async () => {
  if (props.isLoginSetup) {
    const context = sessionStorage.getItem('mfaLoginSetupContext');
    if (!context) {
      throw new Error('MFA setup context not found');
    }
    
    const { email, password } = JSON.parse(context);
    
    return await fetch('/api/auth/mfa-setup-login', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        email,
        password,
        token: verificationCode.value,
        secret: mfaSecret.value
      })
    });
  } else {
    return await fetch('/api/auth/mfa/verify-setup', {
      method: 'POST',
      headers: {
        'Authorization': `Bearer ${authStore.token}`,
        'Content-Type': 'application/json'
      },
      body: JSON.stringify({
        token: verificationCode.value,
        secret: mfaSecret.value
      })
    });
  }
};

const enableMFA = async () => {
  console.log('🔐 Enabling MFA...');
  
  let enableResponse;
  if (props.isLoginSetup) {
    const context = sessionStorage.getItem('mfaLoginSetupContext');
    if (!context) {
      throw new Error('MFA setup context not found');
    }
    
    const { email, password } = JSON.parse(context);
    
    enableResponse = await fetch('/api/auth/mfa-enable-login', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        email,
        password,
        token: verificationCode.value,
        secret: mfaSecret.value
      })
    });
  } else {
    enableResponse = await fetch('/api/auth/mfa/enable', {
      method: 'POST',
      headers: {
        'Authorization': `Bearer ${authStore.token}`,
        'Content-Type': 'application/json'
      },
      body: JSON.stringify({
        token: verificationCode.value,
        secret: mfaSecret.value
      })
    });
  }

  console.log('🔐 Enable MFA response status:', enableResponse.status);

  if (enableResponse.ok) {
    const enableData = await enableResponse.json();
    console.log('🔐 MFA enabled successfully:', enableData);
    
    if (enableData.backup_codes && Array.isArray(enableData.backup_codes)) {
      backupCodes.value = enableData.backup_codes;
    }
    
    if (props.isLoginSetup && enableData.token) {
      handleLoginSetupSuccess(enableData);
    } else {
      handleNormalSetupSuccess();
    }
  } else {
    const enableErrorData = await enableResponse.json().catch(() => ({}));
    console.error('🔐 Enable MFA failed:', enableErrorData);
    throw new Error(enableErrorData.message || 'Failed to enable MFA after verification');
  }
};

const handleLoginSetupSuccess = (enableData: any) => {
  if (enableData.token && enableData.user) {
    authStore.token = enableData.token;
    authStore.user = enableData.user;
    authStore.mfaSetupRequired = false;
    authStore.mfaUserUuid = '';
    
    axios.defaults.headers.common['Authorization'] = `Bearer ${enableData.token}`;
    axios.defaults.headers.common['X-Auth-Provider'] = 'local';
    
    localStorage.setItem('token', enableData.token);
    localStorage.setItem('authProvider', 'local');
    
    mfaStep.value = 'success';
    mfaEnabled.value = true;
  } else {
    emit('error', 'MFA enabled but login response was incomplete');
  }
};

const handleNormalSetupSuccess = () => {
  mfaEnabled.value = true;
  mfaStep.value = 'enabled';
  verificationCode.value = '';
  emit('success', 'MFA enabled successfully! Your account is now more secure.');
};

const disableMFA = async () => {
  const password = prompt('Please enter your password to disable MFA:');
  if (!password) return;

  loading.value = true;
  try {
    const response = await fetch('/api/auth/mfa/disable', {
      method: 'POST',
      headers: {
        'Authorization': `Bearer ${authStore.token}`,
        'Content-Type': 'application/json'
      },
      body: JSON.stringify({ password })
    });

    if (response.ok) {
      resetMFASetup();
      emit('success', 'MFA disabled successfully');
    } else {
      const errorData = await response.json().catch(() => ({}));
      throw new Error(errorData.message || 'Failed to disable MFA');
    }
  } catch (err) {
    emit('error', err instanceof Error ? err.message : 'Failed to disable MFA');
    console.error('Error disabling MFA:', err);
    mfaEnabled.value = true;
  } finally {
    loading.value = false;
  }
};

const resetMFASetup = () => {
  mfaEnabled.value = false;
  mfaStep.value = 'setup';
  qrCodeUrl.value = '';
  verificationCode.value = '';
  backupCodes.value = [];
  mfaSecret.value = '';
  showSecret.value = false;
  secretCopied.value = false;
};

const cancelMFASetup = () => {
  resetMFASetup();
};

const completeSetup = () => {
  sessionStorage.removeItem('mfaLoginSetupContext');
  sessionStorage.removeItem('mfaSetupCredentials');
  emit('success', 'setup-complete');
};

// Format secret with spaces for better readability
const formatSecret = (secret: string) => {
  if (!secret) return '';
  return secret.replace(/(.{4})/g, '$1 ').trim();
};

// Copy secret to clipboard
const copySecret = async () => {
  if (!mfaSecret.value || secretCopied.value) return;
  
  try {
    await navigator.clipboard.writeText(mfaSecret.value);
    secretCopied.value = true;
    
    setTimeout(() => {
      secretCopied.value = false;
    }, 2000);
  } catch (err) {
    console.error('Failed to copy secret:', err);
    emit('error', 'Failed to copy to clipboard');
  }
};

// Download backup codes as text file
const downloadBackupCodes = () => {
  if (!backupCodes.value.length) return;
  
  try {
    const content = `Nosdesk Backup Codes

IMPORTANT: Save these backup codes in a secure location.
Each code can only be used once to access your account if you lose your authenticator device.

Backup Codes:
${backupCodes.value.map((code, index) => `${index + 1}. ${code}`).join('\n')}

Generated on: ${new Date().toLocaleString()}
Account: ${authStore.user?.email || 'Unknown'}

Security Notice:
- Store these codes securely and privately
- Do not share them with anyone
- Each code can only be used once`;

    const blob = new Blob([content], { type: 'text/plain' });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = `nosdesk-backup-codes-${new Date().toISOString().split('T')[0]}.txt`;
    
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
    
    URL.revokeObjectURL(url);
    emit('success', 'Backup codes downloaded successfully');
  } catch (err) {
    console.error('Failed to download backup codes:', err);
    emit('error', 'Failed to download backup codes');
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
      <h2 class="text-lg font-medium text-white">
        {{ isInSuccessState ? 'Setup Complete!' : 'Two-Factor Authentication' }}
      </h2>
      <p class="text-sm text-slate-400 mt-1">
        {{ isInSuccessState ? 'Your account is now protected with 2FA' : 'Add an extra layer of security to your account' }}
      </p>
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
            <li v-for="(step, index) in setupSteps" :key="index" class="flex items-start gap-3">
              <span class="bg-blue-600 text-white rounded-full w-5 h-5 flex items-center justify-center text-xs font-medium mt-0.5 flex-shrink-0">{{ index + 1 }}</span>
              <span>{{ step }}</span>
            </li>
          </ol>
        </div>

        <!-- Main MFA Setup Component - Hidden when verification is successful -->
        <div v-if="shouldShowSetupInterface && !verifying" class="grid grid-cols-1 lg:grid-cols-[auto_1fr] gap-8 items-start">
          <!-- QR Code Section -->
          <div class="flex justify-center lg:justify-start">
            <div class="bg-white p-4 rounded-lg shadow-lg">
              <!-- QR Code Skeleton when loading -->
              <div v-if="loading || !qrCodeUrl" class="w-48 h-48 lg:w-44 lg:h-44 bg-gradient-to-br from-gray-50 to-gray-100 rounded-lg flex items-center justify-center relative overflow-hidden">
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
                <div v-for="(marker, index) in qrMarkers" :key="index"
                     class="absolute w-6 h-6 border-2 border-gray-400 opacity-30 animate-pulse"
                     :class="marker.position"
                     :style="{ animationDelay: `${index * 0.5}s` }">
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
              
              <!-- Actual QR Code when loaded -->
              <img 
                v-else
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

              <div class="flex flex-col sm:flex-row gap-3">
                <div class="bg-slate-700/50 rounded-lg border border-slate-600/30 hover:border-slate-500/50 transition-colors flex-1">
                  <input
                    v-model="verificationCode"
                    type="text"
                    maxlength="6"
                    class="w-full px-4 py-3 bg-transparent text-white rounded-lg focus:ring-2 focus:ring-blue-500 focus:outline-none text-center tracking-widest text-lg sm:text-base"
                    placeholder="000000"
                  />
                </div>
                <button
                  @click="verifyMFA"
                  :disabled="verificationCode.length !== 6 || verifying"
                  class="px-6 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 disabled:opacity-50 disabled:cursor-not-allowed disabled:bg-slate-600 flex items-center justify-center transition-colors min-h-[52px] active:scale-[0.98]"
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
                class="self-start text-sm text-slate-400 hover:text-white transition-colors min-h-[44px] px-2"
              >
                Cancel setup
              </button>
            </div>
          </div>
        </div>

        <!-- Verification Loading State - Replaces the setup interface when verifying -->
        <div v-if="shouldShowSetupInterface && verifying" class="grid grid-cols-1 lg:grid-cols-[auto_1fr] gap-8 items-start">
          <!-- QR Code Section (keep visible during verification) -->
          <div class="flex justify-center lg:justify-start">
            <div class="bg-white p-4 rounded-lg shadow-lg">
              <img 
                v-if="qrCodeUrl"
                :src="qrCodeUrl" 
                alt="MFA QR Code" 
                class="w-48 h-48 lg:w-44 lg:h-44"
              />
            </div>
          </div>

          <!-- Loading State in place of verification components -->
          <div class="flex flex-col gap-6 justify-center">
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
        </div>



        <!-- Backup Codes Display: only show after success or enabled -->
        <div v-if="backupCodes.length > 0 && (mfaStep === 'success' || mfaEnabled)" class="bg-amber-600/10 border border-amber-600/20 rounded-lg p-4">
          <div class="flex flex-col gap-4">
            <div class="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-3">
              <div class="min-w-0 flex-1">
                <h4 class="text-sm font-medium text-amber-400 mb-2">Backup Codes</h4>
                <p class="text-sm text-slate-300">Save these backup codes in a secure location. You can use them to access your account if you lose your authenticator device:</p>
              </div>
              <button
                @click="downloadBackupCodes"
                class="flex items-center gap-2 px-3 py-2 bg-amber-600/20 hover:bg-amber-600/30 text-amber-400 text-sm rounded-lg border border-amber-600/30 hover:border-amber-600/50 transition-colors min-h-[44px] flex-shrink-0 active:scale-[0.98]"
                title="Download backup codes as text file"
              >
                <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 10v6m0 0l-3-3m3 3l3-3m2 8H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
                </svg>
                Download
              </button>
            </div>
            <div class="bg-slate-700/50 rounded-lg p-3 sm:p-4 font-mono text-xs sm:text-sm text-white">
              <div class="grid grid-cols-1 sm:grid-cols-2 gap-2">
                <div v-for="code in backupCodes" :key="code" class="text-center p-2 bg-slate-800/50 rounded break-all">{{ code }}</div>
              </div>
            </div>
            <p class="text-xs text-amber-400 flex items-center gap-2">
              <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.500c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L4.082 16.5c-.77.833.192 2.5 1.732 2.5z" />
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
            
            <!-- Action Button -->
            <div class="flex justify-center pt-2">
              <button
                @click="completeSetup"
                class="w-full sm:w-auto px-8 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 transition-colors font-medium min-h-[52px] active:scale-[0.98]"
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