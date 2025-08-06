<template>
  <div class="bg-slate-800 rounded-xl border border-slate-700/50">
    <div class="px-4 py-3 bg-slate-700/30 border-b border-slate-700/50">
      <h2 class="text-lg font-medium text-white">Two-Factor Authentication</h2>
      <p class="text-sm text-slate-400 mt-1">Add an extra layer of security to your account</p>
    </div>
    
    <div class="p-6">
      <div class="flex flex-col gap-6">
        <!-- Header Section Skeleton -->
        <div class="space-y-2">
          <div class="h-4 bg-slate-600 rounded w-1/3 skeleton-item"></div>
          <div class="h-4 bg-slate-600 rounded w-2/3 skeleton-item" style="animation-delay: 0.2s"></div>
        </div>

        <!-- QR Code and Verification Loading -->
        <div class="grid grid-cols-1 lg:grid-cols-[auto_1fr] gap-8 items-start">
          <!-- QR Code Loading Skeleton -->
          <div class="flex justify-center lg:justify-start">
            <div class="bg-white p-4 rounded-lg shadow-lg">
              <div class="w-48 h-48 lg:w-44 lg:h-44 bg-gradient-to-br from-gray-50 to-gray-100 rounded-lg flex items-center justify-center relative overflow-hidden">
                <!-- Shimmer Effect -->
                <div class="absolute inset-0 skeleton-shimmer"></div>
                
                <!-- Simulated QR Code Pattern -->
                <div class="absolute inset-2 grid grid-cols-10 gap-px opacity-20">
                  <div v-for="i in 100" :key="i" 
                       class="bg-gray-400 rounded-[1px] skeleton-qr-cell" 
                       :class="{ 'opacity-0': Math.random() > 0.4 }"
                       :style="{ animationDelay: `${i * 15}ms` }">
                  </div>
                </div>
                
                <!-- Three corner markers simulation -->
                <div class="absolute top-2 left-2 w-6 h-6 border-2 border-gray-400 opacity-30 skeleton-pulse">
                  <div class="w-2 h-2 bg-gray-400 m-auto mt-1"></div>
                </div>
                <div class="absolute top-2 right-2 w-6 h-6 border-2 border-gray-400 opacity-30 skeleton-pulse" style="animation-delay: 0.5s">
                  <div class="w-2 h-2 bg-gray-400 m-auto mt-1"></div>
                </div>
                <div class="absolute bottom-2 left-2 w-6 h-6 border-2 border-gray-400 opacity-30 skeleton-pulse" style="animation-delay: 1s">
                  <div class="w-2 h-2 bg-gray-400 m-auto mt-1"></div>
                </div>
                
                <!-- Loading Text -->
                <div class="absolute bottom-4 left-1/2 transform -translate-x-1/2">
                  <div class="flex items-center gap-2 text-gray-500 text-xs">
                    <svg class="w-3 h-3 animate-spin" fill="none" viewBox="0 0 24 24">
                      <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                      <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                    </svg>
                    <span>Setting up...</span>
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
                <div class="w-4 h-4 bg-slate-600 rounded skeleton-item"></div>
                <div class="h-4 bg-slate-600 rounded flex-1 skeleton-item" style="animation-delay: 0.2s"></div>
              </div>
              
              <!-- Expandable content skeleton (collapsed state) -->
              <div class="mt-3 space-y-2 opacity-50">
                <div class="h-3 bg-slate-700/50 rounded w-5/6 skeleton-item" style="animation-delay: 0.4s"></div>
                <div class="bg-slate-700/30 rounded-lg p-3 border border-slate-600/10">
                  <div class="flex items-center justify-between gap-3">
                    <div class="h-3 bg-slate-700/70 rounded flex-1 font-mono skeleton-item" style="animation-delay: 0.6s"></div>
                    <div class="w-12 h-6 bg-slate-700/70 rounded skeleton-item" style="animation-delay: 0.8s"></div>
                  </div>
                </div>
              </div>
            </div>
            
            <!-- Verification Input Skeleton -->
            <div class="flex flex-col gap-4">
              <div class="space-y-2">
                <div class="h-4 bg-slate-600 rounded w-3/4 skeleton-item" style="animation-delay: 1s"></div>
                <div class="h-4 bg-slate-600 rounded w-full skeleton-item" style="animation-delay: 1.2s"></div>
              </div>
              
              <div class="flex gap-3">
                <div class="bg-slate-700/50 rounded-lg border border-slate-600/30 flex-1 p-3">
                  <!-- Simulated 6-digit input placeholder -->
                  <div class="flex justify-center gap-2">
                    <div v-for="i in 6" :key="i" 
                         class="w-6 h-6 bg-slate-600 rounded skeleton-item" 
                         :style="{ animationDelay: `${1.4 + i * 0.1}s` }">
                    </div>
                  </div>
                </div>
                <div class="px-6 py-3 bg-blue-600/50 rounded-lg skeleton-item" style="animation-delay: 2s">
                  <div class="w-12 h-6 bg-blue-500/50 rounded"></div>
                </div>
              </div>
              
              <div class="h-3 bg-slate-600 rounded w-24 skeleton-item" style="animation-delay: 2.2s"></div>
            </div>
            
            <!-- Loading Message -->
            <div class="text-center py-4">
              <div class="flex items-center justify-center gap-2 text-slate-400">
                <svg class="w-4 h-4 animate-spin" fill="none" viewBox="0 0 24 24">
                  <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                  <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 818-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                </svg>
                <span class="text-sm">This will only take a moment...</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* Enhanced skeleton loading animations following Vue 3 best practices */
@keyframes pulse-bg {
  0% {
    background-color: #475569;
  }
  50% {
    background-color: #64748b;
  }
  100% {
    background-color: #475569;
  }
}

@keyframes shimmer {
  0% {
    transform: translateX(-100%);
  }
  100% {
    transform: translateX(100%);
  }
}

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

.skeleton-item {
  animation: pulse-bg 1.5s infinite;
}

.skeleton-pulse {
  animation: pulse-bg 1.5s infinite;
}

.skeleton-qr-cell {
  animation: qrPattern 2s infinite;
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
</style> 