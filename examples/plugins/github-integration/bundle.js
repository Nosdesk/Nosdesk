/**
 * GitHub Integration Plugin Bundle
 *
 * Provides a ticket sidebar panel for linking GitHub issues to tickets.
 * Uses typed collections to store linked issue data with cached GitHub metadata.
 */

/**
 * Main GitHub Panel Component
 */
const GitHubPanel = {
  name: 'GitHubPanel',
  props: ['api', 'context', 'actionActivated'],

  data() {
    return {
      linkedIssues: [],
      searchResults: [],
      searchQuery: '',
      isLoading: true,
      isSearching: false,
      isRefreshing: false,
      error: null,
      showSearch: false,
      settings: { owner: '', repo: '' },
    };
  },

  computed: {
    ticketId() {
      return this.context?.ticket?.id;
    },
    collection() {
      return this.api.collections.get('linked_issues');
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
    actionActivated(newVal) {
      if (newVal) {
        this.showSearch = true;
        this.$nextTick(() => {
          if (this.$refs.searchInput) {
            this.$refs.searchInput.focus();
          }
        });
      }
    },
    searchQuery(newVal) {
      if (this._autoFetchTimeout) clearTimeout(this._autoFetchTimeout);
      const issueRef = this.parseIssueReference(newVal);
      if (issueRef) {
        this._autoFetchTimeout = setTimeout(() => this.searchIssues(), 300);
      }
    },
  },

  methods: {
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

    async loadLinkedIssues() {
      if (!this.ticketId) return;
      this.isLoading = true;
      this.error = null;
      try {
        const result = await this.collection.list({
          filter: JSON.stringify({ ticket_id: this.ticketId }),
          limit: 100,
        });
        this.linkedIssues = (result.rows || []).map(row => ({
          uuid: row.uuid,
          owner: row.data.owner,
          repo: row.data.repo,
          number: row.data.issue_number,
          title: row.data.issue_title || '#' + row.data.issue_number,
          state: row.data.issue_state || 'unknown',
          html_url: row.data.issue_url || 'https://github.com/' + row.data.owner + '/' + row.data.repo + '/issues/' + row.data.issue_number,
          user: row.data.issue_author,
          labels: row.data.issue_labels || [],
          created_at: row.data.issue_created_at,
          updated_at: row.data.issue_updated_at,
        }));
      } catch (e) {
        this.error = 'Failed to load linked issues';
        console.error(e);
      } finally {
        this.isLoading = false;
      }
    },

    async fetchIssue(owner, repo, number) {
      try {
        const response = await this.api.fetch(
          'https://api.github.com/repos/' + owner + '/' + repo + '/issues/' + number,
          { method: 'GET', headers: { 'Accept': 'application/vnd.github.v3+json' } }
        );
        if (!response || !response.ok) return null;
        const data = await response.json();
        return {
          owner, repo,
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
        console.error('Failed to fetch issue ' + owner + '/' + repo + '#' + number + ':', e);
        return null;
      }
    },

    parseIssueReference(input) {
      const trimmed = input.trim();
      const urlMatch = trimmed.match(/^https?:\/\/github\.com\/([^/]+)\/([^/]+)\/issues\/(\d+)/);
      if (urlMatch) return { owner: urlMatch[1], repo: urlMatch[2], number: urlMatch[3] };
      const shortMatch = trimmed.match(/^([^/]+)\/([^#]+)#(\d+)$/);
      if (shortMatch) return { owner: shortMatch[1], repo: shortMatch[2], number: shortMatch[3] };
      return null;
    },

    async searchIssues() {
      if (!this.searchQuery.trim()) return;
      this.isSearching = true;
      this.error = null;
      try {
        const issueRef = this.parseIssueReference(this.searchQuery);
        if (issueRef) {
          const issue = await this.fetchIssue(issueRef.owner, issueRef.repo, issueRef.number);
          this.searchResults = issue ? [issue] : [];
        } else {
          const { owner, repo } = this.settings;
          if (!owner || !repo) {
            this.error = 'Set default owner/repo in plugin settings to search by text';
            this.searchResults = [];
            return;
          }
          const q = encodeURIComponent(this.searchQuery + ' repo:' + owner + '/' + repo);
          const response = await this.api.fetch(
            'https://api.github.com/search/issues?q=' + q + '&per_page=5',
            { method: 'GET', headers: { 'Accept': 'application/vnd.github.v3+json' } }
          );
          if (!response || !response.ok) {
            this.error = 'Search failed';
            this.searchResults = [];
            return;
          }
          const data = await response.json();
          this.searchResults = (data.items || []).map(item => ({
            owner, repo,
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

    async linkIssue(issue) {
      try {
        const exists = this.linkedIssues.some(
          i => i.owner === issue.owner && i.repo === issue.repo && i.number === issue.number
        );
        if (!exists) {
          const row = await this.collection.create({
            ticket_id: this.ticketId,
            owner: issue.owner,
            repo: issue.repo,
            issue_number: issue.number,
            issue_title: issue.title || null,
            issue_state: issue.state || null,
            issue_url: issue.html_url || 'https://github.com/' + issue.owner + '/' + issue.repo + '/issues/' + issue.number,
            issue_author: issue.user || null,
            issue_labels: issue.labels || [],
            issue_created_at: issue.created_at || null,
            issue_updated_at: issue.updated_at || null,
          });
          if (row) {
            this.linkedIssues.push({
              uuid: row.uuid,
              owner: issue.owner,
              repo: issue.repo,
              number: issue.number,
              title: issue.title || '#' + issue.number,
              state: issue.state || 'unknown',
              html_url: issue.html_url || 'https://github.com/' + issue.owner + '/' + issue.repo + '/issues/' + issue.number,
              user: issue.user,
              labels: issue.labels || [],
              created_at: issue.created_at,
              updated_at: issue.updated_at,
            });
          }
        }
        this.showSearch = false;
        this.searchQuery = '';
        this.searchResults = [];
      } catch (e) {
        this.error = 'Failed to link issue';
        console.error(e);
      }
    },

    async unlinkIssue(issue) {
      try {
        const success = await this.collection.delete(issue.uuid);
        if (success) {
          this.linkedIssues = this.linkedIssues.filter(i => i.uuid !== issue.uuid);
        }
      } catch (e) {
        this.error = 'Failed to unlink issue';
        console.error(e);
      }
    },

    async refreshIssues() {
      if (!this.hasLinkedIssues) return;
      this.isRefreshing = true;
      this.error = null;
      try {
        const updated = await Promise.all(
          this.linkedIssues.map(async (issue) => {
            const fresh = await this.fetchIssue(issue.owner, issue.repo, issue.number);
            if (!fresh) return issue;
            await this.collection.update(issue.uuid, {
              ticket_id: this.ticketId,
              owner: issue.owner,
              repo: issue.repo,
              issue_number: issue.number,
              issue_title: fresh.title,
              issue_state: fresh.state,
              issue_url: fresh.html_url,
              issue_author: fresh.user,
              issue_labels: fresh.labels,
              issue_created_at: fresh.created_at,
              issue_updated_at: fresh.updated_at,
            });
            return { ...issue, ...fresh };
          })
        );
        this.linkedIssues = updated;
      } catch (e) {
        this.error = 'Failed to refresh issues';
        console.error(e);
      } finally {
        this.isRefreshing = false;
      }
    },

    formatTime(dateStr) {
      if (!dateStr) return '';
      const date = new Date(dateStr);
      const now = new Date();
      const diffMs = now - date;
      const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24));
      if (diffDays === 0) return 'today';
      if (diffDays === 1) return 'yesterday';
      if (diffDays < 7) return diffDays + 'd ago';
      if (diffDays < 30) return Math.floor(diffDays / 7) + 'w ago';
      return date.toLocaleDateString();
    },

    getStateBadgeClass(state) {
      if (state === 'open') return 'bg-status-success/20 text-status-success border-status-success/30';
      if (state === 'closed') return 'bg-purple-500/20 text-purple-400 border-purple-500/30';
      return 'bg-surface-alt text-secondary border-default';
    },
  },

  template: `
    <div v-if="hasLinkedIssues || showSearch" class="github-panel flex flex-col gap-2" :data-print-empty="!hasLinkedIssues">

      <!-- Section Header — shown when plugin has content or search is active -->
      <div class="flex items-center justify-between">
        <h3 class="text-sm font-medium text-secondary">GitHub Issues</h3>
        <div class="print:hidden flex items-center gap-3">
          <button
            v-if="hasLinkedIssues && !isRefreshing"
            @click="refreshIssues"
            class="text-tertiary hover:text-accent transition-colors"
            title="Refresh from GitHub"
          >
            <svg class="w-3.5 h-3.5" viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M4 2a1 1 0 011 1v2.101a7.002 7.002 0 0111.601 2.566 1 1 0 11-1.885.666A5.002 5.002 0 005.999 7H9a1 1 0 010 2H4a1 1 0 01-1-1V3a1 1 0 011-1zm.008 9.057a1 1 0 011.276.61A5.002 5.002 0 0014.001 13H11a1 1 0 110-2h5a1 1 0 011 1v5a1 1 0 11-2 0v-2.101a7.002 7.002 0 01-11.601-2.566 1 1 0 01.61-1.276z" clip-rule="evenodd"/></svg>
          </button>
          <svg v-if="isRefreshing" class="w-3.5 h-3.5 animate-spin text-tertiary" fill="none" viewBox="0 0 24 24"><circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"/><path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"/></svg>
          <button
            v-if="showSearch"
            @click="showSearch = false"
            class="text-xs font-medium text-tertiary hover:text-status-error transition-colors"
          >Cancel</button>
          <button
            v-else
            @click="showSearch = true"
            class="flex items-center gap-1 text-xs font-medium text-tertiary hover:text-accent transition-colors"
          >
            <svg class="w-3.5 h-3.5" viewBox="0 0 20 20" fill="currentColor"><path d="M10 3a1 1 0 011 1v5h5a1 1 0 110 2h-5v5a1 1 0 11-2 0v-5H4a1 1 0 110-2h5V4a1 1 0 011-1z"/></svg>
            Link issue
          </button>
        </div>
      </div>

      <!-- Error -->
      <div v-if="error" class="print:hidden text-sm text-status-error">{{ error }}</div>

      <!-- Search Panel -->
      <div v-if="showSearch" class="print:hidden flex flex-col gap-2">
        <div class="flex gap-2">
          <input
            ref="searchInput"
            v-model="searchQuery"
            @keyup.enter="searchIssues"
            type="text"
            placeholder="URL, owner/repo#123, or search..."
            class="flex-1 px-3 py-1.5 text-sm bg-surface border border-default rounded-lg focus:border-accent focus:outline-none text-primary placeholder:text-tertiary"
          />
          <button
            @click="searchIssues"
            :disabled="isSearching || !searchQuery.trim()"
            class="px-3 py-1.5 text-sm rounded-lg bg-accent text-white hover:bg-accent/80 disabled:opacity-50 transition-colors font-medium"
          >
            <svg v-if="isSearching" class="w-4 h-4 animate-spin" fill="none" viewBox="0 0 24 24"><circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"/><path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"/></svg>
            <span v-else>Search</span>
          </button>
        </div>

        <!-- Search Results -->
        <div v-if="searchResults.length > 0" class="flex flex-col gap-1.5">
          <div
            v-for="issue in searchResults"
            :key="issue.owner + '/' + issue.repo + '#' + issue.number"
            class="flex items-center gap-2 px-3 py-2 rounded-lg bg-surface border border-default hover:border-strong cursor-pointer transition-colors"
            @click="linkIssue(issue)"
          >
            <span
              class="flex-shrink-0 inline-flex items-center px-2 py-0.5 rounded-md text-xs font-semibold border"
              :class="getStateBadgeClass(issue.state)"
            >{{ issue.state }}</span>
            <div class="flex-1 min-w-0">
              <div class="text-sm text-primary truncate">{{ issue.title }}</div>
              <div class="text-xs text-tertiary">{{ issue.owner }}/{{ issue.repo }}#{{ issue.number }}</div>
            </div>
          </div>
        </div>
        <div v-else-if="searchQuery && !isSearching" class="text-sm text-tertiary py-2">
          No results found
        </div>
      </div>

      <!-- Linked Issues -->
      <template v-if="hasLinkedIssues">
        <div
          v-for="issue in linkedIssues"
          :key="issue.uuid"
          class="group bg-surface rounded-xl border border-default overflow-hidden hover:border-strong transition-colors"
        >
          <!-- Card Header -->
          <div class="px-4 py-3 bg-surface-alt border-b border-default">
            <div class="flex items-center justify-between gap-2">
              <div class="flex items-center gap-3 min-w-0 flex-1">
                <span
                  class="flex-shrink-0 inline-flex items-center px-2.5 py-1.5 rounded-md text-xs font-semibold border"
                  :class="getStateBadgeClass(issue.state)"
                >{{ issue.state }}</span>
                <a
                  :href="issue.html_url"
                  target="_blank"
                  rel="noopener noreferrer"
                  class="text-md font-medium text-primary truncate group-hover:text-accent transition-colors min-w-0 flex-1"
                >{{ issue.title }}</a>
              </div>
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
          </div>

          <!-- Card Body -->
          <div class="p-4">
            <div class="grid grid-cols-2 gap-3 text-sm">
              <div class="flex flex-col gap-1">
                <span class="text-xs text-tertiary uppercase tracking-wide">Repository</span>
                <span class="text-secondary font-mono text-sm">{{ issue.owner }}/{{ issue.repo }}</span>
              </div>
              <div class="flex flex-col gap-1">
                <span class="text-xs text-tertiary uppercase tracking-wide">Issue</span>
                <span class="text-secondary font-mono text-sm">#{{ issue.number }}</span>
              </div>
              <div v-if="issue.updated_at" class="flex flex-col gap-1">
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
                class="px-2 py-0.5 rounded-md text-xs font-medium border"
                :style="{ backgroundColor: '#' + label.color + '20', color: '#' + label.color, borderColor: '#' + label.color + '40' }"
              >{{ label.name }}</span>
            </div>
          </div>
        </div>
      </template>

    </div>
  `,
};

// Export components
export default {
  GitHubPanel,
};
