<script setup lang="ts">
import { computed } from 'vue'
import Checkbox from './Checkbox.vue'

const props = defineProps<{
  items: any[]
  selectedIds: (string | number)[]
  selectable?: boolean
  columns: {
    key: string
    label: string
    class?: string
    hidden?: boolean
  }[]
}>()

const emit = defineEmits<{
  'toggle-selection': [event: Event, item: any]
  'toggle-all': [event: Event]
  'row-click': [item: any]
}>()

const allSelected = computed(() => {
  return props.items.length > 0 && props.selectedIds.length === props.items.length
})

const isSelected = (item: any) => {
  return props.selectedIds.includes(item.id)
}
</script>

<template>
  <div class="overflow-x-auto">
    <table class="min-w-full divide-y divide-border-default">
      <thead>
        <tr>
          <th v-if="selectable !== false" scope="col" class="p-4 w-10">
            <Checkbox
              :model-value="allSelected"
              @change="(e) => emit('toggle-all', e)"
            />
          </th>
          <th
            v-for="column in columns"
            :key="column.key"
            scope="col"
            :class="[
              'text-left p-4 font-medium text-xs text-tertiary uppercase tracking-wider',
              column.hidden ? 'hidden md:table-cell' : '',
              column.class || ''
            ]"
          >
            {{ column.label }}
          </th>
        </tr>
      </thead>
      <tbody class="divide-y divide-border-default">
        <slot
          name="row"
          v-for="item in items"
          :key="item.id"
          :item="item"
          :is-selected="isSelected(item)"
          :toggle-selection="(e: Event) => emit('toggle-selection', e, item)"
        >
          <tr
            :class="['hover:bg-surface-hover transition-colors cursor-pointer', isSelected(item) ? 'bg-surface-hover' : '']"
            @click="emit('row-click', item)"
          >
            <td v-if="selectable !== false" class="px-4 py-1" @click.stop>
              <Checkbox
                :model-value="isSelected(item)"
                @change="(e) => emit('toggle-selection', e, item)"
              />
            </td>
            <slot
              name="cell"
              v-for="column in columns"
              :key="`${item.id}-${column.key}`"
              :item="item"
              :column="column"
            >
              <td
                :class="[
                  'px-6 py-4 whitespace-nowrap text-sm',
                  column.hidden ? 'hidden md:table-cell' : '',
                  column.class || ''
                ]"
              >
                {{ item[column.key] }}
              </td>
            </slot>
          </tr>
        </slot>

        <tr v-if="items.length === 0">
          <td :colspan="selectable !== false ? columns.length + 1 : columns.length" class="px-6 py-4 text-center text-secondary">
            <slot name="empty">
              No items found.
            </slot>
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template> 