//! AWS cloud-based URI resolvers (S3, Secrets Manager, SSM Parameter Store).
//!
//! This module is only compiled when the `tls` feature is enabled.

use crate::uri_resolver::SettingsInput;
use crate::uri_resolver::{ResolverResult, UriResolver, MAX_SIZE_BYTES};
use async_trait::async_trait;
use snafu::{ensure, OptionExt, ResultExt, Snafu};
use std::convert::TryFrom;

use aws_config;
use aws_sdk_s3;
use aws_sdk_secretsmanager;
use aws_sdk_ssm;

struct Arn {
    service: String,
    region: String,
    parts: i8,
}

impl Arn {
    fn parse(input: &str) -> ResolverResult<Self> {
        use crate::uri_resolver::resolver_error::InvalidArnFormatSnafu;
        ensure!(
            input.starts_with("arn:aws:"),
            InvalidArnFormatSnafu {
                input_source: input.to_string(),
                reason: "must start with 'arn:aws:'".to_string(),
            }
        );
        let parts: Vec<&str> = input.split(':').collect();
        ensure!(
            parts.len() > 4,
            InvalidArnFormatSnafu {
                input_source: input.to_string(),
                reason: format!("expected at least 4 ':' separators, found {}", parts.len()),
            }
        );
        Ok(Arn {
            service: parts[2].to_string(),
            region: parts[3].to_string(),
            parts: parts.len() as i8,
        })
    }
}

pub struct S3Uri {
    pub bucket: String,
    pub key: String,
}

impl TryFrom<&SettingsInput> for S3Uri {
    type Error = crate::uri_resolver::ResolverError;
    fn try_from(input: &SettingsInput) -> ResolverResult<Self> {
        use cloud_error::*;
        const PREFIX: &str = "s3://";
        let uri_str = input.input.as_str();
        let remainder = uri_str.strip_prefix(PREFIX).context(S3UriSchemeSnafu {
            input_source: input.input.clone(),
        })?;
        let mut parts = remainder.splitn(2, '/');
        let bucket = parts.next().context(S3UriMissingBucketSnafu {
            input_source: input.input.clone(),
        })?;
        let key = parts.next().context(S3UriMissingKeySnafu {
            input_source: input.input.clone(),
        })?;
        Ok(S3Uri {
            bucket: bucket.to_string(),
            key: key.to_string(),
        })
    }
}

#[async_trait]
impl UriResolver for S3Uri {
    async fn resolve(&self) -> ResolverResult<String> {
        use cloud_error::*;
        let cfg = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let client = aws_sdk_s3::Client::new(&cfg);

        let head_resp = client
            .head_object()
            .bucket(&self.bucket)
            .key(&self.key)
            .send()
            .await
            .context(S3HeadSnafu {
                bucket: self.bucket.clone(),
                key: self.key.clone(),
            })?;

        let size = head_resp.content_length.context(S3MissingContentLengthSnafu {
            bucket: self.bucket.clone(),
            key: self.key.clone(),
        })? as u64;

        ensure!(
            size < MAX_SIZE_BYTES,
            S3ObjectTooLargeSnafu {
                size,
                max_size: MAX_SIZE_BYTES,
                bucket: self.bucket.clone(),
                key: self.key.clone(),
            }
        );

        let resp = client
            .get_object()
            .bucket(&self.bucket)
            .key(&self.key)
            .send()
            .await
            .context(S3GetSnafu {
                bucket: self.bucket.clone(),
                key: self.key.clone(),
            })?;

        let bytes = resp.body.collect().await.context(S3BodySnafu {
            bucket: self.bucket.clone(),
            key: self.key.clone(),
        })?;

        String::from_utf8(bytes.to_vec()).context(crate::uri_resolver::resolver_error::Utf8DecodeSnafu {
            uri: format!("s3://{}/{}", self.bucket, self.key),
        })
    }
}

pub struct SecretsManagerArn {
    pub region: String,
    pub full_arn: String,
}

impl TryFrom<&SettingsInput> for SecretsManagerArn {
    type Error = crate::uri_resolver::ResolverError;
    fn try_from(input: &SettingsInput) -> ResolverResult<Self> {
        use crate::uri_resolver::resolver_error::InvalidArnFormatSnafu;
        use cloud_error::SecretsManagerArnSnafu;
        let arn = Arn::parse(input.input.as_str())?;
        ensure!(
            arn.parts == 7,
            InvalidArnFormatSnafu {
                input_source: input.input.clone(),
                reason: format!("expected 6 ':' separators (7 parts), found {}", arn.parts),
            }
        );
        ensure!(
            arn.service == "secretsmanager",
            SecretsManagerArnSnafu {
                input_source: input.input.clone()
            }
        );
        Ok(SecretsManagerArn {
            region: arn.region,
            full_arn: input.input.to_string(),
        })
    }
}

#[async_trait]
impl UriResolver for SecretsManagerArn {
    async fn resolve(&self) -> ResolverResult<String> {
        use cloud_error::*;
        let cfg = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_sdk_secretsmanager::config::Region::new(self.region.clone()))
            .load()
            .await;
        let client = aws_sdk_secretsmanager::Client::new(&cfg);
        let resp = client
            .get_secret_value()
            .secret_id(self.full_arn.clone())
            .send()
            .await
            .context(SecretsManagerGetSnafu {
                secret_id: self.full_arn.clone(),
            })?;
        Ok(resp
            .secret_string()
            .map(str::to_string)
            .context(SecretsManagerStringMissingSnafu {
                secret_id: self.full_arn.clone(),
            })?)
    }
}

/// Resolves `secretsmanager://` URIs to fetch secrets from AWS Secrets Manager.
///
/// Uses the default AWS region from environment (`AWS_REGION`) or instance metadata.
/// For cross-region access, use the full ARN instead: `arn:aws:secretsmanager:REGION:...`
pub struct SecretsManagerUri {
    pub secret_id: String,
}

impl TryFrom<&SettingsInput> for SecretsManagerUri {
    type Error = crate::uri_resolver::ResolverError;
    fn try_from(input: &SettingsInput) -> ResolverResult<Self> {
        use cloud_error::*;
        const PREFIX: &str = "secretsmanager://";
        let uri_str = input.input.as_str();
        let remainder = uri_str.strip_prefix(PREFIX).context(SecretsManagerUriSnafu {
            input_source: input.input.clone(),
        })?;
        ensure!(
            !remainder.is_empty(),
            SecretsManagerUriSnafu {
                input_source: input.input.clone()
            }
        );
        Ok(SecretsManagerUri {
            secret_id: remainder.to_string(),
        })
    }
}

#[async_trait]
impl UriResolver for SecretsManagerUri {
    async fn resolve(&self) -> ResolverResult<String> {
        use cloud_error::*;
        let cfg = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let client = aws_sdk_secretsmanager::Client::new(&cfg);
        let resp = client
            .get_secret_value()
            .secret_id(self.secret_id.clone())
            .send()
            .await
            .context(SecretsManagerGetSnafu {
                secret_id: self.secret_id.clone(),
            })?;
        Ok(resp
            .secret_string()
            .map(str::to_string)
            .context(SecretsManagerStringMissingSnafu {
                secret_id: self.secret_id.clone(),
            })?)
    }
}

pub struct SsmArn {
    pub region: String,
    pub full_arn: String,
}

impl TryFrom<&SettingsInput> for SsmArn {
    type Error = crate::uri_resolver::ResolverError;
    fn try_from(input: &SettingsInput) -> ResolverResult<Self> {
        use crate::uri_resolver::resolver_error::InvalidArnFormatSnafu;
        use cloud_error::SsmArnSnafu;
        let arn = Arn::parse(input.input.as_str())?;
        ensure!(
            arn.parts == 6,
            InvalidArnFormatSnafu {
                input_source: input.input.clone(),
                reason: format!("expected 5 ':' separators (6 parts), found {}", arn.parts),
            }
        );
        ensure!(
            arn.service == "ssm",
            SsmArnSnafu {
                input_source: input.input.clone()
            }
        );
        Ok(SsmArn {
            region: arn.region,
            full_arn: input.input.to_string(),
        })
    }
}

#[async_trait]
impl UriResolver for SsmArn {
    async fn resolve(&self) -> ResolverResult<String> {
        use cloud_error::*;
        let cfg = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_sdk_ssm::config::Region::new(self.region.clone()))
            .load()
            .await;
        let client = aws_sdk_ssm::Client::new(&cfg);
        let resp = client
            .get_parameter()
            .name(self.full_arn.clone())
            .with_decryption(true)
            .send()
            .await
            .context(SsmGetParameterSnafu {
                parameter_name: self.full_arn.clone(),
            })?;
        let value = resp
            .parameter
            .and_then(|p| p.value().map(|v| v.to_string()))
            .context(SsmParameterMissingSnafu {
                parameter_name: self.full_arn.clone(),
            })?;
        Ok(value)
    }
}

/// Resolves `ssm://` URIs to fetch parameters from AWS Systems Manager Parameter Store.
///
/// Uses the default AWS region from environment (`AWS_REGION`) or instance metadata.
/// For cross-region access, use the full ARN instead: `arn:aws:ssm:REGION:...`
pub struct SsmUri {
    pub parameter_name: String,
}

impl TryFrom<&SettingsInput> for SsmUri {
    type Error = crate::uri_resolver::ResolverError;
    fn try_from(input: &SettingsInput) -> ResolverResult<Self> {
        use cloud_error::*;
        const PREFIX: &str = "ssm://";
        let uri_str = input.input.as_str();
        let remainder = uri_str.strip_prefix(PREFIX).context(SsmUriSnafu {
            input_source: input.input.clone(),
        })?;
        ensure!(
            !remainder.is_empty(),
            SsmUriSnafu {
                input_source: input.input.clone()
            }
        );
        Ok(SsmUri {
            parameter_name: remainder.to_string(),
        })
    }
}

#[async_trait]
impl UriResolver for SsmUri {
    async fn resolve(&self) -> ResolverResult<String> {
        use cloud_error::*;
        let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let client = aws_sdk_ssm::Client::new(&config);
        let resp = client
            .get_parameter()
            .name(self.parameter_name.clone())
            .with_decryption(true)
            .send()
            .await
            .context(SsmGetParameterSnafu {
                parameter_name: self.parameter_name.clone(),
            })?;
        let value = resp
            .parameter
            .and_then(|p| p.value().map(|v| v.to_string()))
            .context(SsmParameterMissingSnafu {
                parameter_name: self.parameter_name.clone(),
            })?;
        Ok(value)
    }
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum CloudError {
    #[snafu(display("Failed to HEAD S3 object s3://{bucket}/{key}: {source}"))]
    S3Head {
        source: aws_sdk_s3::error::SdkError<
            aws_sdk_s3::operation::head_object::HeadObjectError,
            aws_sdk_s3::config::http::HttpResponse,
        >,
        bucket: String,
        key: String,
    },

    #[snafu(display("Failed to fetch S3 object '{bucket}/{key}': {}", source))]
    S3Get {
        bucket: String,
        key: String,
        source: aws_sdk_s3::error::SdkError<
            aws_sdk_s3::operation::get_object::GetObjectError,
            aws_sdk_s3::config::http::HttpResponse,
        >,
    },

    #[snafu(display("Failed to read S3 object body '{bucket}/{key}': {}", source))]
    S3Body {
        bucket: String,
        key: String,
        source: aws_sdk_s3::primitives::ByteStreamError,
    },

    #[snafu(display("Failed to fetch secret '{}' from Secrets Manager: {}", secret_id, source))]
    SecretsManagerGet {
        secret_id: String,
        source: aws_sdk_secretsmanager::error::SdkError<
            aws_sdk_secretsmanager::operation::get_secret_value::GetSecretValueError,
        >,
    },

    #[snafu(display("Failed to fetch parameter '{}' from SSM: {}", parameter_name, source))]
    SsmGetParameter {
        parameter_name: String,
        source: aws_sdk_ssm::error::SdkError<aws_sdk_ssm::operation::get_parameter::GetParameterError>,
    },

    #[snafu(display("S3 object s3://{bucket}/{key} is too large ({size} bytes, maximum is {max_size} bytes)"))]
    S3ObjectTooLarge { size: u64, max_size: u64, bucket: String, key: String },

    #[snafu(display("Invalid S3 URI scheme for '{}', expected s3://", input_source))]
    S3UriScheme { input_source: String },

    #[snafu(display("Invalid S3 URI '{}': missing bucket name", input_source))]
    S3UriMissingBucket { input_source: String },

    #[snafu(display("Invalid S3 URI '{}': missing key name", input_source))]
    S3UriMissingKey { input_source: String },

    #[snafu(display("No Content-Length for S3 object {bucket}/{key}"))]
    S3MissingContentLength { bucket: String, key: String },

    #[snafu(display("Invalid Secrets Manager URI scheme for '{}', expected secretsmanager://", input_source))]
    SecretsManagerUri { input_source: String },

    #[snafu(display("Secrets Manager secret '{}' did not return a string value", secret_id))]
    SecretsManagerStringMissing { secret_id: String },

    #[snafu(display("Invalid Secrets Manager ARN scheme for '{}', expected arn:aws:secretsmanager:…", input_source))]
    SecretsManagerArn { input_source: String },

    #[snafu(display("Invalid SSM ARN scheme for '{}', expected arn:aws:ssm:…", input_source))]
    SsmArn { input_source: String },

    #[snafu(display("SSM ARN parameter '{}' did not return a string value", parameter_name))]
    SsmArnValueMissing { parameter_name: String },

    #[snafu(display("Invalid SSM URI scheme for '{}', expected ssm://", input_source))]
    SsmUri { input_source: String },

    #[snafu(display("SSM parameter '{}' did not return a string value", parameter_name))]
    SsmParameterMissing { parameter_name: String },
}
