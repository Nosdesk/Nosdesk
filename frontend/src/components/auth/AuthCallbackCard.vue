<script setup lang="ts">
import AssetImg from '@/components/common/AssetImg.vue'
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import LogoIcon from '@/components/icons/LogoIcon.vue'
import { useBrandingStore } from '@/stores/branding'
import { useThemeStore } from '@/stores/theme'

const fluent = useFluent()
const t = (key: string) => fluent.$t(key)

const brandingStore = useBrandingStore()
const themeStore = useThemeStore()

const logoUrl = computed(() => brandingStore.getLogoUrl(themeStore.isDarkMode))
</script>

<template>
  <div class="min-h-screen flex items-center justify-center bg-app p-4">
    <div class="bg-surface p-8 rounded-xl shadow-lg max-w-md w-full border border-default flex flex-col gap-6">
      <div class="flex justify-center">
        <AssetImg
          v-if="logoUrl"
          :src="logoUrl"
          :alt="brandingStore.appName"
          class="h-10 max-w-[200px] object-contain"
        />
        <LogoIcon v-else class="h-10 text-accent" />
      </div>

      <div class="flex flex-col items-center justify-center gap-4">
        <div class="relative">
          <div class="w-12 h-12 rounded-full border-2 border-surface-hover"></div>
          <div class="absolute inset-0 w-12 h-12 rounded-full border-2 border-accent border-t-transparent animate-spin"></div>
        </div>
        <h2 class="text-lg font-medium text-primary">{{ t('auth-callback-loading-default') }}</h2>
        <p class="text-sm text-tertiary text-center">{{ t('auth-callback-loading-subtitle') }}</p>
      </div>
    </div>
  </div>
</template>
