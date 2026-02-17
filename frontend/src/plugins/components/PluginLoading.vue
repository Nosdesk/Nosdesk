<script setup lang="ts">
/**
 * Plugin Loading Component
 *
 * Displayed while a plugin component is being loaded.
 * Uses a subtle skeleton that matches sidebar card layout to prevent flash.
 */
</script>

<template>
  <div class="plugin-loading">
    <div class="animate-pulse flex flex-col gap-2">
      <div class="h-4 bg-surface-alt rounded w-1/3"></div>
      <div class="bg-surface rounded-xl border border-default overflow-hidden">
        <div class="px-4 py-3 bg-surface-alt border-b border-default">
          <div class="h-5 bg-surface rounded w-2/3"></div>
        </div>
        <div class="p-4">
          <div class="grid grid-cols-2 gap-3">
            <div class="h-4 bg-surface-alt rounded"></div>
            <div class="h-4 bg-surface-alt rounded"></div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
