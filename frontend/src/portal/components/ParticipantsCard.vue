<script setup lang="ts">
/**
 * People on a request. The requester can add colleagues by email (they see the
 * request in their own portal and get the team's replies) and remove them;
 * anyone who was added can leave.
 */
import { ref } from 'vue'
import { useQuery } from '@pinia/colada'
import { useFluent } from 'fluent-vue'
import { useRouter } from 'vue-router'

import Button from '@/components/common/Button.vue'
import FormInput from '@/components/common/FormInput.vue'

import PortalAvatar from './PortalAvatar.vue'

import { addParticipant, getMe, removeParticipant, type PortalParticipant } from '../service'

const props = defineProps<{
  ticketId: number
  participants: PortalParticipant[]
  isRequester: boolean
}>()
const emit = defineEmits<{ changed: [] }>()
const { $t: t } = useFluent()
const router = useRouter()
const me = useQuery({ key: ['portal', 'me'], query: getMe })

const email = ref('')
const adding = ref(false)
const busy = ref(false)
const failed = ref<string | null>(null)

async function add(): Promise<void> {
  const address = email.value.trim()
  if (!address) return
  busy.value = true
  failed.value = null
  try {
    await addParticipant(props.ticketId, address)
    email.value = ''
    adding.value = false
    emit('changed')
  } catch {
    failed.value = t('portal-people-add-failed')
  } finally {
    busy.value = false
  }
}

async function remove(person: PortalParticipant): Promise<void> {
  busy.value = true
  failed.value = null
  try {
    await removeParticipant(props.ticketId, person.uuid)
    // Leaving a request you were added to: it's no longer yours to see.
    if (person.uuid === me.data.value?.uuid) {
      void router.push('/tickets')
      return
    }
    emit('changed')
  } catch {
    failed.value = t('portal-people-remove-failed')
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <section class="flex flex-col gap-3 bg-surface border border-default rounded-xl p-4">
    <div class="flex items-center gap-2">
      <h2 class="text-sm font-semibold text-primary flex-1">{{ t('portal-people-title') }}</h2>
      <Button
        v-if="isRequester && !adding"
        variant="ghost"
        size="xs"
        icon="userPlus"
        @click="adding = true"
      >
        {{ t('portal-people-add') }}
      </Button>
    </div>
    <ul class="flex flex-col gap-2.5">
      <li v-for="person in participants" :key="person.uuid" class="flex items-center gap-3 text-sm">
        <PortalAvatar :name="person.name" size="sm" />
        <span class="flex flex-col min-w-0 flex-1">
          <span class="text-primary truncate">{{ person.name }}</span>
          <span class="text-xs text-secondary truncate">
            {{ person.is_requester ? t('portal-people-requester') : person.email }}
          </span>
        </span>
        <Button
          v-if="!person.is_requester && (isRequester || person.uuid === me.data.value?.uuid)"
          variant="ghost"
          size="xs"
          :disabled="busy"
          @click="remove(person)"
        >
          {{ person.uuid === me.data.value?.uuid ? t('portal-people-leave') : t('portal-people-remove') }}
        </Button>
      </li>
    </ul>
    <form v-if="isRequester && adding" class="flex flex-col gap-2" @submit.prevent="add">
      <FormInput
        v-model="email"
        type="email"
        :label="t('portal-people-add-label')"
        :placeholder="t('portal-people-add-placeholder')"
        :disabled="busy"
      />
      <p class="text-xs text-secondary">{{ t('portal-people-hint') }}</p>
      <div class="flex justify-end gap-2">
        <Button type="button" variant="ghost" size="sm" :disabled="busy" @click="adding = false">
          {{ t('portal-people-cancel') }}
        </Button>
        <Button type="submit" size="sm" :loading="busy" :disabled="!email.trim()">
          {{ t('portal-people-add') }}
        </Button>
      </div>
    </form>
    <p v-if="failed" role="alert" class="text-sm text-status-error">{{ failed }}</p>
  </section>
</template>
