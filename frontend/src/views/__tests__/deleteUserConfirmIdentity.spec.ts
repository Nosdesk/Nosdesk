import { describe, expect, it } from 'vitest'
import profileSettings from '../ProfileSettingsView.vue?raw'
import usersList from '../UsersListView.vue?raw'

// Deleting one user and bulk-deleting users both run only after the admin has
// confirmed it's them. The views are too large to mount here, so this pins the
// wiring; ConfirmIdentity and the sign-out-others dialog (SessionsSettings)
// cover the behaviour end to end.
describe.each([
  ['ProfileSettingsView', profileSettings, 'deleteAccount', 'userService.deleteUser('],
  ['UsersListView', usersList, 'confirmDelete', 'userService.bulkAction('],
])('%s delete confirms it is you first', (_name, source, handler, action) => {
  it('renders ConfirmIdentity in the dialog, wired to the button', () => {
    expect(source).toMatch(
      new RegExp(`<ConfirmIdentity\\s+ref="deleteIdentity"\\s+v-model:ready="deleteIdentityReady"\\s+@submit="${handler}"`),
    )
    // The button stays off until the status has loaded and there is proof.
    expect(source).toMatch(/const deleteConfirmDisabled = computed\(\(\) => \w+\.value \|\| !deleteIdentityReady\.value\)/)
  })

  it('confirms before the action, and sends no credential with it', () => {
    const start = source.search(new RegExp(`(const ${handler} = async|async function ${handler})`))
    expect(start).toBeGreaterThan(-1)
    const body = source.slice(start)
    const confirmAt = body.indexOf('await deleteIdentity.value?.confirm()')
    const actionAt = body.indexOf(action)
    expect(confirmAt).toBeGreaterThan(-1)
    expect(actionAt).toBeGreaterThan(confirmAt)
    const call = body.slice(actionAt, body.indexOf(')', actionAt) + 1)
    expect(call).not.toMatch(/password|mfa_code|credential|StepUp/i)
  })

  it('shows a failed action inline and re-checks the confirmation', () => {
    expect(source).toContain('void deleteIdentity.value?.refresh()')
    expect(source).toMatch(/<p v-if="deleteError" class="text-sm text-status-error">/)
  })

  it('no longer asks for a credential of its own', () => {
    expect(source).not.toMatch(/useStepUpMethod|deleteCredential|stepup-password|stepup-mfa/)
  })
})
