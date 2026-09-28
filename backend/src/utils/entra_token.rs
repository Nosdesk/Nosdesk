//! Checks a Microsoft Entra access token that a workspace's own Entra app
//! issued for our API: the Teams tab gets one from Teams (nested app
//! authentication) and trades it for a portal session.
//!
//! The token must be signed by the tenant's current keys, issued by that tenant
//! (`iss` and `tid`), meant for the workspace's app (`aud`), carry our scope
//! (`scp` has `access_as_user`, so a token for some other API of the same app
//! doesn't pass) and be for a person, not an application.
//!
//! The person is `tid` + `oid`, which Microsoft keeps stable. The email (the
//! `email` claim, else the UPN) is only used to find or create their account,
//! under the same domain rules as requester SSO, and never trusted when Entra
//! says the domain owner is unverified (`xms_edov` false).

use jsonwebtoken::{jwk::JwkSet, Algorithm, DecodingKey, Validation};
use serde::Deserialize;

/// The scope the workspace's app exposes for the tab.
pub const SCOPE: &str = "access_as_user";

/// Who a checked token is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntraUser {
    pub tenant_id: String,
    pub object_id: String,
    pub email: Option<String>,
    /// `xms_edov`: whether Entra verified the email domain's owner. `None` when
    /// the token doesn't say.
    pub email_verified: Option<bool>,
    pub name: Option<String>,
}

/// Why a token was refused (for the log, never shown).
pub type Refusal = &'static str;

#[derive(Deserialize)]
struct Claims {
    tid: String,
    #[serde(default)]
    oid: Option<String>,
    #[serde(default)]
    scp: Option<String>,
    #[serde(default)]
    idtyp: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    xms_edov: Option<serde_json::Value>,
    #[serde(default)]
    name: Option<String>,
    /// v2 tokens: the sign-in name, usually the UPN.
    #[serde(default)]
    preferred_username: Option<String>,
    /// v1 tokens: the UPN.
    #[serde(default)]
    upn: Option<String>,
}

/// The tenant id in an Entra v2 issuer (`https://login.microsoftonline.com/{tid}/v2.0`).
pub fn tenant_of(issuer: &str) -> Option<&str> {
    let tid = issuer
        .strip_prefix("https://login.microsoftonline.com/")?
        .strip_suffix("/v2.0")?;
    uuid::Uuid::parse_str(tid).ok().map(|_| tid)
}

fn truthy(v: &serde_json::Value) -> Option<bool> {
    match v {
        serde_json::Value::Bool(b) => Some(*b),
        serde_json::Value::String(s) => match s.as_str() {
            "1" | "true" => Some(true),
            "0" | "false" => Some(false),
            _ => None,
        },
        serde_json::Value::Number(n) => n.as_i64().map(|n| n != 0),
        _ => None,
    }
}

/// Check `token` against `keys`, for the app `client_id` in the tenant whose
/// v2 issuer is `issuer`. Accepts v2 tokens and, when the app hasn't been
/// switched to v2, v1 tokens (`sts.windows.net` issuer, `api://` audience).
pub fn verify_with(
    token: &str,
    keys: &JwkSet,
    issuer: &str,
    client_id: &str,
) -> Result<EntraUser, Refusal> {
    let tenant = tenant_of(issuer).ok_or("issuer isn't an Entra tenant")?;
    let header = jsonwebtoken::decode_header(token).map_err(|_| "malformed")?;
    if header.alg != Algorithm::RS256 {
        return Err("unexpected algorithm");
    }
    let kid = header.kid.ok_or("no key id")?;
    let jwk = keys.find(&kid).ok_or("unknown key")?;
    let key = DecodingKey::from_jwk(jwk).map_err(|_| "unusable key")?;

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[client_id.to_string(), format!("api://{client_id}")]);
    validation.set_issuer(&[
        issuer.to_string(),
        format!("https://sts.windows.net/{tenant}/"),
    ]);
    validation.set_required_spec_claims(&["exp", "iss", "aud"]);
    validation.validate_nbf = true;
    validation.leeway = 300;
    let claims = jsonwebtoken::decode::<Claims>(token, &key, &validation)
        .map_err(|e| match e.kind() {
            jsonwebtoken::errors::ErrorKind::ExpiredSignature => "expired",
            jsonwebtoken::errors::ErrorKind::InvalidAudience => "another app's token",
            jsonwebtoken::errors::ErrorKind::InvalidIssuer => "another tenant's token",
            jsonwebtoken::errors::ErrorKind::InvalidSignature => "bad signature",
            _ => "invalid",
        })?
        .claims;

    if !claims.tid.eq_ignore_ascii_case(tenant) {
        return Err("another tenant's token");
    }
    if claims.idtyp.as_deref().is_some_and(|t| t != "user") {
        return Err("not a person");
    }
    if !claims
        .scp
        .as_deref()
        .is_some_and(|s| s.split(' ').any(|scope| scope == SCOPE))
    {
        return Err("missing scope");
    }
    let object_id = claims
        .oid
        .filter(|o| uuid::Uuid::parse_str(o).is_ok())
        .ok_or("no object id")?;
    Ok(EntraUser {
        tenant_id: tenant.to_lowercase(),
        object_id: object_id.to_lowercase(),
        // Without the optional `email` claim, the UPN, as requester SSO does:
        // its domain is one the tenant verified.
        email_verified: claims
            .email
            .as_ref()
            .and(claims.xms_edov.as_ref())
            .and_then(truthy),
        email: claims
            .email
            .or(claims.preferred_username)
            .or(claims.upn)
            .map(|e| e.trim().to_lowercase())
            .filter(|e| e.contains('@')),
        name: claims
            .name
            .map(|n| n.trim().chars().take(120).collect::<String>())
            .filter(|n| !n.is_empty()),
    })
}

/// Check `token` for the workspace provider at `issuer_url` (an Entra tenant),
/// fetching its keys (cached, refetched once for a key we haven't seen).
pub async fn verify(token: &str, issuer_url: &str, client_id: &str) -> Result<EntraUser, Refusal> {
    let load = |fresh| async move {
        let (issuer, keys) = crate::oidc::provider_keys(issuer_url, fresh)
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, "entra: couldn't load the tenant's keys");
                "keys unavailable"
            })?;
        let keys: JwkSet = serde_json::from_value(keys).map_err(|_| "keys unreadable")?;
        Ok::<_, Refusal>((issuer, keys))
    };
    let (issuer, keys) = load(false).await?;
    match verify_with(token, &keys, &issuer, client_id) {
        Err("unknown key") => {
            let (issuer, keys) = load(true).await?;
            verify_with(token, &keys, &issuer, client_id)
        }
        other => other,
    }
}

/// A fake Entra tenant for tests: its keys and tokens it signs.
#[cfg(test)]
pub(crate) mod test_support {
    use jsonwebtoken::{Algorithm, EncodingKey, Header};
    use serde_json::json;

    // A test-only key: `openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048`.
    const KEY_PEM: &str = include_str!("entra_token_test_key.pem");

    const N: &str = "nV5NyHAjEs5UNGwBsvAj-UdvIRILygB8oh30-rMdNRtaHlvS8d9TUsUW-ZpZgxvY3QaecaJsul95uVRofQEs4VlmOaSmo6DdrennQVdLWa6YYRY29Fu8oRy7rGeZXrsJ7urx5oV3wu8LqWv8Q7rU_8QmGVzGoj_zl0GMcBKoxJfIRQKfIVrkduMuFqa_UYz2-GuObwFpX2GtZEZKbaK2llLRufg0tAR7A7JPiL3CTYRnfyhv_n_HXQ2kjsHqM17l0b-og2wmkzjjWrHLvSDHx6jEqab_ELDcffdmZMTJkzEMgbVA1cz_0pFkv5UlvD7zVtwXHKW2brSIJL1cY3udlw";
    pub const TID: &str = "11111111-2222-3333-4444-555555555555";
    pub const OID: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    pub const CLIENT: &str = "99999999-8888-7777-6666-555555555555";

    pub fn issuer() -> String {
        format!("https://login.microsoftonline.com/{TID}/v2.0")
    }

    pub fn jwks() -> serde_json::Value {
        json!({ "keys": [
            { "kty": "RSA", "use": "sig", "kid": "k1", "n": N, "e": "AQAB" }
        ]})
    }

    pub fn claims() -> serde_json::Value {
        let now = chrono::Utc::now().timestamp();
        json!({
            "iss": issuer(), "aud": CLIENT, "tid": TID, "oid": OID,
            "scp": "access_as_user", "email": "Sam@Acme.test", "xms_edov": true,
            "name": "Sam", "iat": now, "nbf": now, "exp": now + 3600,
        })
    }

    pub fn sign(claims: &serde_json::Value, kid: &str) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(kid.into());
        jsonwebtoken::encode(
            &header,
            claims,
            &EncodingKey::from_rsa_pem(KEY_PEM.as_bytes()).unwrap(),
        )
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use serde_json::json;

    fn keys() -> JwkSet {
        serde_json::from_value(jwks()).unwrap()
    }

    fn with(changes: serde_json::Value) -> serde_json::Value {
        let mut c = claims();
        for (k, v) in changes.as_object().unwrap() {
            if v.is_null() {
                c.as_object_mut().unwrap().remove(k);
            } else {
                c[k] = v.clone();
            }
        }
        c
    }

    fn check(c: &serde_json::Value) -> Result<EntraUser, Refusal> {
        verify_with(&sign(c, "k1"), &keys(), &issuer(), CLIENT)
    }

    #[test]
    fn a_token_for_our_scope_from_the_tenant_names_the_person() {
        let user = check(&claims()).unwrap();
        assert_eq!(user.tenant_id, TID);
        assert_eq!(user.object_id, OID);
        assert_eq!(user.email.as_deref(), Some("sam@acme.test"));
        assert_eq!(user.email_verified, Some(true));
        // A v1 token (the app wasn't switched to v2) passes too.
        let v1 = with(json!({
            "iss": format!("https://sts.windows.net/{TID}/"),
            "aud": format!("api://{CLIENT}"),
        }));
        assert_eq!(check(&v1).unwrap().object_id, OID);
    }

    #[test]
    fn anything_else_is_refused() {
        let other_tenant = "99999999-2222-3333-4444-555555555555";
        let cases = [
            (with(json!({ "aud": "another-app" })), "another app's token"),
            (
                with(
                    json!({ "iss": format!("https://login.microsoftonline.com/{other_tenant}/v2.0") }),
                ),
                "another tenant's token",
            ),
            (
                with(json!({ "tid": other_tenant })),
                "another tenant's token",
            ),
            (with(json!({ "scp": "User.Read" })), "missing scope"),
            (
                with(json!({ "scp": null, "roles": ["x"] })),
                "missing scope",
            ),
            (with(json!({ "idtyp": "app" })), "not a person"),
            (with(json!({ "oid": null })), "no object id"),
            (
                with(json!({ "exp": chrono::Utc::now().timestamp() - 600 })),
                "expired",
            ),
        ];
        for (c, why) in cases {
            assert_eq!(check(&c), Err(why), "{c}");
        }
        assert_eq!(
            verify_with(&sign(&claims(), "k2"), &keys(), &issuer(), CLIENT),
            Err("unknown key")
        );
        let tampered = {
            let t = sign(&claims(), "k1");
            let mut parts: Vec<&str> = t.split('.').collect();
            let body = base64::Engine::encode(
                &base64::engine::general_purpose::URL_SAFE_NO_PAD,
                serde_json::to_vec(&with(
                    json!({ "oid": "bbbbbbbb-bbbb-cccc-dddd-eeeeeeeeeeee" }),
                ))
                .unwrap(),
            );
            parts[1] = &body;
            parts.join(".")
        };
        assert_eq!(
            verify_with(&tampered, &keys(), &issuer(), CLIENT),
            Err("bad signature")
        );
    }

    #[test]
    fn an_unverified_domain_owner_is_reported() {
        let user = check(&with(json!({ "xms_edov": "0" }))).unwrap();
        assert_eq!(user.email_verified, Some(false));
        assert_eq!(
            check(&with(json!({ "xms_edov": null })))
                .unwrap()
                .email_verified,
            None
        );
    }

    #[test]
    fn without_an_email_claim_the_upn_stands_in() {
        let user = check(&with(json!({
            "email": null, "xms_edov": null, "preferred_username": "Sam@Acme.test"
        })))
        .unwrap();
        assert_eq!(user.email.as_deref(), Some("sam@acme.test"));
        assert_eq!(user.email_verified, None);
    }

    #[test]
    fn only_a_guid_tenant_issuer_counts() {
        assert_eq!(tenant_of(&issuer()), Some(TID));
        assert_eq!(
            tenant_of("https://login.microsoftonline.com/common/v2.0"),
            None
        );
        assert_eq!(tenant_of("https://accounts.google.com"), None);
    }
}
