//! Reserved workspace-slug denylist (Phase 4 W4).
//!
//! A slug is "reserved" when it collides with a platform-level
//! routing concept: marketing subdomains, control-plane services,
//! infra subdomains, or common environment names that could lead a
//! customer into a confusing URL state. Reserving them up front is
//! cheaper than reclaiming a slug later.
//!
//! Two enforcement layers:
//!   * **Application** — `validate_slug` in
//!     `handlers/internal_workspaces.rs` (the M5 internal create
//!     endpoint) and any future admin / control-plane creation
//!     handler call [`is_reserved`] before insertion.
//!   * **Database** — the workspaces table CHECK constraint
//!     (migration `2026-06-04-000000_workspaces_reserved_slugs`)
//!     refuses the same list at the SQL layer so a hand-edit or
//!     forgotten code path can't bypass it.
//!
//! Defense in depth: both layers must agree. If you add an entry
//! here, also extend the CHECK in the migration; if you remove
//! one, write a new migration that drops it from the CHECK.
//!
//! `default` is intentionally NOT reserved: it's the bootstrap
//! workspace's slug on every self-hosted instance, and a CHECK
//! that excluded it would refuse the existing row on a fresh
//! migration run.

/// Slugs the workspace creation flow must refuse. Globally
/// alphabetically sorted so `binary_search` works correctly; new
/// entries go in the alphabetical insertion point.
///
/// Categories represented:
///   * Platform / marketing subdomains (api, app, www, dashboard, ...)
///   * Versioned API + RPC surfaces (api-v1, api-v2, graphql, grpc, ws, wss)
///   * Webhooks / callbacks (webhook, webhooks, callback, callbacks)
///   * Auth surface (auth, oauth, oidc, sso, saml, ldap, mfa, totp)
///   * Anti-phishing brand-confusion guards (account, verify, update,
///     password, security, secure, payment, checkout, wallet, crypto)
///   * Admin variants (admin, administrator, root, superuser, sudo)
///   * Infra / mail / DNS conventions (mail, smtp, ns1, ftp, vpn, ...)
///   * Observability / cluster (metrics, grafana, kibana, prometheus,
///     k8s, kubernetes, cluster, node)
///   * Files / downloads (file, files, upload, download, installer)
///   * Marketing / legal (about, contact, faq, terms, privacy, legal)
///   * Environment names (staging, prod, dev, qa, uat, sandbox, ...)
pub const RESERVED_SLUGS: &[&str] = &[
    "about",
    "abuse",
    "access",
    "account",
    "accounts",
    "adm",
    "admin",
    "administrator",
    "administrators",
    "ads",
    "alpha",
    "alumni",
    "api",
    "api-v1",
    "api-v2",
    "api-v3",
    "app",
    "apps",
    "archive",
    "assets",
    "auth",
    "authenticate",
    "autoconfig",
    "autodiscover",
    "backup",
    "backups",
    "bbs",
    "beta",
    "billing",
    "blog",
    "blogs",
    "bounce",
    "bounces",
    "bugs",
    "cache",
    "cacti",
    "calendar",
    "callback",
    "callbacks",
    "cart",
    "catalog",
    "cdn",
    "cert",
    "certs",
    "changelog",
    "chat",
    "checkout",
    "citrix",
    "cloud",
    "cluster",
    "clusters",
    "cms",
    "community",
    "conference",
    "connect",
    "console",
    "contact",
    "contacts",
    "content",
    "control",
    "copyright",
    "correo",
    "cpanel",
    "crm",
    "crypto",
    "css",
    "dashboard",
    "data",
    "demo",
    "dev",
    "dev2",
    "devel",
    "develop",
    "development",
    "dialin",
    "dkim",
    "dmarc",
    "dns",
    "dns1",
    "dns2",
    "dns3",
    "dns4",
    "doc",
    "docs",
    "documentation",
    "download",
    "download-now",
    "downloads",
    "edge",
    "edu",
    "elearning",
    "email",
    "english",
    "error",
    "events",
    "exchange",
    "extranet",
    "facebook",
    "faq",
    "faqs",
    "feedback",
    "feeds",
    "file",
    "files",
    "forum",
    "forums",
    "ftp",
    "ftp1",
    "ftp2",
    "ftps",
    "gallery",
    "game",
    "games",
    "gateway",
    "get",
    "git",
    "gmail",
    "grafana",
    "graphql",
    "grpc",
    "health",
    "healthcheck",
    "healthz",
    "help",
    "helpcenter",
    "helpdesk",
    "home",
    "host",
    "host2",
    "hosting",
    "id",
    "identity",
    "idp",
    "image",
    "images",
    "images2",
    "imap",
    "imaps",
    "img",
    "img2",
    "inbound",
    "info",
    "install",
    "installer",
    "internal",
    "intranet",
    "invoice",
    "invoices",
    "iphone",
    "ipv4",
    "irc",
    "jabber",
    "jira",
    "job",
    "jobs",
    "jwks",
    "k8s",
    "kb",
    "key",
    "keys",
    "kibana",
    "kubernetes",
    "ldap",
    "legacy",
    "legal",
    "lib",
    "library",
    "list",
    "lists",
    "live",
    "local",
    "localhost",
    "log",
    "login",
    "logout",
    "logs",
    "lyncdiscover",
    "mail",
    "mail1",
    "mail2",
    "mail3",
    "mail4",
    "mailadmin",
    "mailer",
    "mailer-daemon",
    "mailhost",
    "mailserver",
    "manage",
    "marketing",
    "master",
    "media",
    "meet",
    "member",
    "members",
    "metrics",
    "mfa",
    "mobile",
    "monitor",
    "monitoring",
    "moodle",
    "mrtg",
    "msoid",
    "mssql",
    "music",
    "mx",
    "mx1",
    "mx2",
    "mx3",
    "mysql",
    "nagios",
    "new",
    "news",
    "newsletter",
    "no-reply",
    "noreply",
    "nosdesk",
    "ns",
    "ns0",
    "ns1",
    "ns2",
    "ns3",
    "ns4",
    "ns5",
    "ns6",
    "ntp",
    "oauth",
    "oauth2",
    "office",
    "oidc",
    "old",
    "online",
    "owa",
    "panel",
    "partner",
    "partners",
    "passkey",
    "password",
    "passwords",
    "pay",
    "payment",
    "payments",
    "pda",
    "photo",
    "photos",
    "phpmyadmin",
    "ping",
    "plan",
    "plans",
    "poczta",
    "policy",
    "pop",
    "pop3",
    "portal",
    "post",
    "postmaster",
    "preprod",
    "press",
    "preview",
    "pricing",
    "privacy",
    "private",
    "prod",
    "production",
    "project",
    "projects",
    "prometheus",
    "proxy",
    "public",
    "qa",
    "queue",
    "queues",
    "radio",
    "ready",
    "redmine",
    "register",
    "registration",
    "relay",
    "release",
    "releases",
    "remote",
    "reports",
    "root",
    "router",
    "rss",
    "saml",
    "sandbox",
    "search",
    "secure",
    "security",
    "server",
    "server1",
    "service",
    "services",
    "ses",
    "session",
    "sessions",
    "settings",
    "sftp",
    "sharepoint",
    "shop",
    "signin",
    "signout",
    "signup",
    "sip",
    "site",
    "sites",
    "sms",
    "smtp",
    "smtp1",
    "smtp2",
    "smtps",
    "speedtest",
    "spf",
    "sport",
    "sql",
    "ssh",
    "ssl",
    "sso",
    "staff",
    "stage",
    "staging",
    "start",
    "stat",
    "static",
    "stats",
    "status",
    "storage",
    "store",
    "stream",
    "streaming",
    "student",
    "sub",
    "subscribe",
    "subscription",
    "subscriptions",
    "sudo",
    "superuser",
    "support",
    "survey",
    "svn",
    "terms",
    "test",
    "test1",
    "test2",
    "testing",
    "tests",
    "time",
    "tls",
    "token",
    "tokens",
    "tools",
    "totp",
    "trac",
    "training",
    "travel",
    "uat",
    "unsubscribe",
    "update",
    "upgrade",
    "upload",
    "uploads",
    "validate",
    "verify",
    "video",
    "videos",
    "voip",
    "vpn",
    "vpn2",
    "vps",
    "wallet",
    "wap",
    "web",
    "web1",
    "web2",
    "web3",
    "web4",
    "web5",
    "webdisk",
    "webhook",
    "webhooks",
    "webmail",
    "webmail2",
    "websocket",
    "whm",
    "wiki",
    "worker",
    "workers",
    "ws",
    "wss",
    "ww2",
    "www",
    "www1",
    "www2",
    "www3",
    "www4",
    "www5",
    "www6",
    "wwww",
];

/// Returns true when `slug` is in the reserved denylist. Case-
/// insensitive; callers should already have lowercased the input
/// per the slug shape rule, but the safety net is cheap.
pub fn is_reserved(slug: &str) -> bool {
    let lower = slug.to_ascii_lowercase();
    RESERVED_SLUGS.binary_search(&lower.as_str()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserves_routing_subdomains() {
        for s in ["api", "app", "www", "admin", "dashboard", "auth"] {
            assert!(is_reserved(s), "{s} should be reserved");
        }
    }

    #[test]
    fn reserves_infra_subdomains() {
        for s in ["mail", "smtp", "imap", "ns1", "ftp"] {
            assert!(is_reserved(s), "{s} should be reserved");
        }
    }

    #[test]
    fn reserves_environment_names() {
        for s in ["staging", "prod", "production", "dev", "localhost"] {
            assert!(is_reserved(s), "{s} should be reserved");
        }
    }

    #[test]
    fn does_not_reserve_default() {
        // The bootstrap workspace uses slug='default' on every
        // self-hosted instance; reserving it would refuse the
        // existing row.
        assert!(!is_reserved("default"));
    }

    #[test]
    fn does_not_reserve_ordinary_slugs() {
        for s in ["acme", "acme-co", "support-team", "my-company-123"] {
            assert!(!is_reserved(s), "{s} should NOT be reserved");
        }
    }

    #[test]
    fn case_insensitive() {
        assert!(is_reserved("API"));
        assert!(is_reserved("Admin"));
        assert!(is_reserved("WWW"));
    }

    #[test]
    fn list_is_sorted_for_binary_search() {
        // binary_search returns garbage if the slice isn't sorted.
        for window in RESERVED_SLUGS.windows(2) {
            assert!(
                window[0] < window[1],
                "RESERVED_SLUGS must be sorted: {} >= {}",
                window[0],
                window[1]
            );
        }
    }
}
