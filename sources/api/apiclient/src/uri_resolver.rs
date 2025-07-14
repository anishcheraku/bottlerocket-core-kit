//! Defines `UriResolver`, an async trait for fetching UTF-8 text from various URI schemes.
//! Concrete types parse and resolve a single scheme via `TryFrom` + `resolve()`:  
//!  - `StdinUri` for "-", `FileUri` for `file://`, `HttpUri` for `http(s)://`  
//!  - `S3Uri` for `s3://`, `SecretsManagerUri` for `secretsmanager://`, `SsmUri` for `ssm://`  
//! To add a new scheme, implement its `TryFrom` and `UriResolver::resolve()`.  

use async_trait::async_trait;
use aws_sdk_ssm as ssm; 
use aws_config;
use snafu::{ensure, ResultExt, Snafu, OptionExt};
use std::convert::TryFrom;
use std::path::PathBuf;
use tokio::io::AsyncReadExt;
use reqwest::Url;
pub type ResolverResult<T> = std::result::Result<T, ResolverError>;
use crate::apply::SettingsInput;

/// Anything that can fetch itself as a UTF-8 `String`.
#[async_trait]
pub trait UriResolver {
    /// Fetches the contents of this URI as a `String`.
    async fn resolve(&self) -> ResolverResult<String>;
}

/// Uri Resolver that reads from standard input.
///
/// This resolver accepts exactly "-" as its URI and will read all of stdin
/// into a single UTF-8 string.
pub struct StdinUri;

impl TryFrom<&SettingsInput> for StdinUri {
    type Error = ();
    fn try_from(input: &SettingsInput) -> std::result::Result<Self, Self::Error> {
        if input.input == "-" {
            Ok(StdinUri)
        } else {
            Err(())
        }
    }
}

#[async_trait]
impl UriResolver for StdinUri {
    async fn resolve(&self) -> ResolverResult<String> {
        use resolver_error::*;
        let mut buf = String::new();
        tokio::io::stdin()
            .read_to_string(&mut buf)
            .await
            .context(StdinReadSnafu)?;
        Ok(buf)
    }
}


/// Uri Resolver that reads from a local file.
///
/// This resolver accepts URIs of the form `file:///path/to/file` (or on Windows
/// `file://C:/path/to/file`), converts them to a `PathBuf`, and returns the
/// file’s entire contents as a UTF-8 string.
pub struct FileUri {
    path: PathBuf,
}

impl TryFrom<&SettingsInput> for FileUri {
    type Error = ResolverError;
    fn try_from(input: &SettingsInput) -> ResolverResult<Self> {
        let url = input
            .parsed_url
            .clone()
            .context(FileUriSnafu { input_source: input.input.clone() })?;
        use resolver_error::*;

        // only accept file://
        ensure!(
            url.scheme() == "file",
            FileUriSnafu { input_source: url.to_string() }
        );

        // convert to PathBuf or error
        let path = url
            .to_file_path()
            .ok()
            .context(FileUriSnafu { input_source: url.to_string() })?;

        Ok(FileUri { path })
    }
}

#[async_trait]
impl UriResolver for FileUri {
    async fn resolve(&self) -> ResolverResult<String> {
        use resolver_error::*;
        tokio::fs::read_to_string(&self.path)
            .await
            .context(FileReadSnafu {
                input_source: self.path.to_string_lossy().into_owned(),
            })
    }
}


/// Uri Resolver that fetches over HTTP(S).
///
/// This resolver accepts URIs beginning with `http://` or `https://` and
/// performs a GET request with `reqwest`, returning the response body as a
/// UTF-8 string (erroring on non-2xx status).
pub struct HttpUri {
    url: Url,
}

impl TryFrom<&SettingsInput> for HttpUri {
    type Error = ResolverError;
    fn try_from(input: &SettingsInput) -> ResolverResult<Self> {
        let url = input
            .parsed_url
            .clone()
            .context(InvalidHTTPUriSnafu { input_source: input.input.clone() })?;
        use resolver_error::*;
        ensure!(
            url.scheme() == "http" || url.scheme() == "https",
            InvalidHTTPUriSnafu { input_source: url.to_string() }
        );
        Ok(HttpUri { url })
    }
}

#[async_trait]
impl UriResolver for HttpUri {
    async fn resolve(&self) -> ResolverResult<String> {
        use resolver_error::*;
        // 1) issue the GET
        let resp = reqwest::get(self.url.clone())
            .await
            .context(HttpRequestSnafu {
                uri: self.url.to_string(),
            })?;

        // 2) check status
        let resp = if resp.status().is_success() {
            resp
        } else {
            return HttpStatusSnafu {
                uri: self.url.to_string(),
                status: resp.status(),
            }
            .fail();
        };

        // 3) read the body
        resp.text()
            .await
            .context(HttpBodySnafu {
                uri: self.url.to_string(),
            })
    }
}


/// Uri Resolver that fetches content from S3
///
/// This resolver accepts input of the form s3://bucket/key and translates this into
/// authenticated AWS S3 get requests using the standard AWS credential resolution mechanism
pub struct S3Uri {
    bucket: String,
    key: String,
}

impl TryFrom<&SettingsInput> for S3Uri {
    type Error = ResolverError;
    fn try_from(input: &SettingsInput) -> ResolverResult<Self> {
        let url = input
            .parsed_url
            .clone()
            .context(S3UriSchemeSnafu { input_source: input.input.clone() })?;
        use resolver_error::*;
        ensure!(
            url.scheme() == "s3",
            S3UriSchemeSnafu { input_source: url.to_string() }
        );
        let bucket = url
            .host_str()
            .context(S3UriMissingBucketSnafu { input_source: url.to_string() })?
            .to_string();
        let key = url.path().trim_start_matches('/').to_string();
        Ok(S3Uri { bucket, key })
    }
}

#[async_trait]
impl UriResolver for S3Uri {
    async fn resolve(&self) -> ResolverResult<String> {
        use resolver_error::*;
        let cfg = aws_config::load_from_env().await;
        let client = aws_sdk_s3::Client::new(&cfg);

        // 1) GET the object
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

        // 2) COLLECT the body stream
        let bytes = resp
            .body
            .collect()
            .await
            .context(S3BodySnafu {
                bucket: self.bucket.clone(),
                key: self.key.clone(),
            })?;

        // 3) UTF-8 decode
        Ok(String::from_utf8_lossy(&bytes.into_bytes()).into_owned())
    }
}


/// Uri Resolver that fetches secrets from AWS Secrets Manager.
///
/// This resolver accepts URIs of the form `secretsmanager://secret_id`, uses
/// the AWS SDK’s default credential and region resolution, and returns the
/// `SecretString` payload of the given secret.
pub struct SecretsManagerUri {
    secret_id: String,
}

impl TryFrom<&SettingsInput> for SecretsManagerUri {
    type Error = ResolverError;
    fn try_from(input: &SettingsInput) -> ResolverResult<Self> {
        use resolver_error::*;
        // must start with "secretsmanager://"
        ensure!(
            input.input.as_str().starts_with("secretsmanager://"),
            SecretsManagerUriSnafu { input_source: input.input.as_str().to_string() }
        );
        // strip the prefix and ensure there's actually an ID
        let id = &input.input.as_str()["secretsmanager://".len()..];
        ensure!(
            !id.is_empty(),
            SecretsManagerUriSnafu { input_source: input.input.as_str().to_string() }
        );
        Ok(SecretsManagerUri { secret_id: id.to_string() })
    }
}

#[async_trait]
impl UriResolver for SecretsManagerUri {
    async fn resolve(&self) -> ResolverResult<String> {
        use aws_config::{self};
        use aws_sdk_secretsmanager;
        use resolver_error::*;

        // 1) load AWS config (region/account via env)
        let cfg = aws_config::load_from_env().await;
        let client = aws_sdk_secretsmanager::Client::new(&cfg);

        // 2) fetch the secret, propagating any SdkError into SecretsManagerGet
        let resp = client
            .get_secret_value()
            .secret_id(self.secret_id.clone())
            .send()
            .await
            .context(SecretsManagerGetSnafu { secret_id: self.secret_id.clone() })?;

        // 3) extract the string payload, or error if it was missing
        resp.secret_string()
            .map(str::to_string)
            .context(SecretsManagerStringMissingSnafu { secret_id: self.secret_id.clone() })
    }
}


/// Uri Resolver that fetches parameters from AWS SSM Parameter Store.
///
/// This resolver accepts URIs of the form `ssm://parameter_name`, uses the
/// AWS SDK’s default credential and region resolution, and returns the value
/// of the requested parameter.
pub struct SsmUri {
    parameter_name: String,
}

impl TryFrom<&SettingsInput> for SsmUri {
    type Error = ResolverError;
    fn try_from(input: &SettingsInput) -> ResolverResult<Self> {
        use resolver_error::*;

        // must start with "ssm://"
        ensure!(
            input.input.as_str().starts_with("ssm://"),
            SsmUriSnafu { input_source: input.input.as_str().to_string() }
        );

        // strip the prefix and ensure there's actually a name
        let name = &input.input.as_str()["ssm://".len()..];
        ensure!(
            !name.is_empty(),
            SsmUriSnafu { input_source: input.input.as_str().to_string() }
        );

        Ok(SsmUri { parameter_name: name.to_string() })
    }
}

#[async_trait]
impl UriResolver for SsmUri {
    async fn resolve(&self) -> ResolverResult<String> {
        // use default region chain
        let config = aws_config::load_from_env().await;
        let client = ssm::Client::new(&config);
        use resolver_error::*;

        // fetch the parameter, with decryption
        let resp = client
            .get_parameter()
            .name(self.parameter_name.clone())
            .with_decryption(true)
            .send()
            .await
            .context(SsmGetParameterSnafu { parameter_name: self.parameter_name.clone() })?;

        // extract the string value
        let value = resp
            .parameter
            .and_then(|p| p.value().map(|v| v.to_string()))
            .context(SsmParameterMissingSnafu { parameter_name: self.parameter_name.clone() })?;

        Ok(value)
    }
}


#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum ResolverError {

    #[snafu(display("Failed to read standard input: {}", source))]
    StdinRead { source: std::io::Error },

    #[snafu(display("Given invalid file URI '{}'", input_source))]
    FileUri { input_source: String },

    #[snafu(display("Failed to read given file '{}': {}", input_source, source))]
    FileRead {
        input_source: String,
        source: std::io::Error,
    },

    #[snafu(display("Given HTTP(S) URI '{}'", input_source))]
    InvalidHTTPUri { input_source: String },

    #[snafu(display("Failed to perform HTTP GET to '{}': {}", uri, source))]
    HttpRequest { uri: String, source: reqwest::Error },

    #[snafu(display("Non-success HTTP status from '{}': {}", uri, status))]
    HttpStatus {
        uri: String,
        status: reqwest::StatusCode,
    },

    #[snafu(display("Failed to read HTTP response body from '{}': {}", uri, source))]
    HttpBody { uri: String, source: reqwest::Error },

    #[snafu(display("Invalid S3 URI scheme for '{}', expected s3://", input_source))]
    S3UriScheme { input_source: String },

    #[snafu(display("Invalid S3 URI '{}': missing bucket name", input_source))]
    S3UriMissingBucket { input_source: String },

    #[snafu(display("Failed to fetch S3 object '{bucket}/{key}': {}", source))]
    S3Get {
        bucket: String,
        key: String,
        source: aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::get_object::GetObjectError, aws_sdk_secretsmanager::config::http::HttpResponse>,
    },

    #[snafu(display("Failed to read S3 object body '{bucket}/{key}': {}", source))]
    S3Body {
        bucket: String,
        key: String,
        source: aws_sdk_s3::primitives::ByteStreamError,
    },

    #[snafu(display("Invalid Secrets Manager URI scheme for '{}', expected secretsmanager://", input_source))]
    SecretsManagerUri { input_source: String },

    #[snafu(display("Failed to fetch secret '{}' from Secrets Manager: {}", secret_id, source))]
    SecretsManagerGet {
        secret_id: String,
        source: aws_sdk_secretsmanager::error::SdkError<aws_sdk_secretsmanager::operation::get_secret_value::GetSecretValueError>,
    },

    #[snafu(display("Secrets Manager secret '{}' did not return a string value", secret_id))]
    SecretsManagerStringMissing { secret_id: String },

    #[snafu(display("Invalid SSM URI scheme for '{}', expected ssm://", input_source))]
    SsmUri { input_source: String },

    #[snafu(display("Failed to fetch parameter '{}' from SSM: {}", parameter_name, source))]
    SsmGetParameter {
        parameter_name: String,
        source: aws_sdk_ssm::error::SdkError<aws_sdk_ssm::operation::get_parameter::GetParameterError>,
    },

    #[snafu(display("SSM parameter '{}' did not return a string value", parameter_name))]
    SsmParameterMissing { parameter_name: String },

}
