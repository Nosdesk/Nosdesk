<script setup lang="ts">
import UserAvatar from '@/components/UserAvatar.vue'
import { useFluent } from 'fluent-vue'

interface Props {
  userId?: string
  userName?: string
  avatar?: string | null
  size?: 'xs' | 'sm' | 'md' | 'lg'
  showName?: boolean
  emptyText?: string
}

// `withDefaults` evaluates the default once at module load, which
// would freeze the empty-text translation in whatever locale was
// active at bootstrap. Resolving inside the component (via a
// computed) lets the fallback re-render on locale switches and
// lets callers still override with their own literal.
const props = withDefaults(defineProps<Props>(), {
  size: 'sm',
  showName: false,
})

const fluent = useFluent()
const displayEmpty = () => props.emptyText ?? fluent.$t('filter-assignee-unassigned')
</script>

<template>
  <div v-if="userId" class="flex items-center gap-2">
    <UserAvatar
      :uuid="userId"
      :fallbackName="userName"
      :fallbackAvatar="avatar"
      :size="size"
      :clickable="false"
      :show-name="showName"
    />
  </div>
  <span v-else class="text-xs text-tertiary">
    {{ displayEmpty() }}
  </span>
</template> 