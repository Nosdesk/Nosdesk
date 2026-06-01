-- =====================================================================
-- Reserved workspace-slug denylist (multi-tenant Phase 4 W4).
--
-- Defense in depth: the application layer
-- (handlers/internal_workspaces.rs::validate_slug + the future Phase 4 W1
-- admin handler) already calls `utils::reserved_slugs::is_reserved`, but
-- a forgotten code path, a CLI tool, or a direct INSERT would otherwise
-- be able to bypass it. This CHECK constraint keeps the slugs out of the
-- table regardless.
--
-- The list MUST match `backend/src/utils/reserved_slugs.rs::RESERVED_SLUGS`.
-- When adding or removing an entry, change both sides in the same PR.
--
-- Source: hand-curated list (platform routing, anti-phishing, infra
-- conventions, common environments) merged with the top 250 of
-- rbsec/dnscan's subdomains-1000.txt (frequency-ranked DNS
-- reconnaissance list of real-world subdomain conventions), with
-- wordlist artifacts and overly-generic vertical names removed.
--
-- `default` is intentionally NOT included: the bootstrap workspace
-- (id=1, inserted by 2026-05-23-100000_workspaces_phase_1) uses that
-- slug, and refusing it here would break the existing row on every
-- migration replay.
-- =====================================================================

ALTER TABLE workspaces
    ADD CONSTRAINT workspaces_slug_not_reserved
    CHECK (slug NOT IN (
        'about', 'access', 'account', 'accounts', 'adm', 'admin', 'administrator', 'administrators',
        'ads', 'alpha', 'alumni', 'api', 'api-v1', 'api-v2', 'api-v3', 'app',
        'apps', 'archive', 'assets', 'auth', 'authenticate', 'autoconfig', 'autodiscover', 'backup',
        'backups', 'bbs', 'beta', 'billing', 'blog', 'blogs', 'bugs', 'cache',
        'cacti', 'calendar', 'callback', 'callbacks', 'cart', 'catalog', 'cdn', 'cert',
        'certs', 'changelog', 'chat', 'checkout', 'citrix', 'cloud', 'cluster', 'clusters',
        'cms', 'community', 'conference', 'connect', 'console', 'contact', 'contacts', 'content',
        'control', 'copyright', 'correo', 'cpanel', 'crm', 'crypto', 'css', 'dashboard',
        'data', 'demo', 'dev', 'dev2', 'devel', 'develop', 'development', 'dialin',
        'dns', 'dns1', 'dns2', 'dns3', 'dns4', 'doc', 'docs', 'documentation',
        'download', 'download-now', 'downloads', 'edge', 'edu', 'elearning', 'email', 'english',
        'error', 'events', 'exchange', 'extranet', 'facebook', 'faq', 'faqs', 'feeds',
        'file', 'files', 'forum', 'forums', 'ftp', 'ftp1', 'ftp2', 'ftps',
        'gallery', 'game', 'games', 'gateway', 'get', 'git', 'gmail', 'grafana',
        'graphql', 'grpc', 'health', 'healthcheck', 'healthz', 'help', 'helpcenter', 'helpdesk',
        'home', 'host', 'host2', 'hosting', 'id', 'identity', 'idp', 'image',
        'images', 'images2', 'imap', 'imaps', 'img', 'img2', 'info', 'install',
        'installer', 'internal', 'intranet', 'invoice', 'invoices', 'iphone', 'ipv4', 'irc',
        'jabber', 'jira', 'job', 'jobs', 'jwks', 'k8s', 'kb', 'key',
        'keys', 'kibana', 'kubernetes', 'ldap', 'legacy', 'legal', 'lib', 'library',
        'list', 'lists', 'live', 'local', 'localhost', 'log', 'login', 'logout',
        'logs', 'lyncdiscover', 'mail', 'mail1', 'mail2', 'mail3', 'mail4', 'mailadmin',
        'mailer', 'mailhost', 'mailserver', 'manage', 'marketing', 'master', 'media', 'meet',
        'member', 'members', 'metrics', 'mfa', 'mobile', 'monitor', 'monitoring', 'moodle',
        'mrtg', 'msoid', 'mssql', 'music', 'mx', 'mx1', 'mx2', 'mx3',
        'mysql', 'nagios', 'new', 'news', 'newsletter', 'nosdesk', 'ns', 'ns0',
        'ns1', 'ns2', 'ns3', 'ns4', 'ns5', 'ns6', 'ntp', 'oauth',
        'oauth2', 'office', 'oidc', 'old', 'online', 'owa', 'panel', 'partner',
        'partners', 'passkey', 'password', 'passwords', 'pay', 'payment', 'payments', 'pda',
        'photo', 'photos', 'phpmyadmin', 'ping', 'plan', 'plans', 'poczta', 'policy',
        'pop', 'pop3', 'portal', 'post', 'preprod', 'press', 'preview', 'pricing',
        'privacy', 'private', 'prod', 'production', 'project', 'projects', 'prometheus', 'proxy',
        'public', 'qa', 'queue', 'queues', 'radio', 'ready', 'redmine', 'register',
        'registration', 'relay', 'release', 'releases', 'remote', 'reports', 'root', 'router',
        'rss', 'saml', 'sandbox', 'search', 'secure', 'security', 'server', 'server1',
        'service', 'services', 'session', 'sessions', 'settings', 'sftp', 'sharepoint', 'shop',
        'signin', 'signout', 'signup', 'sip', 'site', 'sites', 'sms', 'smtp',
        'smtp1', 'smtp2', 'smtps', 'speedtest', 'sport', 'sql', 'ssh', 'ssl',
        'sso', 'staff', 'stage', 'staging', 'start', 'stat', 'static', 'stats',
        'status', 'storage', 'store', 'stream', 'streaming', 'student', 'sub', 'subscribe',
        'subscription', 'subscriptions', 'sudo', 'superuser', 'support', 'survey', 'svn', 'terms',
        'test', 'test1', 'test2', 'testing', 'tests', 'time', 'tls', 'token',
        'tokens', 'tools', 'totp', 'trac', 'training', 'travel', 'uat', 'update',
        'upgrade', 'upload', 'uploads', 'validate', 'verify', 'video', 'videos', 'voip',
        'vpn', 'vpn2', 'vps', 'wallet', 'wap', 'web', 'web1', 'web2',
        'web3', 'web4', 'web5', 'webdisk', 'webhook', 'webhooks', 'webmail', 'webmail2',
        'websocket', 'whm', 'wiki', 'worker', 'workers', 'ws', 'wss', 'ww2',
        'www', 'www1', 'www2', 'www3', 'www4', 'www5', 'www6', 'wwww'
    ));
