/**
 * GitHub Integration Plugin Bundle
 *
 * Provides a ticket sidebar panel for linking GitHub issues to tickets.
 */

// GitHub issue state colors
const STATE_COLORS = {
  open: { bg: 'rgba(34, 197, 94, 0.2)', text: '#22c55e' },
  closed: { bg: 'rgba(168, 85, 247, 0.2)', text: '#a855f7' },
};


/**
 * Main GitHub Panel Component
 */
const GitHubPanel = {
  name: 'GitHubPanel',
  props: ['api', 'context'],

  data() {
    return {
      // State
      linkedIssues: [],
      searchResults: [],
      searchQuery: '',
      isLoading: false,
      isSearching: false,
      error: null,
      showSearch: false,

      // Settings cache
      settings: {
        owner: '',
        repo: '',
      },

      // Icons (stored as data to avoid template interpolation issues)
      icons: {
        github: '<svg viewBox="0 0 24 24" fill="currentColor" class="w-5 h-5"><path d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0024 12c0-6.63-5.37-12-12-12z"/></svg>',
        spinner: '<svg class="animate-spin w-4 h-4" fill="none" viewBox="0 0 24 24"><circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"/><path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"/></svg>',
      },
    };
  },

  computed: {
    ticketId() {
      return this.context?.ticket?.id;
    },
    storageKey() {
      return `ticket_${this.ticketId}_issues`;
    },
    hasLinkedIssues() {
      return this.linkedIssues.length > 0;
    },
  },

  async mounted() {
    await this.loadSettings();
    await this.loadLinkedIssues();
  },

  watch: {
    // Auto-fetch when a valid URL or issue reference is detected
    searchQuery(newVal) {
      // Clear any pending debounce
      if (this._autoFetchTimeout) {
        clearTimeout(this._autoFetchTimeout);
      }

      // Check if it's a valid issue reference
      const issueRef = this.parseIssueReference(newVal);
      if (issueRef) {
        // Debounce to allow for paste completion
        this._autoFetchTimeout = setTimeout(() => {
          this.searchIssues();
        }, 300);
      }
    },
  },

  methods: {
    // Load plugin settings
    async loadSettings() {
      try {
        const owner = await this.api.storage.get('setting:default_owner');
        const repo = await this.api.storage.get('setting:default_repo');
        this.settings.owner = owner || '';
        this.settings.repo = repo || '';
      } catch (e) {
        console.error('Failed to load settings:', e);
      }
    },

    // Load linked issues from storage
    async loadLinkedIssues() {
      if (!this.ticketId) return;

      this.isLoading = true;
      this.error = null;

      try {
        const stored = await this.api.storage.get(this.storageKey);
        const issueRefs = stored || [];

        // Fetch fresh data for each linked issue
        const issues = await Promise.all(
          issueRefs.map(ref => this.fetchIssue(ref.owner, ref.repo, ref.number))
        );

        this.linkedIssues = issues.filter(Boolean);
      } catch (e) {
        this.error = 'Failed to load linked issues';
        console.error(e);
      } finally {
        this.isLoading = false;
      }
    },

    // Fetch a single issue from GitHub API
    // Note: Authorization is automatically injected by the backend proxy
    // based on the github_token secret setting
    async fetchIssue(owner, repo, number) {
      try {
        const response = await this.api.fetch(
          `https://api.github.com/repos/${owner}/${repo}/issues/${number}`,
          {
            method: 'GET',
            headers: { 'Accept': 'application/vnd.github.v3+json' },
          }
        );

        if (!response || !response.ok) return null;

        const data = await response.json();
        return {
          owner,
          repo,
          number: data.number,
          title: data.title,
          state: data.state,
          html_url: data.html_url,
          user: data.user?.login,
          labels: data.labels?.map(l => ({ name: l.name, color: l.color })) || [],
          created_at: data.created_at,
          updated_at: data.updated_at,
        };
      } catch (e) {
        console.error(`Failed to fetch issue ${owner}/${repo}#${number}:`, e);
        return null;
      }
    },

    // Parse issue reference from various formats
    parseIssueReference(input) {
      const trimmed = input.trim();

      // Try GitHub URL format: https://github.com/owner/repo/issues/123
      const urlMatch = trimmed.match(/^https?:\/\/github\.com\/([^/]+)\/([^/]+)\/issues\/(\d+)/);
      if (urlMatch) {
        return { owner: urlMatch[1], repo: urlMatch[2], number: urlMatch[3] };
      }

      // Try shorthand format: owner/repo#123
      const shortMatch = trimmed.match(/^([^/]+)\/([^#]+)#(\d+)$/);
      if (shortMatch) {
        return { owner: shortMatch[1], repo: shortMatch[2], number: shortMatch[3] };
      }

      return null;
    },

    // Search for issues
    async searchIssues() {
      if (!this.searchQuery.trim()) return;

      this.isSearching = true;
      this.error = null;

      try {
        // Parse search query - could be URL, "owner/repo#123", or just search terms
        const issueRef = this.parseIssueReference(this.searchQuery);

        if (issueRef) {
          // Direct issue reference (URL or shorthand)
          const issue = await this.fetchIssue(issueRef.owner, issueRef.repo, issueRef.number);
          this.searchResults = issue ? [issue] : [];
        } else {
          // Search by text in default repo
          const owner = this.settings.owner;
          const repo = this.settings.repo;

          if (!owner || !repo) {
            this.error = 'Set default owner/repo in plugin settings to search by text';
            this.searchResults = [];
            return;
          }

          const q = encodeURIComponent(`${this.searchQuery} repo:${owner}/${repo}`);
          const response = await this.api.fetch(
            `https://api.github.com/search/issues?q=${q}&per_page=5`,
            {
              method: 'GET',
              headers: { 'Accept': 'application/vnd.github.v3+json' },
            }
          );

          if (!response || !response.ok) {
            this.error = 'Search failed';
            this.searchResults = [];
            return;
          }

          const data = await response.json();
          this.searchResults = (data.items || []).map(item => ({
            owner,
            repo,
            number: item.number,
            title: item.title,
            state: item.state,
            html_url: item.html_url,
            user: item.user?.login,
            labels: item.labels?.map(l => ({ name: l.name, color: l.color })) || [],
          }));
        }
      } catch (e) {
        this.error = 'Search failed';
        console.error(e);
      } finally {
        this.isSearching = false;
      }
    },

    // Link an issue to this ticket
    async linkIssue(issue) {
      try {
        const current = await this.api.storage.get(this.storageKey) || [];
        const exists = current.some(
          ref => ref.owner === issue.owner && ref.repo === issue.repo && ref.number === issue.number
        );

        if (!exists) {
          current.push({
            owner: issue.owner,
            repo: issue.repo,
            number: issue.number,
          });
          await this.api.storage.set(this.storageKey, current);
          this.linkedIssues.push(issue);
        }

        // Clear search
        this.showSearch = false;
        this.searchQuery = '';
        this.searchResults = [];
      } catch (e) {
        this.error = 'Failed to link issue';
        console.error(e);
      }
    },

    // Unlink an issue from this ticket
    async unlinkIssue(issue) {
      try {
        const current = await this.api.storage.get(this.storageKey) || [];
        const filtered = current.filter(
          ref => !(ref.owner === issue.owner && ref.repo === issue.repo && ref.number === issue.number)
        );
        await this.api.storage.set(this.storageKey, filtered);
        this.linkedIssues = this.linkedIssues.filter(
          i => !(i.owner === issue.owner && i.repo === issue.repo && i.number === issue.number)
        );
      } catch (e) {
        this.error = 'Failed to unlink issue';
        console.error(e);
      }
    },

    // Format relative time
    formatTime(dateStr) {
      const date = new Date(dateStr);
      const now = new Date();
      const diffMs = now - date;
      const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24));

      if (diffDays === 0) return 'today';
      if (diffDays === 1) return 'yesterday';
      if (diffDays < 7) return `${diffDays} days ago`;
      if (diffDays < 30) return `${Math.floor(diffDays / 7)} weeks ago`;
      return date.toLocaleDateString();
    },

    // Get state badge style
    getStateStyle(state) {
      const colors = STATE_COLORS[state] || STATE_COLORS.open;
      return {
        backgroundColor: colors.bg,
        color: colors.text,
      };
    },
  },

  template: `
    <div class="github-panel flex flex-col gap-2" :data-print-empty="!hasLinkedIssues">
      <!-- Section Header (matches TicketView sidebar sections) -->
      <div class="flex items-center justify-between">
        <div class="flex items-center gap-2">
          <span v-html="icons.github" class="text-secondary"></span>
          <span class="text-secondary font-medium text-sm">GitHub Issues</span>
        </div>
        <a
          @click.prevent="showSearch = !showSearch"
          href="#"
          class="print:hidden text-accent hover:underline text-sm"
        >
          {{ showSearch ? 'Cancel' : 'Link Issue' }}
        </a>
      </div>

      <!-- Error (hidden on print) -->
      <div v-if="error" class="print:hidden p-3 rounded-xl bg-status-error/10 border border-status-error/30 text-status-error text-sm">
        {{ error }}
      </div>

      <!-- Search Panel (hidden on print) -->
      <div v-if="showSearch" class="print:hidden p-3 rounded-xl bg-surface border border-default flex flex-col gap-3">
        <div class="flex gap-2">
          <input
            v-model="searchQuery"
            @keyup.enter="searchIssues"
            type="text"
            placeholder="Paste URL, owner/repo#123, or search"
            class="flex-1 px-3 py-2 text-sm bg-surface-alt border border-default rounded-lg focus:border-accent focus:outline-none text-primary placeholder:text-tertiary"
          />
          <button
            @click="searchIssues"
            :disabled="isSearching"
            class="px-4 py-2 text-sm rounded-lg bg-accent text-white hover:bg-accent/80 disabled:opacity-50 transition-colors font-medium"
          >
            <span v-if="isSearching" v-html="icons.spinner"></span>
            <span v-else>Search</span>
          </button>
        </div>

        <!-- Search Results -->
        <div v-if="searchResults.length > 0" class="space-y-2">
          <div
            v-for="issue in searchResults"
            :key="issue.owner + '/' + issue.repo + '#' + issue.number"
            class="p-3 rounded-lg bg-surface-alt border border-default hover:border-strong cursor-pointer transition-colors"
            @click="linkIssue(issue)"
          >
            <div class="flex items-start justify-between gap-2">
              <div class="flex-1 min-w-0">
                <div class="text-xs text-tertiary">{{ issue.owner }}/{{ issue.repo }}#{{ issue.number }}</div>
                <div class="text-sm text-primary truncate">{{ issue.title }}</div>
              </div>
              <span
                class="px-2 py-1 rounded-md text-xs font-semibold shrink-0"
                :style="getStateStyle(issue.state)"
              >
                {{ issue.state }}
              </span>
            </div>
          </div>
        </div>

        <div v-else-if="searchQuery && !isSearching" class="text-sm text-tertiary text-center py-3">
          No results found
        </div>
      </div>

      <!-- Loading (hidden on print) -->
      <div v-if="isLoading" class="print:hidden flex items-center justify-center py-4">
        <span v-html="icons.spinner" class="text-tertiary"></span>
      </div>

      <!-- Linked Issues (individual cards like LinkedTicketPreview) -->
      <div v-else-if="hasLinkedIssues" class="flex flex-col gap-2">
        <div
          v-for="issue in linkedIssues"
          :key="issue.owner + '/' + issue.repo + '#' + issue.number"
          class="group bg-surface rounded-xl border border-default overflow-hidden hover:border-strong transition-colors"
        >
          <!-- Issue Header -->
          <div class="px-4 py-3 bg-surface-alt border-b border-default flex items-center gap-3">
            <span
              class="flex-shrink-0 inline-flex items-center px-2.5 py-1.5 rounded-md text-xs font-semibold"
              :style="getStateStyle(issue.state)"
            >
              {{ issue.state }}
            </span>
            <a
              :href="issue.html_url"
              target="_blank"
              rel="noopener noreferrer"
              class="text-primary font-medium truncate text-sm group-hover:text-accent transition-colors min-w-0 flex-1"
            >
              {{ issue.title }}
            </a>
            <button
              @click="unlinkIssue(issue)"
              class="print:hidden p-1.5 flex-shrink-0 text-tertiary hover:text-status-error hover:bg-status-error/20 rounded-md transition-colors"
              title="Unlink issue"
            >
              <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
                <path fill-rule="evenodd" d="M4.293 4.293a1 1 0 011.414 0L10 8.586l4.293-4.293a1 1 0 111.414 1.414L11.414 10l4.293 4.293a1 1 0 01-1.414 1.414L10 11.414l-4.293 4.293a1 1 0 01-1.414-1.414L8.586 10 4.293 5.707a1 1 0 010-1.414z" clip-rule="evenodd"/>
              </svg>
            </button>
          </div>

          <!-- Issue Content -->
          <div class="p-4">
            <div class="grid grid-cols-2 gap-3 text-sm">
              <div class="flex flex-col gap-1">
                <span class="text-xs text-tertiary uppercase tracking-wide">Repository</span>
                <span class="text-secondary">{{ issue.owner }}/{{ issue.repo }}</span>
              </div>
              <div class="flex flex-col gap-1">
                <span class="text-xs text-tertiary uppercase tracking-wide">Issue</span>
                <span class="text-secondary">#{{ issue.number }}</span>
              </div>
              <div class="flex flex-col gap-1">
                <span class="text-xs text-tertiary uppercase tracking-wide">Updated</span>
                <span class="text-secondary">{{ formatTime(issue.updated_at) }}</span>
              </div>
              <div v-if="issue.user" class="flex flex-col gap-1">
                <span class="text-xs text-tertiary uppercase tracking-wide">Author</span>
                <span class="text-secondary">{{ issue.user }}</span>
              </div>
            </div>

            <!-- Labels -->
            <div v-if="issue.labels && issue.labels.length > 0" class="flex flex-wrap gap-1.5 mt-3">
              <span
                v-for="label in issue.labels"
                :key="label.name"
                class="px-2 py-0.5 rounded-md text-xs font-medium"
                :style="{ backgroundColor: '#' + label.color + '33', color: '#' + label.color }"
              >
                {{ label.name }}
              </span>
            </div>
          </div>
        </div>
      </div>

      <!-- Empty State (hidden on print - entire panel is hidden when empty) -->
      <div v-else class="print:hidden text-tertiary text-sm">
        No linked GitHub issues
      </div>
    </div>
  `,
};

// Export components
export default {
  GitHubPanel,
};
