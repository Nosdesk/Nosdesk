# Settings Components

This directory contains the modular settings components for the ProfileSettingsView. The original monolithic ProfileSettingsView has been broken down into smaller, focused components for better maintainability and organization.

## Components

### `UserProfileCard.vue`
- Discord-inspired profile card with avatar and banner
- Profile information form fields (name, email, pronouns)
- Image upload functionality for avatar and banner
- Individual save buttons for each field

### `AppearanceSettings.vue`
- Theme selection (light/dark mode)
- Display options (compact view toggle)
- Integration with the theme store

### `NotificationSettings.vue`
- Email notification preferences
- Desktop notification settings with browser permission handling

### `SecuritySettings.vue`
- Password change form
- Form validation and security checks

### `MFASettings.vue`
- Multi-factor authentication setup and management
- QR code display for authenticator apps
- Verification code input
- MFA enable/disable functionality

### `AuthMethodsSettings.vue`
- Authentication methods management
- OAuth provider connections (Google, GitHub)

### `SessionsSettings.vue`
- Active sessions, one row per signed-in device
- Per-session sign-out, plus a bulk sign-out gated behind step-up re-auth
- Self-only: the endpoints resolve the user from the JWT, so it takes no
  target-user prop and is hidden in admin mode

### `index.ts`
- Barrel export file for easy component importing
- Allows importing all components from a single location

## Usage

```vue
<script setup lang="ts">
import {
  UserProfileCard,
  AppearanceSettings,
  NotificationSettings,
  SecuritySettings,
  MFASettings,
  AuthMethodsSettings
} from '@/components/settings';
</script>
```

## Event Handling

All components emit standardized events:
- `@success` - Emitted with a success message string
- `@error` - Emitted with an error message string

These events are handled centrally in the ProfileSettingsView for consistent user feedback.

## Styling

All components follow the consistent design system:
- Slate color palette with proper contrast
- Rounded card layouts with subtle borders
- Hover effects and transitions
- Consistent form field styling
- Proper focus states and accessibility

## Benefits of This Structure

1. **Maintainability**: Each component has a single responsibility
2. **Reusability**: Components can be reused in other views if needed
3. **Testing**: Easier to write unit tests for individual components
4. **Performance**: Can lazy load components if needed
5. **Development**: Multiple developers can work on different settings sections simultaneously
6. **Code Organization**: Related functionality is grouped together 