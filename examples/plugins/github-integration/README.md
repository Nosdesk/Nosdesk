# GitHub Integration Plugin

Example plugin that demonstrates Nosdesk's plugin system by integrating with GitHub.

## Features

- **Link Issues**: Search and link GitHub issues to Nosdesk tickets
- **View Status**: See issue state (open/closed), labels, and recent activity
- **Quick Access**: Click linked issues to open them in GitHub

## Installation

This plugin is provisioned automatically when mounted at `/app/plugins/github-integration`.

### Docker Compose

```yaml
volumes:
  - ./examples/plugins:/app/plugins:ro
```

### Manual Install

Go to **Administration → Plugins → Install Plugin** and upload a zip containing:
- `manifest.json`
- `bundle.js`

## GitHub PAT Permissions

Create a Personal Access Token at https://github.com/settings/tokens

### Fine-Grained Token (Recommended)

1. Go to **Settings → Developer settings → Personal access tokens → Fine-grained tokens**
2. Click **Generate new token**
3. Set token name and expiration
4. Under **Repository access**, select your repositories
5. Under **Permissions → Repository permissions**:
   - **Issues**: Read-only
6. Generate token

### Classic Token

1. Go to **Settings → Developer settings → Personal access tokens → Tokens (classic)**
2. Click **Generate new token**
3. Select scopes:
   - `repo` - Full access to private repositories (if accessing private repos)
   - `public_repo` - Access to public repositories only
4. Generate token

## Configuration

1. Go to **Administration → Plugins**
2. Find "GitHub Integration" and click the settings icon
3. Enter your GitHub Personal Access Token
4. Optionally set default owner/repo for text search:
   - **Default Owner**: e.g., `nosdesk`
   - **Default Repository**: e.g., `helpdesk`

## Usage

1. Open any ticket
2. Find the "GitHub Issues" panel in the sidebar
3. Click "Link Issue" to search
4. Enter either:
   - Direct reference: `nosdesk/helpdesk#42`
   - Search text (requires default owner/repo configured)
5. Click a search result to link it
6. Hover over linked issues to see the unlink button

## Plugin API Usage

This plugin demonstrates:

- **`api.storage.get/set`**: Persist linked issues per ticket
- **`api.fetch`**: Make requests to GitHub API via proxy
- **`context.ticket`**: Access current ticket information
- **Component rendering**: Vue component in ticket-sidebar slot

## Authentication

The backend proxy automatically injects Authorization headers for known APIs:

- `github_token` (secret setting) → `Authorization: Bearer <token>` for `api.github.com`
- `gitlab_token` (secret setting) → `Authorization: Bearer <token>` for `gitlab.com`

This means plugins don't need to handle auth tokens directly - just configure the secret in plugin settings.

## Development

The bundle is a standard ES module exporting Vue components:

```javascript
export default {
  GitHubPanel: {
    props: ['api', 'context'],
    // ... component definition
  }
};
```

Components receive:
- `api` - The Plugin API instance with tickets, storage, fetch, etc.
- `context` - Current context with ticket/device data
