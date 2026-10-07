//! A workspace's emails use its own default language. The locale resolver
//! reads the settings of the workspace the connection is pinned to, so a
//! second workspace's default isn't swapped for the first one's.

#![allow(clippy::expect_used)]

use backend::models::UpdateSiteSettings;
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;
use backend::utils::email::{EmailConfig, EmailService, SmtpSecurity};
use backend::utils::email_branding::{get_email_branding, SentFrom};

fn pinned<T>(
    conn: &mut backend::db::DbConnection,
    workspace_id: i32,
    f: impl FnOnce(&mut backend::db::DbConnection) -> diesel::QueryResult<T>,
) -> T {
    let actor = ActorContext::system("test:email_locale").with_workspace(workspace_id);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, f).expect("pinned")
}

fn default_locale(conn: &mut backend::db::DbConnection, workspace_id: i32, locale: &str) {
    pinned(conn, workspace_id, |c| {
        backend::repository::site_settings::update_site_settings(
            c,
            UpdateSiteSettings {
                default_locale: Some(locale.to_string()),
                ..Default::default()
            },
        )
    });
}

#[test]
fn a_workspace_default_language_reaches_its_emails() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(2);
    let mut conn = pool.get().expect("conn");
    let second = crate::common::mint_workspace(&mut conn, "lingua", "Lingua");
    // Someone with no language of their own: the workspace's default decides.
    let requester = crate::common::insert_plain_user(&mut conn, "Requester");
    default_locale(&mut conn, 1, "nl-NL");
    default_locale(&mut conn, second, "fr-FR");

    let svc = EmailService::new(EmailConfig {
        smtp_host: "smtp.example.com".into(),
        smtp_port: 587,
        smtp_username: String::new(),
        smtp_password: String::new(),
        from_name: "Lingua".into(),
        from_email: "help@lingua.example.com".into(),
        enabled: true,
        security: SmtpSecurity::StartTls,
    });
    let (locale, subject) = pinned(&mut conn, second, |c| {
        let locale = backend::repository::user_locale::resolve_effective_locale(c, requester);
        let branding = get_email_branding(c, "https://lingua.example.com", SentFrom::Workspace);
        let row = backend::services::transactional_email::prepare_portal_magic_link(
            &svc,
            &branding,
            "requester@example.com",
            "Requester",
            "token",
            None,
            &locale,
        );
        Ok((locale.to_string(), row.subject))
    });
    assert_eq!(locale, "fr-FR");
    assert!(subject.starts_with("Connectez-vous"), "{subject}");

    // The first workspace still reads its own.
    let first = pinned(&mut conn, 1, |c| {
        Ok(backend::repository::user_locale::resolve_effective_locale(c, requester).to_string())
    });
    assert_eq!(first, "nl-NL");
}
