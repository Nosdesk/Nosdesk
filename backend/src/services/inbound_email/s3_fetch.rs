//! Fetching raw inbound MIME from the SES inbound S3 bucket.
//!
//! The SES receipt rule writes each received message's raw bytes to S3 (the
//! SNS notification only carries metadata + the object key). This client reads
//! that object. It's built from the SES IAM credentials (the same identity
//! that manages sending domains, extended with `s3:GetObject` on the inbound
//! bucket) and is gated on `NOSDESK_INBOUND_S3_BUCKET`, so self-host (which
//! keeps IMAP polling) leaves it unconfigured.

use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::Client;

pub struct InboundS3 {
    client: Client,
    bucket: String,
}

impl InboundS3 {
    /// Build from env, or `None` when not configured (self-host). Requires
    /// `NOSDESK_INBOUND_S3_BUCKET` plus the `NOSDESK_SES_*` credentials; a
    /// bucket set without those credentials is a misconfiguration, not a
    /// silent disable, so it returns `Err`.
    pub fn from_env() -> Result<Option<Self>, String> {
        let bucket = match std::env::var("NOSDESK_INBOUND_S3_BUCKET") {
            Ok(b) if !b.is_empty() => b,
            _ => return Ok(None),
        };
        let region = std::env::var("NOSDESK_SES_REGION").map_err(|_| {
            "NOSDESK_INBOUND_S3_BUCKET set but NOSDESK_SES_REGION missing".to_string()
        })?;
        let access_key = std::env::var("NOSDESK_SES_ACCESS_KEY_ID").map_err(|_| {
            "NOSDESK_INBOUND_S3_BUCKET set but NOSDESK_SES_ACCESS_KEY_ID missing".to_string()
        })?;
        let secret_key = std::env::var("NOSDESK_SES_SECRET_ACCESS_KEY").map_err(|_| {
            "NOSDESK_INBOUND_S3_BUCKET set but NOSDESK_SES_SECRET_ACCESS_KEY missing".to_string()
        })?;

        let creds = Credentials::new(access_key, secret_key, None, None, "nosdesk-inbound-s3");
        let conf = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new(region))
            .credentials_provider(creds)
            .build();
        Ok(Some(Self {
            client: Client::from_conf(conf),
            bucket,
        }))
    }

    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    /// Fetch an object by key. We always read our own configured bucket, never
    /// a bucket named in the (authenticated, but still externally-shaped)
    /// notification, as defence in depth.
    pub async fn get(&self, key: &str) -> Result<Vec<u8>, String> {
        let out = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| format!("s3 get_object {key}: {e}"))?;
        let body = out
            .body
            .collect()
            .await
            .map_err(|e| format!("s3 read body {key}: {e}"))?;
        Ok(body.into_bytes().to_vec())
    }
}
