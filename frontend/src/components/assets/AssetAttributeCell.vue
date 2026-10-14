<script setup lang="ts">
/**
 * One kind-attribute value in the inventory list. Renders by the
 * attribute's schema type: a user as their avatar and name, an asset
 * reference as a link with the asset's name, dates in the active locale,
 * booleans as yes / no, arrays joined. Anything else is plain text.
 */
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import { RouterLink } from 'vue-router'
import { formatDate, formatDateTime } from '@nosdesk/core/utils/dateUtils'
import { useReference } from '@nosdesk/core/sync/composables'
import { UserAvatarCell } from '@/components/common/cells'
import type { AttributeValueType } from '@/utils/assetAttributeColumns'

const props = defineProps<{
  value: unknown
  valueType: AttributeValueType
}>()

const fluent = useFluent()

const isEmpty = computed(
  () =>
    props.value == null ||
    props.value === '' ||
    (Array.isArray(props.value) && props.value.length === 0),
)

/** Asset references are stringified ids (`asset-ref`). */
const assetId = computed<number | null>(() => {
  if (props.valueType !== 'asset' || isEmpty.value) return null
  const id = Number(props.value)
  return Number.isInteger(id) && id > 0 ? id : null
})
const referenced = useReference<{ name?: string }>('asset', () => assetId.value)

const text = computed<string>(() => {
  const v = props.value
  if (isEmpty.value) return ''
  switch (props.valueType) {
    case 'boolean':
      return v === true
        ? fluent.$t('assets-list-attribute-yes')
        : fluent.$t('assets-list-attribute-no')
    // A plain date is a calendar day; format it in UTC so it can't shift.
    case 'date':
      return formatDate(String(v), 'date', 'UTC') || String(v)
    case 'datetime':
      return formatDateTime(String(v)) || String(v)
    case 'list':
      return Array.isArray(v) ? v.join(', ') : String(v)
    default:
      return typeof v === 'object' ? JSON.stringify(v) : String(v)
  }
})
</script>

<template>
  <span v-if="isEmpty" class="text-xs text-tertiary">-</span>
  <UserAvatarCell
    v-else-if="valueType === 'user' && typeof value === 'string'"
    :user-id="value"
    size="xs"
    :show-name="true"
  />
  <RouterLink
    v-else-if="assetId != null"
    :to="`/assets/${assetId}`"
    class="text-xs text-accent hover:underline truncate"
    @click.stop
  >
    {{ referenced?.name || `#${assetId}` }}
  </RouterLink>
  <span
    v-else
    class="text-xs text-secondary truncate"
    :class="valueType === 'number' ? 'tabular-nums' : ''"
    :title="text"
  >{{ text }}</span>
</template>
