<script setup lang="ts">
/**
 * The request types a workspace offers, as a radio list with each type's
 * description. Optional: "Something else" leaves the type unset. Shared by the
 * portal's new-request page and the guest submit form.
 */
import { RadioGroupItem, RadioGroupRoot } from 'reka-ui'
import { useFluent } from 'fluent-vue'

import type { RequestTypeOption } from './requestTypes'

defineProps<{ types: RequestTypeOption[]; disabled?: boolean }>()
const model = defineModel<number | null>({ required: true })
const { $t: t } = useFluent()

const OTHER = 'other'
</script>

<template>
  <fieldset class="flex flex-col gap-2" :disabled="disabled">
    <legend class="text-xs font-medium text-tertiary uppercase tracking-wide mb-2">{{ t('requester-type-label') }}</legend>
    <RadioGroupRoot
      :model-value="model === null ? OTHER : String(model)"
      class="flex flex-col gap-2"
      @update:model-value="model = $event === OTHER ? null : Number($event)"
    >
      <RadioGroupItem
        v-for="type in types"
        :key="type.id"
        :value="String(type.id)"
        class="flex items-start gap-3 text-left rounded-lg border border-default px-3 py-2.5 hover:bg-surface-hover aria-checked:border-accent aria-checked:bg-accent/5 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
      >
        <span
          class="mt-1.5 w-2.5 h-2.5 rounded-full shrink-0 bg-tertiary"
          :style="type.color ? { backgroundColor: type.color } : undefined"
          aria-hidden="true"
        />
        <span class="flex flex-col gap-0.5 min-w-0">
          <span class="text-sm font-medium text-primary">{{ type.name }}</span>
          <span v-if="type.description" class="text-xs text-secondary">{{ type.description }}</span>
        </span>
      </RadioGroupItem>
      <RadioGroupItem
        :value="OTHER"
        class="flex items-start gap-3 text-left rounded-lg border border-default px-3 py-2.5 hover:bg-surface-hover aria-checked:border-accent aria-checked:bg-accent/5 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
      >
        <span class="mt-1.5 w-2.5 h-2.5 rounded-full shrink-0 border border-default" aria-hidden="true" />
        <span class="text-sm font-medium text-primary">{{ t('requester-type-other') }}</span>
      </RadioGroupItem>
    </RadioGroupRoot>
  </fieldset>
</template>
