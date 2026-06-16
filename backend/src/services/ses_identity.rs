//! Optional Amazon SES sending-identity management for hosted deployments.
//!
//! When a workspace sets a verified sending domain, SES has to be told to
//! authorise that From domain or it rejects the send in production. We register
//! the domain as a BYODKIM identity using the workspace's own DKIM key, so the
//! single DNS record the admin publishes (Nosdesk's key) doubles as the SES
//! authorisation and stays valid if the relay is ever swapped.
//!
//! Gated on `NOSDESK_SES_REGION`. Self-host leaves it unset and every call here
//! is a no-op, so the verified-domain flow degrades to "publish the record, we
//! DNS-verify it" with no SES round-trip. When the region is set, the SES API
//! credentials (`NOSDESK_SES_ACCESS_KEY_ID` / `NOSDESK_SES_SECRET_ACCESS_KEY`)
//! are an IAM principal allowed `ses:CreateEmailIdentity`, `ses:DeleteEmailIdentity`
//! and `ses:PutEmailIdentityDkimSigningAttributes`. These are distinct from the
//! SES SMTP credentials, which can't call the API.

use aws_sdk_sesv2::operation::create_email_identity::CreateEmailIdentityError;
use aws_sdk_sesv2::operation::delete_email_identity::DeleteEmailIdentityError;
use aws_sdk_sesv2::types::{DkimSigningAttributes, DkimSigningAttributesOrigin};
use aws_sdk_sesv2::Client;

use crate::repository::workspace_email_settings as ws_settings;

#[derive(Debug)]
pub enum SesError {
    /// Region is set but the API credentials are missing or malformed.
    Config(String),
    /// The SES API call failed.
    Api(String),
    /// The stored DKIM key could not be converted to the format SES wants.
    Key(String),
}

impl std::fmt::Display for SesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SesError::Config(m) => write!(f, "SES config: {m}"),
            SesError::Api(m) => write!(f, "SES API: {m}"),
            SesError::Key(m) => write!(f, "DKIM key: {m}"),
        }
    }
}

impl std::error::Error for SesError {}

pub struct SesIdentityManager {
    client: Client,
}

impl SesIdentityManager {
    /// Build from env when `NOSDESK_SES_REGION` is set, else `None` (self-host
    /// or no SES identity management). Mirrors the explicit-credentials build in
    /// `utils::storage::S3Storage` rather than the default credential chain.
    pub fn from_env() -> Result<Option<Self>, SesError> {
        let region = match std::env::var("NOSDESK_SES_REGION") {
            Ok(r) if !r.trim().is_empty() => r,
            _ => return Ok(None),
        };
        let access_key = std::env::var("NOSDESK_SES_ACCESS_KEY_ID").map_err(|_| {
            SesError::Config("NOSDESK_SES_REGION set but NOSDESK_SES_ACCESS_KEY_ID missing".into())
        })?;
        let secret_key = std::env::var("NOSDESK_SES_SECRET_ACCESS_KEY").map_err(|_| {
            SesError::Config(
                "NOSDESK_SES_REGION set but NOSDESK_SES_SECRET_ACCESS_KEY missing".into(),
            )
        })?;

        use aws_sdk_sesv2::config::{BehaviorVersion, Credentials, Region};
        let creds = Credentials::new(access_key, secret_key, None, None, "nosdesk-ses");
        let conf = aws_sdk_sesv2::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new(region))
            .credentials_provider(creds)
            .build();
        Ok(Some(Self {
            client: Client::from_conf(conf),
        }))
    }

    /// Register `domain` as a BYODKIM sending identity signed by the workspace's
    /// key (`private_pem`, the stored PKCS#1 PEM). Idempotent: an already-registered
    /// identity has its signing key updated, so a key rotation re-registers cleanly.
    pub async fn register_sending_domain(
        &self,
        domain: &str,
        selector: &str,
        private_pem: &str,
    ) -> Result<(), SesError> {
        let key_b64 = ws_settings::dkim_private_pkcs8_b64(private_pem)
            .map_err(|e| SesError::Key(e.to_string()))?;
        let signing = DkimSigningAttributes::builder()
            .domain_signing_selector(selector)
            .domain_signing_private_key(key_b64)
            .build();

        let created = self
            .client
            .create_email_identity()
            .email_identity(domain)
            .dkim_signing_attributes(signing.clone())
            .send()
            .await;

        match created {
            Ok(_) => Ok(()),
            // Already registered (re-config / rotation): swap the signing key.
            Err(aws_sdk_sesv2::error::SdkError::ServiceError(se))
                if matches!(
                    se.err(),
                    CreateEmailIdentityError::AlreadyExistsException(_)
                ) =>
            {
                self.client
                    .put_email_identity_dkim_signing_attributes()
                    .email_identity(domain)
                    .signing_attributes_origin(DkimSigningAttributesOrigin::External)
                    .signing_attributes(signing)
                    .send()
                    .await
                    .map_err(|e| SesError::Api(format!("update DKIM signing for {domain}: {e}")))?;
                Ok(())
            }
            Err(e) => Err(SesError::Api(format!("create identity {domain}: {e}"))),
        }
    }

    /// Remove the SES identity for `domain`. A missing identity is treated as
    /// success, so a reset after a failed registration still succeeds.
    pub async fn deregister_sending_domain(&self, domain: &str) -> Result<(), SesError> {
        match self
            .client
            .delete_email_identity()
            .email_identity(domain)
            .send()
            .await
        {
            Ok(_) => Ok(()),
            Err(aws_sdk_sesv2::error::SdkError::ServiceError(se))
                if matches!(se.err(), DeleteEmailIdentityError::NotFoundException(_)) =>
            {
                Ok(())
            }
            Err(e) => Err(SesError::Api(format!("delete identity {domain}: {e}"))),
        }
    }
}
