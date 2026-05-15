<script setup lang="ts">
import { useFluent } from 'fluent-vue'
import type { FluentVariable } from '@fluent/bundle'
import SectionCard from '@/components/common/SectionCard.vue'
import { useColorFilter } from '@/composables/useColorFilter'
import type { DeviceGroup } from '@/types/device'

defineProps<{
  groups: DeviceGroup[] | undefined
}>()

const { colorFilterStyle } = useColorFilter()

const fluent = useFluent()
const t = (k: string, args?: Record<string, FluentVariable>) => fluent.$t(k, args)
</script>

<template>
  <SectionCard v-if="groups && groups.length > 0" content-padding="p-4">
    <template #title>{{ t('ui-device-groups-title') }}</template>

    <div class="flex flex-wrap gap-2">
      <router-link
        v-for="group in groups"
        :key="group.uuid"
        :to="`/groups/${group.uuid}`"
        class="inline-flex items-center gap-2 px-3 py-1.5 bg-surface-alt rounded-lg border border-default hover:border-strong transition-colors"
      >
        <div
          class="w-3 h-3 rounded-full flex-shrink-0"
          :style="{ backgroundColor: group.color || '#6b7280', ...colorFilterStyle }"
        ></div>
        <span class="text-sm text-primary">{{ group.name }}</span>
      </router-link>
    </div>
  </SectionCard>
</template>
