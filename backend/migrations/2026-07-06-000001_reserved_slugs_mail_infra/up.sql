-- Extend the reserved-slug CHECK with the mail-infrastructure names the
-- managed email identity work makes load-bearing (`bounce` is the SES MAIL
-- FROM host, `inbound` the token-address host, both under the tenant domain
-- alongside workspace slugs) plus the RFC 2142 / anti-phishing mail set
-- (postmaster, abuse, noreply, dmarc, dkim, spf, mailer-daemon, ses,
-- feedback, unsubscribe, no-reply, bounces).
--
-- Mirrors utils/reserved_slugs.rs exactly (defense in depth: both layers
-- must agree). Regenerated as a full list, not a delta, so the constraint is
-- always the module's list verbatim.
ALTER TABLE workspaces DROP CONSTRAINT workspaces_slug_not_reserved;
ALTER TABLE workspaces ADD CONSTRAINT workspaces_slug_not_reserved
    CHECK (slug <> ALL (ARRAY['about', 'abuse', 'access', 'account', 'accounts', 'adm', 'admin', 'administrator', 'administrators', 'ads', 'alpha', 'alumni', 'api', 'api-v1', 'api-v2', 'api-v3', 'app', 'apps', 'archive', 'assets', 'auth', 'authenticate', 'autoconfig', 'autodiscover', 'backup', 'backups', 'bbs', 'beta', 'billing', 'blog', 'blogs', 'bounce', 'bounces', 'bugs', 'cache', 'cacti', 'calendar', 'callback', 'callbacks', 'cart', 'catalog', 'cdn', 'cert', 'certs', 'changelog', 'chat', 'checkout', 'citrix', 'cloud', 'cluster', 'clusters', 'cms', 'community', 'conference', 'connect', 'console', 'contact', 'contacts', 'content', 'control', 'copyright', 'correo', 'cpanel', 'crm', 'crypto', 'css', 'dashboard', 'data', 'demo', 'dev', 'dev2', 'devel', 'develop', 'development', 'dialin', 'dkim', 'dmarc', 'dns', 'dns1', 'dns2', 'dns3', 'dns4', 'doc', 'docs', 'documentation', 'download', 'download-now', 'downloads', 'edge', 'edu', 'elearning', 'email', 'english', 'error', 'events', 'exchange', 'extranet', 'facebook', 'faq', 'faqs', 'feedback', 'feeds', 'file', 'files', 'forum', 'forums', 'ftp', 'ftp1', 'ftp2', 'ftps', 'gallery', 'game', 'games', 'gateway', 'get', 'git', 'gmail', 'grafana', 'graphql', 'grpc', 'health', 'healthcheck', 'healthz', 'help', 'helpcenter', 'helpdesk', 'home', 'host', 'host2', 'hosting', 'id', 'identity', 'idp', 'image', 'images', 'images2', 'imap', 'imaps', 'img', 'img2', 'inbound', 'info', 'install', 'installer', 'internal', 'intranet', 'invoice', 'invoices', 'iphone', 'ipv4', 'irc', 'jabber', 'jira', 'job', 'jobs', 'jwks', 'k8s', 'kb', 'key', 'keys', 'kibana', 'kubernetes', 'ldap', 'legacy', 'legal', 'lib', 'library', 'list', 'lists', 'live', 'local', 'localhost', 'log', 'login', 'logout', 'logs', 'lyncdiscover', 'mail', 'mail1', 'mail2', 'mail3', 'mail4', 'mailadmin', 'mailer', 'mailer-daemon', 'mailhost', 'mailserver', 'manage', 'marketing', 'master', 'media', 'meet', 'member', 'members', 'metrics', 'mfa', 'mobile', 'monitor', 'monitoring', 'moodle', 'mrtg', 'msoid', 'mssql', 'music', 'mx', 'mx1', 'mx2', 'mx3', 'mysql', 'nagios', 'new', 'news', 'newsletter', 'no-reply', 'noreply', 'nosdesk', 'ns', 'ns0', 'ns1', 'ns2', 'ns3', 'ns4', 'ns5', 'ns6', 'ntp', 'oauth', 'oauth2', 'office', 'oidc', 'old', 'online', 'owa', 'panel', 'partner', 'partners', 'passkey', 'password', 'passwords', 'pay', 'payment', 'payments', 'pda', 'photo', 'photos', 'phpmyadmin', 'ping', 'plan', 'plans', 'poczta', 'policy', 'pop', 'pop3', 'portal', 'post', 'postmaster', 'preprod', 'press', 'preview', 'pricing', 'privacy', 'private', 'prod', 'production', 'project', 'projects', 'prometheus', 'proxy', 'public', 'qa', 'queue', 'queues', 'radio', 'ready', 'redmine', 'register', 'registration', 'relay', 'release', 'releases', 'remote', 'reports', 'root', 'router', 'rss', 'saml', 'sandbox', 'search', 'secure', 'security', 'server', 'server1', 'service', 'services', 'ses', 'session', 'sessions', 'settings', 'sftp', 'sharepoint', 'shop', 'signin', 'signout', 'signup', 'sip', 'site', 'sites', 'sms', 'smtp', 'smtp1', 'smtp2', 'smtps', 'speedtest', 'spf', 'sport', 'sql', 'ssh', 'ssl', 'sso', 'staff', 'stage', 'staging', 'start', 'stat', 'static', 'stats', 'status', 'storage', 'store', 'stream', 'streaming', 'student', 'sub', 'subscribe', 'subscription', 'subscriptions', 'sudo', 'superuser', 'support', 'survey', 'svn', 'terms', 'test', 'test1', 'test2', 'testing', 'tests', 'time', 'tls', 'token', 'tokens', 'tools', 'totp', 'trac', 'training', 'travel', 'uat', 'unsubscribe', 'update', 'upgrade', 'upload', 'uploads', 'validate', 'verify', 'video', 'videos', 'voip', 'vpn', 'vpn2', 'vps', 'wallet', 'wap', 'web', 'web1', 'web2', 'web3', 'web4', 'web5', 'webdisk', 'webhook', 'webhooks', 'webmail', 'webmail2', 'websocket', 'whm', 'wiki', 'worker', 'workers', 'ws', 'wss', 'ww2', 'www', 'www1', 'www2', 'www3', 'www4', 'www5', 'www6', 'wwww'])) NOT VALID;

-- A workspace created before this may already use one of the names it adds.
-- Rename it instead of failing the upgrade: <slug>-workspace, else <slug>-2,
-- -3 and so on, skipping slugs that are taken, retired or reserved. The
-- constraint goes on NOT VALID first so it rejects a reserved candidate, then
-- is validated. Diesel discards notices, so each rename is a WARNING, which
-- reaches the Postgres server log. `workspaces` has no audit trigger to
-- disable for the UPDATE.
DO $$
DECLARE
    ws record;
    candidate text;
    n int;
BEGIN
    FOR ws IN
        SELECT id, uuid, slug::text AS slug FROM workspaces
        WHERE slug = ANY (ARRAY['abuse', 'bounce', 'bounces', 'dkim', 'dmarc', 'feedback', 'inbound', 'mailer-daemon', 'no-reply', 'noreply', 'postmaster', 'ses', 'spf', 'unsubscribe'])
        ORDER BY id
    LOOP
        n := 1;
        LOOP
            candidate := ws.slug || CASE WHEN n = 1 THEN '-workspace' ELSE '-' || n END;
            IF NOT EXISTS (SELECT 1 FROM retired_slugs WHERE slug = candidate) THEN
                BEGIN
                    UPDATE workspaces SET slug = candidate WHERE id = ws.id;
                    RAISE WARNING 'workspace % renamed from "%" to "%": "%" is now reserved',
                        ws.uuid, ws.slug, candidate, ws.slug;
                    EXIT;
                EXCEPTION WHEN unique_violation OR check_violation THEN
                    NULL; -- taken or reserved: try the next suffix
                END;
            END IF;
            n := n + 1;
            IF n > 1000 THEN
                RAISE EXCEPTION 'no free slug for workspace % ("%")', ws.uuid, ws.slug;
            END IF;
        END LOOP;
    END LOOP;
END $$;
ALTER TABLE workspaces VALIDATE CONSTRAINT workspaces_slug_not_reserved;
