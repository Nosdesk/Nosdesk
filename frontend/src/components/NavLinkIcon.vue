<!--
NavLinkIcon renders a nav link's icon. Core links carry a registry icon name
(`icon`); plugin nav items carry an icon URL (`iconUrl`, rendered as an <img>).
Keeps the img-vs-icon branch in one place across the sidebar's render variants.
-->
<script setup lang="ts">
import AssetImg from '@/components/common/AssetImg.vue'
import Icon from '@/components/common/Icon.vue'
import type { IconName } from '@/components/common/icons'
defineProps<{
  /** Registry icon name for a core nav link. */
  icon?: IconName;
  /** Icon URL for a plugin nav item; when set, wins over `icon`. */
  iconUrl?: string;
}>();
</script>

<template>
  <AssetImg v-if="iconUrl" :src="iconUrl" class="w-4 h-4 rounded-sm object-cover" alt="" />
  <Icon v-else-if="icon" :name="icon" />
</template>
