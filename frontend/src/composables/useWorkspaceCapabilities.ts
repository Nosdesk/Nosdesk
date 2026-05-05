/**
 * Reactive workspace-level capability flags. Populated from the
 * bootstrap meta header during sync lifecycle and read by any
 * UI component that needs to know "is this feature in play for
 * this workspace?"
 *
 * Module-scoped state (one workspace per session in this app),
 * exposed via a composable so consumers compose with
 * computed / watchers without reaching into the singleton
 * directly. Defaults to all-disabled until bootstrap arrives —
 * the right safe default (the UI starts quiet, then lights up
 * features as the meta header confirms them).
 *
 * The product principle this implements: "doesn't get in the
 * way if you don't need it." A small business with no SLA
 * policies should never see SLA chrome — no filter chip, no
 * default column, no summary segment. They opt in by creating
 * a policy in the admin UI; bootstrap re-runs and the chrome
 * lights up.
 */
import { computed, readonly, ref, type ComputedRef, type Ref } from 'vue'

interface CapabilityState {
  slaEnabled: boolean
}

const DEFAULT_STATE: CapabilityState = {
  slaEnabled: false,
}

const state: Ref<CapabilityState> = ref({ ...DEFAULT_STATE })

/** Called by sync/lifecycle.ts when the bootstrap header lands.
 * Pass the meta object verbatim — undefined fields fall back to
 * the safe default (feature off). */
export function applyWorkspaceCapabilities(meta: {
  sla_enabled?: boolean
}): void {
  state.value = {
    slaEnabled: meta.sla_enabled ?? false,
  }
}

/** Reset to defaults — used when the user logs out or switches
 * workspaces. The next bootstrap will re-populate. */
export function resetWorkspaceCapabilities(): void {
  state.value = { ...DEFAULT_STATE }
}

export interface UseWorkspaceCapabilities {
  /** True when the workspace has at least one SLA policy
   * configured. False otherwise. */
  slaEnabled: ComputedRef<boolean>
  /** Read-only access to the underlying state for cases that
   * need to watch / react to multi-flag changes at once. */
  capabilities: Readonly<Ref<Readonly<CapabilityState>>>
}

export function useWorkspaceCapabilities(): UseWorkspaceCapabilities {
  return {
    slaEnabled: computed(() => state.value.slaEnabled),
    capabilities: readonly(state),
  }
}
