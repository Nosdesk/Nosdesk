<script setup lang="ts">
import { useRoute, useRouter } from 'vue-router';
import GroupConfigurationPanel from '@/components/admin/GroupConfigurationPanel.vue';

const route = useRoute();
const router = useRouter();
const uuid = route.params.uuid as string;

const onClose = () => router.push('/admin/groups');
const onDeleted = () => router.push('/admin/groups');
</script>

<template>
  <GroupConfigurationPanel :groupUuid="uuid" @close="onClose" @deleted="onDeleted" />
</template>
