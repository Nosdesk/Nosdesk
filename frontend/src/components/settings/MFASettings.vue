<script setup lang="ts">
import { logger } from "@/utils/logger";
import { ref, computed, onMounted, onUnmounted, watch } from "vue";
import ToggleSwitch from "@/components/common/ToggleSwitch.vue";
import OtpInput from "@/components/common/OtpInput.vue";
import { useAuthStore } from "@/stores/auth";
import { useMfaSetupStore } from "@/stores/mfaSetup";
import { useMfa } from "@/composables/useMfa";

// Props for different modes
const props = defineProps<{
    isLoginSetup?: boolean;
    limitedSessionToken?: string;
}>();

// Emits for notifications
const emit = defineEmits<{
    (e: "success", message: string): void;
    (e: "error", message: string): void;
    (e: "mfa-disabled"): void;
    (e: "mfa-enabled"): void;
}>();

// Use MFA composable - follows Vue 3 best practices
const mfa = useMfa({ isLoginSetup: props.isLoginSetup });

// Auth store for user data
const authStore = useAuthStore();

// Secure MFA setup store for credentials
const mfaSetupStore = useMfaSetupStore();

// UI-specific state
const verificationCode = ref("");
const showSecret = ref(false);
const secretCopied = ref(false);

// QR code rendering: Single grid that shows skeleton pattern initially,
// then smoothly transitions when real data arrives. Cells animate in
// radially from center, creating a generative effect.

// Handle OTP complete (auto-submit)
const handleOtpComplete = () => {
    setTimeout(() => verifyMFA(), 100);
};

// Check if using limited session (for conditional password requirements)
const isLimitedSession = computed(() => {
    return !!props.limitedSessionToken;
});

// Computed properties - simplified and optimized
const isInSuccessState = computed(() => mfa.mfaStep.value === "success");

// Computed for conditional rendering
const shouldShowSetupInterface = computed(() => {
    // For login setup mode, show interface immediately
    if (props.isLoginSetup) {
        return !mfa.mfaEnabled.value;
    }
    // For normal mode, only show when in verify step (after user clicks toggle)
    return !mfa.mfaEnabled.value && mfa.mfaStep.value === "verify";
});

// Static data for setup steps
const setupSteps = [
    "Download an authenticator app like Google Authenticator or Authy",
    "Click the toggle above to start the setup process",
    "Scan the QR code with your authenticator app",
    "Enter the 6-digit code to complete setup",
];

// Wrapper methods that emit events based on composable state
const emitMfaMessages = () => {
    if (mfa.error.value) {
        emit("error", mfa.error.value);
    }
    if (mfa.successMessage.value) {
        emit("success", mfa.successMessage.value);
    }
};

// Async setup function for login mode
const setupMFAData = async () => {
    if (props.isLoginSetup) {
        const creds = await waitForCredentials();
        const setupData = await mfa.setupMFAForLogin(
            creds.email,
            creds.password,
        );

        if (!setupData) {
            throw new Error("Failed to start MFA setup");
        }

        emitMfaMessages();
        return setupData;
    } else {
        await mfa.checkMFAStatus();
        emitMfaMessages();
        return null;
    }
};

// Helper function for waiting for credentials from secure store
const waitForCredentials = async (): Promise<{ email: string; password: string }> => {
    return new Promise((resolve, reject) => {
        let attempts = 0;
        const maxAttempts = 30;

        const checkForCredentials = () => {
            if (mfaSetupStore.hasValidCredentials) {
                const creds = mfaSetupStore.getCredentials;
                if (creds) {
                    resolve({ email: creds.email, password: creds.password });
                    return;
                }
            }

            attempts++;
            if (attempts >= maxAttempts) {
                reject(new Error("Timeout waiting for MFA setup credentials"));
            } else {
                setTimeout(checkForCredentials, 100);
            }
        };

        checkForCredentials();
    });
};

// Initialize based on mode
onMounted(async () => {
    if (props.isLoginSetup) {
        try {
            await setupMFAData();
        } catch (error) {
            logger.error("Failed to initialize MFA setup:", error);
            emit("error", "Failed to initialize MFA setup");
        }
    } else {
        await mfa.checkMFAStatus();
        emitMfaMessages();
    }
});

// MFA action methods using composable
const toggleMFA = async (newValue: boolean) => {
    if (mfa.mfaEnabled.value) {
        await disableMFA();
    } else {
        await startMFASetup();
    }
};

const startMFASetup = async () => {
    logger.debug(
        "🔐 startMFASetup called, isLoginSetup:",
        props.isLoginSetup,
        "qrCodeUrl exists:",
        !!mfa.qrCodeUrl.value,
    );

    if (props.isLoginSetup) {
        if (!mfa.qrCodeUrl.value) {
            emit("error", "MFA setup not initialized properly");
            return;
        }
    } else {
        await mfa.startMFASetup();
        emitMfaMessages();
    }
};

const verifyMFA = async () => {
    logger.debug("🔐 verifyMFA called, isLoginSetup:", props.isLoginSetup);

    if (verificationCode.value.length !== 6) {
        emit("error", "Please enter a valid 6-digit code");
        return;
    }

    if (!mfa.mfaSecret.value) {
        emit(
            "error",
            "MFA secret is missing. Please restart the setup process.",
        );
        return;
    }

    mfa.clearMessages();
    emit("success", "");
    emit("error", "");

    try {
        if (props.isLoginSetup) {
            await enableMFAForLogin();
        } else {
            // First verify, then enable
            const isValid = await mfa.verifyMFAToken(verificationCode.value);
            if (isValid) {
                await enableMFAForAuthenticatedUser();
            } else {
                emitMfaMessages();
            }
        }
    } catch (err) {
        logger.error("🔐 MFA verification error:", err);
        emit(
            "error",
            err instanceof Error
                ? err.message
                : "Invalid verification code. Please try again.",
        );
    }
};

const enableMFAForLogin = async () => {
    const creds = mfaSetupStore.getCredentials;
    if (!creds) {
        throw new Error("MFA setup credentials not found");
    }

    const result = await mfa.enableMFAForLogin(
        creds.email,
        creds.password,
        verificationCode.value,
    );

    if (result.success) {
        // Handle successful login-flow MFA setup
        if (result.csrf_token && result.user) {
            authStore.user = result.user;
            authStore.mfaSetupRequired = false;
            authStore.mfaUserUuid = "";

            // Set auth provider (handled by auth store now)
            authStore.setAuthProvider("local");

            emit("mfa-enabled");
            emitMfaMessages();
        } else {
            emit("error", "MFA enabled but login response was incomplete");
        }
    } else {
        emitMfaMessages();
    }
};

const enableMFAForAuthenticatedUser = async () => {
    const result = await mfa.enableMFA(verificationCode.value);

    if (result.success) {
        verificationCode.value = "";
        emit("mfa-enabled");
        emitMfaMessages();
    } else {
        emitMfaMessages();
    }
};

const disableMFA = async () => {
    // Skip password prompt for limited sessions (already authenticated via magic link)
    let password = "";
    if (!isLimitedSession.value) {
        const userPassword = prompt(
            "Please enter your password to disable MFA:",
        );
        if (!userPassword) return;
        password = userPassword;
    }

    const success = await mfa.disableMFA(password);

    if (success) {
        resetMFASetup();
        emit("mfa-disabled");
    }
    emitMfaMessages();
};

const resetMFASetup = () => {
    mfa.resetMFASetup();
    verificationCode.value = "";
    showSecret.value = false;
    secretCopied.value = false;
};

const cancelMFASetup = () => {
    resetMFASetup();
};

const completeSetup = () => {
    mfaSetupStore.clearCredentials();
    emit("success", "setup-complete");
};

// Computed: total cells for QR grid (depends on matrix size or default)
// TOTP QR codes are Version 6 (41x41) for typical email lengths
const qrGridSize = computed(() => {
    return mfa.qrMatrix.value?.size || 41;
});
const qrTotalCells = computed(() => qrGridSize.value * qrGridSize.value);

// Computed: padding for QR grid to add quiet zone (standard is 4 modules)
// This creates a white border around the QR code for better scanning
const qrGridPadding = computed(() => {
    // 4 modules of quiet zone on each side
    // As percentage of total container: 4 / (size + 8) * 100
    const quietZone = 4;
    const totalWithQuiet = qrGridSize.value + (quietZone * 2);
    const paddingPercent = (quietZone / totalWithQuiet) * 100;
    return `${paddingPercent.toFixed(2)}%`;
});

// Animation tick for dynamic loading pattern - updates every 60ms
const animTick = ref(0);
let animInterval: ReturnType<typeof setInterval> | null = null;

// Track when data arrives for transition animation
const dataArrivedTick = ref<number | null>(null);
const transitionDuration = 30; // ticks for transition (~1.8s)

// Start/stop animation based on whether real data is available
watch(() => mfa.qrMatrix.value, (newVal) => {
    if (newVal && dataArrivedTick.value === null) {
        // Data just arrived - record the tick and keep animating for transition
        dataArrivedTick.value = animTick.value;
    }
}, { immediate: true });

// Start animation on mount
onMounted(() => {
    animInterval = setInterval(() => {
        animTick.value++;
        // Stop animation after transition completes
        if (dataArrivedTick.value !== null &&
            animTick.value > dataArrivedTick.value + transitionDuration) {
            clearInterval(animInterval!);
            animInterval = null;
        }
    }, 60);
});

// Cleanup on unmount
onUnmounted(() => {
    if (animInterval) {
        clearInterval(animInterval);
    }
});

// Hash function for deterministic but chaotic noise
const hash = (x: number, y: number, t: number): number => {
    const n = Math.sin(x * 127.1 + y * 311.7 + t * 53.3) * 43758.5453;
    return n - Math.floor(n);
};

// Determine if a cell should be dark in the loading skeleton (time-varying)
// Starts all white, radial wave brings in noise pattern from center
const isLoadingCellDark = (row: number, col: number, size: number, tick: number): boolean => {
    const center = (size - 1) / 2;
    const dr = row - center;
    const dc = col - center;
    const dist = Math.sqrt(dr * dr + dc * dc);
    const maxDist = Math.sqrt(2) * center;
    const normalizedDist = dist / maxDist;

    // Initial expansion wave - starts white, pattern radiates out
    // Wave reaches edge by tick 15 (~900ms)
    const initialWaveDuration = 15;
    if (tick < initialWaveDuration) {
        const initialWavePos = tick / initialWaveDuration;
        const edgeNoise = hash(row, col, 0) * 0.1;
        // Cell is white until initial wave reaches it
        if (normalizedDist + edgeNoise > initialWavePos) {
            return false;
        }
    }

    // Finder patterns (7x7 in corners) - show once wave reaches them
    const inTopLeftPattern = row <= 6 && col <= 6;
    const inTopRightPattern = row <= 6 && col >= size - 7;
    const inBottomLeftPattern = row >= size - 7 && col <= 6;

    if (inTopLeftPattern || inTopRightPattern || inBottomLeftPattern) {
        let localRow = row;
        let localCol = col;
        if (inTopRightPattern) localCol = col - (size - 7);
        if (inBottomLeftPattern) localRow = row - (size - 7);

        // Outer ring: dark
        if (localRow === 0 || localRow === 6 || localCol === 0 || localCol === 6) return true;
        // Inner white ring
        if (localRow === 1 || localRow === 5 || localCol === 1 || localCol === 5) return false;
        // Center 3x3: dark
        return true;
    }

    // White border around finder patterns
    const isFinderBorder =
        (row === 7 && col <= 7) || (col === 7 && row <= 7) ||
        (row === 7 && col >= size - 8) || (col === size - 8 && row <= 7) ||
        (row === size - 8 && col <= 7) || (col === 7 && row >= size - 8);
    if (isFinderBorder) return false;

    // Base noise for this cell - static per position
    const baseNoise = hash(row, col, 0);

    // Radial wave - continuously emanates from center
    // Wave position cycles from 0 to 1 every 20 ticks (~1.2s at 60ms interval)
    const waveCycle = 20;
    const wavePos = (tick % waveCycle) / waveCycle;

    // Calculate wave influence on this cell
    // Wave creates a sinusoidal modulation that travels outward
    const waveOffset = (normalizedDist - wavePos) * Math.PI * 2;
    const waveInfluence = Math.sin(waveOffset) * 0.5 + 0.5; // 0 to 1

    // Time-varying component - changes with each wave cycle
    const cycleNum = Math.floor(tick / waveCycle);
    const timeNoise = hash(row, col, cycleNum);

    // Combine: base noise + wave-modulated time noise
    // Wave passing through causes cells to potentially flip
    const combinedNoise = baseNoise * 0.4 + timeNoise * 0.3 + waveInfluence * 0.3;

    // Threshold with slight position variation for organic feel
    const threshold = 0.48 + hash(row * 7, col * 13, 0) * 0.08;

    return combinedNoise > threshold;
};

// Get cell style - radial delay for final state animation
const getQrCellStyle = (i: number) => {
    const size = qrGridSize.value;
    const row = Math.floor((i - 1) / size);
    const col = (i - 1) % size;

    // Radial distance from center
    const center = (size - 1) / 2;
    const dr = row - center;
    const dc = col - center;
    const dist = Math.sqrt(dr * dr + dc * dc);

    // Small noise for organic feel
    const n1 = Math.sin(row * 17.31 + col * 83.17) * 7654.321;
    const noise = Math.abs(n1 - Math.floor(n1));

    // Radial delay - cells lock in from center outward when data arrives
    const radialDelay = dist * 25 + noise * 100;

    return {
        "--del": `${Math.round(radialDelay)}ms`,
    };
};

// Get cell class - handles loading, transition, and final states
const getQrCellClass = (i: number) => {
    const size = qrGridSize.value;
    const row = Math.floor((i - 1) / size);
    const col = (i - 1) % size;

    // Calculate radial distance for this cell
    const center = (size - 1) / 2;
    const dr = row - center;
    const dc = col - center;
    const dist = Math.sqrt(dr * dr + dc * dc);
    const maxDist = Math.sqrt(2) * center;
    const normalizedDist = dist / maxDist;

    // Real matrix data available
    if (mfa.qrMatrix.value) {
        const matrix = mfa.qrMatrix.value;
        const idx = row * matrix.size + col;
        const finalIsDark = matrix.data[idx];

        // Check if still in transition
        if (dataArrivedTick.value !== null) {
            const ticksSinceArrival = animTick.value - dataArrivedTick.value;
            // Radial wave progress: 0 at start, 1 when complete
            const waveProgress = ticksSinceArrival / transitionDuration;

            // Add noise to the transition wave edge for organic feel
            const edgeNoise = hash(row, col, 0) * 0.15;
            const cellThreshold = normalizedDist + edgeNoise;

            // If wave hasn't reached this cell yet, show loading pattern
            if (cellThreshold > waveProgress) {
                const loadingIsDark = isLoadingCellDark(row, col, size, animTick.value);
                return loadingIsDark ? "aspect-square qr-cell-loading-dark" : "aspect-square qr-cell-loading-light";
            }
        }

        // Wave has passed or transition complete - show final data
        return finalIsDark ? "aspect-square qr-cell-final-dark" : "aspect-square qr-cell-final-light";
    }

    // No data yet - show loading skeleton
    const isDark = isLoadingCellDark(row, col, size, animTick.value);
    return isDark ? "aspect-square qr-cell-loading-dark" : "aspect-square qr-cell-loading-light";
};


// Format secret with spaces for better readability
const formatSecret = (secret: string) => {
    if (!secret) return "";
    return secret.replace(/(.{4})/g, "$1 ").trim();
};

// Copy secret to clipboard
const copySecret = async () => {
    if (!mfa.mfaSecret.value || secretCopied.value) return;

    try {
        await navigator.clipboard.writeText(mfa.mfaSecret.value);
        secretCopied.value = true;

        setTimeout(() => {
            secretCopied.value = false;
        }, 2000);
    } catch (err) {
        logger.error("Failed to copy secret:", err);
        emit("error", "Failed to copy to clipboard");
    }
};

// Download backup codes as text file
const downloadBackupCodes = () => {
    if (!mfa.backupCodes.value.length) return;

    try {
        const content = `Nosdesk Backup Codes

IMPORTANT: Save these backup codes in a secure location.
Each code can only be used once to access your account if you lose your authenticator device.

Backup Codes:
${mfa.backupCodes.value.map((code, index) => `${index + 1}. ${code}`).join("\n")}

Generated on: ${new Date().toISOString()}`;

        const blob = new Blob([content], { type: "text/plain" });
        const url = URL.createObjectURL(blob);
        const link = document.createElement("a");
        link.href = url;
        link.download = `nosdesk-backup-codes-${new Date().toISOString().split("T")[0]}.txt`;

        document.body.appendChild(link);
        link.click();
        document.body.removeChild(link);

        URL.revokeObjectURL(url);
        emit("success", "Backup codes downloaded successfully");
    } catch (err) {
        logger.error("Failed to download backup codes:", err);
        emit("error", "Failed to download backup codes");
    }
};

// Handle paste events for verification codes - auto-submit on 6-digit code
const handleVerificationPaste = (event: ClipboardEvent) => {
    event.preventDefault();
    const pastedText = event.clipboardData?.getData("text") || "";
    const cleanValue = pastedText.replace(/[^0-9]/g, "");

    if (cleanValue.length >= 6) {
        // Take first 6 characters for standard TOTP codes
        verificationCode.value = cleanValue.slice(0, 6);
        // Auto-submit on exactly 6-digit code
        if (cleanValue.length === 6) {
            // Small delay to let the UI update
            setTimeout(() => {
                verifyMFA();
            }, 100);
        }
    }
};

// Expose methods for parent component access
defineExpose({
    startMFASetup,
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
    content: "";
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

/* Loading state - cells shift dynamically */
.qr-cell-loading-dark {
    background-color: rgb(17, 24, 39);
    transition: background-color 120ms ease-out;
}

.qr-cell-loading-light {
    background-color: white;
    transition: background-color 120ms ease-out;
}

/* Final state - solid colors, no animation (transition handled in JS) */
.qr-cell-final-dark {
    background-color: rgb(17, 24, 39);
}

.qr-cell-final-light {
    background-color: white;
}
</style>

<template>
    <div
        class="bg-surface rounded-xl border border-default hover:border-strong transition-colors overflow-hidden"
    >
        <div class="px-4 py-3 bg-surface-alt border-b border-default">
            <h2 class="text-lg font-medium text-primary">
                {{
                    isInSuccessState
                        ? "Setup Complete!"
                        : "Two-Factor Authentication"
                }}
            </h2>
            <p class="text-sm text-tertiary mt-1">
                {{
                    isInSuccessState
                        ? "Your account is now protected with 2FA"
                        : "Add an extra layer of security to your account"
                }}
            </p>
        </div>

        <div class="p-6">
            <div class="flex flex-col gap-4">
                <!-- MFA Toggle / Status (hidden in login setup mode) -->
                <div
                    v-if="!props.isLoginSetup"
                    class="bg-surface-alt rounded-lg border border-subtle hover:border-strong transition-colors p-4"
                >
                    <ToggleSwitch
                        :modelValue="mfa.mfaEnabled.value"
                        :disabled="mfa.loading.value"
                        label="Enable Two-Factor Authentication"
                        :description="
                            mfa.mfaEnabled.value
                                ? 'Your account is protected with 2FA'
                                : 'Secure your account with an authenticator app'
                        "
                        @update:modelValue="toggleMFA"
                    />
                </div>

                <!-- Setup Steps (hidden in login setup mode) -->
                <div
                    v-if="mfa.showSetupSteps.value && !props.isLoginSetup"
                    class="bg-surface-alt rounded-lg border border-default/20 p-4"
                >
                    <h3 class="text-sm font-medium text-primary mb-4">
                        How to set up 2FA:
                    </h3>
                    <ol class="flex flex-col gap-3 text-sm text-secondary">
                        <li
                            v-for="(step, index) in setupSteps"
                            :key="index"
                            class="flex items-start gap-3"
                        >
                            <span
                                class="bg-accent text-white rounded-full w-5 h-5 flex items-center justify-center text-xs font-medium mt-0.5 flex-shrink-0"
                                >{{ index + 1 }}</span
                            >
                            <span>{{ step }}</span>
                        </li>
                    </ol>
                </div>

                <!-- Main MFA Setup Component - Hidden when verification is successful -->
                <div
                    v-if="shouldShowSetupInterface && !mfa.verifying.value"
                    class="grid grid-cols-1 lg:grid-cols-[auto_1fr] gap-8 items-start"
                >
                    <!-- QR Code Section -->
                    <div class="flex justify-center lg:justify-start">
                        <div class="bg-white p-4 rounded-lg shadow-lg">
                            <!-- QR Code container - single grid that handles both states -->
                            <div class="relative w-48 h-48 lg:w-44 lg:h-44">
                                <div
                                    class="absolute inset-0 bg-white rounded-lg overflow-hidden"
                                    :style="{ padding: qrGridPadding }"
                                >
                                    <div
                                        class="w-full h-full grid"
                                        :style="{ gridTemplateColumns: `repeat(${qrGridSize}, 1fr)` }"
                                    >
                                        <template v-for="i in qrTotalCells" :key="i">
                                            <div
                                                :class="getQrCellClass(i)"
                                                :style="getQrCellStyle(i)"
                                            ></div>
                                        </template>
                                    </div>
                                </div>
                            </div>
                        </div>
                    </div>

                    <!-- Verification Components -->
                    <div class="flex flex-col gap-6">
                        <!-- Manual Secret Entry Option -->
                        <div
                            class="bg-surface/50 rounded-lg border border-default/20 p-4"
                        >
                            <button
                                @click="showSecret = !showSecret"
                                class="flex items-center gap-2 text-sm text-tertiary hover:text-primary transition-colors"
                            >
                                <svg
                                    xmlns="http://www.w3.org/2000/svg"
                                    class="h-4 w-4 transition-transform"
                                    :class="{ 'rotate-90': showSecret }"
                                    fill="none"
                                    viewBox="0 0 24 24"
                                    stroke="currentColor"
                                >
                                    <path
                                        stroke-linecap="round"
                                        stroke-linejoin="round"
                                        stroke-width="2"
                                        d="M9 5l7 7-7 7"
                                    />
                                </svg>
                                Can't scan? Enter the code manually
                            </button>

                            <div
                                v-if="showSecret"
                                class="mt-4 flex flex-col gap-3"
                            >
                                <p class="text-sm text-tertiary">
                                    Enter this secret key in your authenticator
                                    app:
                                </p>
                                <div
                                    class="bg-surface-alt rounded-lg p-3 border border-subtle"
                                >
                                    <div
                                        class="flex items-center justify-between gap-3"
                                    >
                                        <code
                                            class="text-sm font-mono text-status-success select-all flex-1 break-all"
                                            >{{
                                                formatSecret(
                                                    mfa.mfaSecret.value,
                                                )
                                            }}</code
                                        >
                                        <button
                                            @click="copySecret"
                                            :disabled="secretCopied"
                                            class="px-3 py-1 text-xs rounded transition-all duration-200 flex-shrink-0"
                                            :class="
                                                secretCopied
                                                    ? 'bg-status-success text-white cursor-default'
                                                    : 'bg-surface-hover text-primary hover:bg-surface cursor-pointer'
                                            "
                                            :title="
                                                secretCopied
                                                    ? 'Copied to clipboard!'
                                                    : 'Copy to clipboard'
                                            "
                                        >
                                            {{
                                                secretCopied
                                                    ? "Copied!"
                                                    : "Copy"
                                            }}
                                        </button>
                                    </div>
                                </div>
                            </div>
                        </div>

                        <!-- Verification Input Section -->
                        <div class="flex flex-col gap-4">
                            <div class="text-center lg:text-left">
                                <h4
                                    class="text-sm font-medium text-primary mb-2"
                                >
                                    Enter Verification Code
                                </h4>
                                <p class="text-sm text-tertiary">
                                    Enter the 6-digit code from your
                                    authenticator app:
                                </p>
                            </div>

                            <div
                                class="flex flex-col lg:flex-row lg:items-center gap-2 lg:gap-3"
                            >
                                <!-- OTP input component -->
                                <div
                                    class="flex justify-center lg:justify-start"
                                >
                                    <OtpInput
                                        v-model="verificationCode"
                                        @complete="handleOtpComplete"
                                        aria-label="MFA verification code"
                                    />
                                </div>

                                <button
                                    @click="verifyMFA"
                                    :disabled="
                                        verificationCode.length !== 6 ||
                                        mfa.verifying.value
                                    "
                                    class="w-full lg:w-auto px-6 h-12 lg:h-14 bg-accent text-white rounded-lg hover:opacity-90 focus:outline-none focus:ring-2 focus:ring-accent disabled:opacity-50 disabled:cursor-not-allowed disabled:bg-surface-hover flex items-center justify-center transition-colors active:scale-[0.98]"
                                >
                                    <span
                                        v-if="mfa.verifying.value"
                                        class="animate-spin h-4 w-4 mr-2"
                                    >
                                        <svg
                                            xmlns="http://www.w3.org/2000/svg"
                                            fill="none"
                                            viewBox="0 0 24 24"
                                        >
                                            <circle
                                                class="opacity-25"
                                                cx="12"
                                                cy="12"
                                                r="10"
                                                stroke="currentColor"
                                                stroke-width="4"
                                            ></circle>
                                            <path
                                                class="opacity-75"
                                                fill="currentColor"
                                                d="M4 12a8 8 0 0 1 8-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 0 1 4 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
                                            ></path>
                                        </svg>
                                    </span>
                                    {{
                                        mfa.verifying.value
                                            ? "Verifying..."
                                            : "Verify"
                                    }}
                                </button>
                            </div>
                        </div>
                    </div>
                </div>

                <!-- Verification Loading State - Replaces the setup interface when verifying -->
                <div
                    v-if="shouldShowSetupInterface && mfa.verifying.value"
                    class="grid grid-cols-1 lg:grid-cols-[auto_1fr] gap-8 items-start"
                >
                    <!-- QR Code Section (keep visible during verification) -->
                    <div class="flex justify-center lg:justify-start">
                        <div class="bg-white p-4 rounded-lg shadow-lg">
                            <img
                                v-if="mfa.qrCodeUrl.value"
                                :src="mfa.qrCodeUrl.value"
                                alt="MFA QR Code"
                                class="w-48 h-48 lg:w-44 lg:h-44"
                            />
                        </div>
                    </div>

                    <!-- Loading State in place of verification components -->
                    <div class="flex flex-col gap-6 justify-center">
                        <div class="flex items-center justify-center py-8">
                            <div class="flex flex-col items-center gap-4">
                                <div class="bg-accent rounded-full p-4">
                                    <svg
                                        class="w-8 h-8 text-white animate-spin"
                                        fill="none"
                                        viewBox="0 0 24 24"
                                    >
                                        <circle
                                            class="opacity-25"
                                            cx="12"
                                            cy="12"
                                            r="10"
                                            stroke="currentColor"
                                            stroke-width="4"
                                        ></circle>
                                        <path
                                            class="opacity-75"
                                            fill="currentColor"
                                            d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
                                        ></path>
                                    </svg>
                                </div>
                                <div class="text-center">
                                    <h3
                                        class="text-lg font-medium text-primary mb-2"
                                    >
                                        Verifying Code
                                    </h3>
                                    <p class="text-sm text-tertiary">
                                        Please wait while we verify your
                                        authenticator code...
                                    </p>
                                </div>
                            </div>
                        </div>
                    </div>
                </div>

                <!-- Backup Codes Display: only show after success or enabled -->
                <div
                    v-if="
                        mfa.backupCodes.value.length > 0 &&
                        (mfa.mfaStep.value === 'success' ||
                            mfa.mfaEnabled.value)
                    "
                    class="flex flex-col gap-2 bg-surface border border-default rounded-xl p-6"
                >
                    <div
                        class="flex flex-col md:flex-row md:items-center md:justify-between gap-4 mb-4"
                    >
                        <div class="flex-1">
                            <h2 class="text-lg font-semibold text-primary mb-2">
                                Backup Codes
                            </h2>
                            <p class="text-secondary text-sm">
                                Save these backup codes in a secure location.
                                You can use them to access your account if you
                                lose your authenticator device.
                            </p>
                        </div>
                        <div class="flex-shrink-0">
                            <button
                                @click="downloadBackupCodes"
                                class="px-4 py-2 bg-surface hover:bg-surface-hover border border-status-warning text-primary rounded-lg transition-colors flex items-center"
                                title="Download backup codes as text file"
                            >
                                <svg
                                    class="w-4 h-4 mr-2 text-status-warning"
                                    fill="none"
                                    stroke="currentColor"
                                    viewBox="0 0 24 24"
                                >
                                    <path
                                        stroke-linecap="round"
                                        stroke-linejoin="round"
                                        stroke-width="2"
                                        d="M12 10v6m0 0l-3-3m3 3l3-3m2 8H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z"
                                    />
                                </svg>
                                Download
                            </button>
                        </div>
                    </div>
                    <div
                        class="bg-surface-alt rounded-lg p-4 font-mono text-sm text-primary"
                    >
                        <div class="grid grid-cols-2 sm:grid-cols-4 gap-3">
                            <div
                                v-for="code in mfa.backupCodes.value"
                                :key="code"
                                class="text-center p-2 bg-surface rounded break-all"
                            >
                                {{ code }}
                            </div>
                        </div>
                    </div>
                </div>

                <!-- Success State (for login setup) -->
                <div
                    v-if="mfa.showSuccessState.value"
                    class="bg-status-success-muted border border-status-success/20 rounded-lg p-6"
                >
                    <div class="flex flex-col gap-4">
                        <div class="flex items-center gap-3">
                            <div class="bg-status-success rounded-full p-2">
                                <svg
                                    xmlns="http://www.w3.org/2000/svg"
                                    class="h-6 w-6 text-white"
                                    fill="none"
                                    viewBox="0 0 24 24"
                                    stroke="currentColor"
                                >
                                    <path
                                        stroke-linecap="round"
                                        stroke-linejoin="round"
                                        stroke-width="2"
                                        d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z"
                                    />
                                </svg>
                            </div>
                            <div>
                                <h3 class="text-lg font-medium text-status-success">
                                    Two-Factor Authentication Enabled!
                                </h3>
                                <p class="text-sm text-secondary">
                                    Your account is now protected with 2FA.
                                    You'll need to enter a code from your
                                    authenticator app when signing in.
                                </p>
                            </div>
                        </div>

                        <!-- Action Button -->
                        <div class="flex justify-center pt-2">
                            <button
                                @click="completeSetup"
                                class="w-full sm:w-auto px-8 py-3 bg-accent text-white rounded-lg hover:opacity-90 focus:outline-none focus:ring-2 focus:ring-accent transition-colors font-medium min-h-[52px] active:scale-[0.98]"
                            >
                                Start Using Nosdesk!
                            </button>
                        </div>
                    </div>
                </div>

            </div>
        </div>
    </div>
</template>
