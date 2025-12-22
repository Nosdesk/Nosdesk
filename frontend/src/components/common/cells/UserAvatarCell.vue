<script setup lang="ts">
import UserAvatar from '@/components/UserAvatar.vue'

interface Props {
  userId?: string
  userName?: string
  avatar?: string | null
  size?: 'xs' | 'sm' | 'md' | 'lg'
  showName?: boolean
  emptyText?: string
}

const props = withDefaults(defineProps<Props>(), {
  size: 'sm',
  showName: false,
  emptyText: 'Unassigned'
})
</script>

<template>
  <div v-if="userId" class="flex items-center gap-2">
    <UserAvatar
      :name="userId"
      :user-name="userName"
      :avatar="avatar"
      :size="size"
      :clickable="false"
      :show-name="showName"
    />
  </div>
  <span v-else class="text-xs text-tertiary">
    {{ emptyText }}
  </span>
</template> 