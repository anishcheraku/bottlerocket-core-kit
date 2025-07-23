//! Defines `UriResolver`, an async trait for fetching UTF-8 text from various URI schemes.
//! Concrete types parse and resolve a single scheme via `TryFrom` + `resolve()`:
//!  - `StdinUri` for "-", `FileUri` for `file://`, `HttpUri` for `http(s)://`
//!  - `S3Uri` for `s3://`, `SecretsManagerUri` for `secretsmanager://`, `SsmUri` for `ssm://`
//!
//! To add a new scheme, implement its `TryFrom` and `UriResolver::resolve()`.  
use crate::apply::SettingsInput;
use async_trait::async_trait;
use aws_config;
use aws_sdk_ssm as ssm;
use reqwest::Url;
use snafu::{ensure, OptionExt, ResultExt, Snafu};
use std::any::Any;
use std::convert::TryFrom;
use std::path::PathBuf;
use tokio::io::AsyncReadExt;
const MAX_SIZE_BYTES: u64 = 100 * 1024 * 1024;
/// Maximum allowed object size for S3 and HTTP(S) resolvers (100 MiB).

/// Anything that can fetch itself as a UTF-8 `String`.
#[async_trait]
pub trait UriResolver: Any {
    /// Fetches the contents of this URI as a `String`.
    async fn resolve(&self) -> ResolverResult<String>;
}

// A minimal AWS ARN parser for our resolvers.
struct Arn {
    service: String,
    region: String,
    parts: i8,
}

impl Arn {
    /// Parse an ARN of the form:
    ///   arn:aws:<service>:<region>:<account>:<resource…>
    fn parse(input: &str) -> ResolverResult<Self> {
        use resolver_error::InvalidArnFormatSnafu;
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
        let service = parts[2];
        let region = parts[3];

        Ok(Arn {
            service: service.to_string(),
            region: region.to_string(),
            parts: parts.len() as i8,
        })
    }
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
        use resolver_error::*;

        let url = input.parsed_url.clone().context(FileUriSnafu {
            input_source: input.input.clone(),
        })?;

        ensure!(
            url.scheme() == "file",
            FileUriSnafu {
                input_source: url.to_string()
            }
        );

        let path = url.to_file_path().ok().context(FileUriSnafu {
            input_source: url.to_string(),
        })?;

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
        use resolver_error::*;
        let url = input.parsed_url.clone().context(InvalidHTTPUriSnafu {
            input_source: input.input.clone(),
        })?;
        ensure!(
            url.scheme() == "http" || url.scheme() == "https",
            InvalidHTTPUriSnafu {
                input_source: url.to_string()
            }
        );
        Ok(HttpUri { url })
    }
}

#[async_trait]
impl UriResolver for HttpUri {
    async fn resolve(&self) -> ResolverResult<String> {
        use resolver_error::*;
        let resp = reqwest::get(self.url.clone())
            .await
            .context(HttpRequestSnafu {
                uri: self.url.to_string(),
            })?;

        ensure!(
            resp.status().is_success(),
            HttpStatusSnafu {
                uri: self.url.to_string(),
                status: resp.status(),
            }
        );

        // check content length if available
        if let Some(content_length) = resp.content_length() {
            ensure!(
                content_length < MAX_SIZE_BYTES,
                HttpObjectTooLargeSnafu {
                    size: content_length,
                    max_size: MAX_SIZE_BYTES,
                    uri: self.url.to_string(),
                }
            );
        }

        // read the body as bytes first to check size
        let bytes = resp.bytes().await.context(HttpBodySnafu {
            uri: self.url.to_string(),
        })?;
        ensure!(
            bytes.len() as u64 <= MAX_SIZE_BYTES,
            HttpObjectTooLargeSnafu {
                size: bytes.len() as u64,
                max_size: MAX_SIZE_BYTES,
                uri: self.url.to_string(),
            }
        );

        String::from_utf8(bytes.to_vec()).context(Utf8DecodeSnafu {
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
        use resolver_error::*;
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
        use resolver_error::*;
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

        if let Some(size) = head_resp.content_length {
            ensure!(
                (size as u64) < MAX_SIZE_BYTES,
                resolver_error::S3ObjectTooLargeSnafu {
                    size: size as u64,
                    max_size: MAX_SIZE_BYTES,
                    bucket: self.bucket.clone(),
                    key: self.key.clone(),
                }
            );
        }

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

        String::from_utf8(bytes.to_vec()).context(Utf8DecodeSnafu {
            uri: format!("s3://{}/{}", self.bucket, self.key),
        })
    }
}

/// Uri Resolver that fetches secrets from AWS Secrets Manager by ARN.
///
/// This resolver accepts full AWS Secrets Manager ARNs of the form
/// `arn:aws:secretsmanager:region:account-id:secret:secret-id`,
/// uses the AWS SDK’s default credential and region resolution scoped to the
/// ARN’s region, and returns the `SecretString` payload of the specified secret.
pub struct SecretsManagerArn {
    region: String,
    full_arn: String,
}

impl TryFrom<&SettingsInput> for SecretsManagerArn {
    type Error = ResolverError;

    fn try_from(input: &SettingsInput) -> ResolverResult<Self> {
        use resolver_error::*;

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

#[async_trait::async_trait]
impl UriResolver for SecretsManagerArn {
    async fn resolve(&self) -> ResolverResult<String> {
        use resolver_error::*;
        let cfg = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_sdk_secretsmanager::config::Region::new(
                self.region.clone(),
            ))
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

        resp.secret_string()
            .map(str::to_string)
            .context(SecretsManagerStringMissingSnafu {
                secret_id: self.full_arn.clone(),
            })
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

        const PREFIX: &str = "secretsmanager://";
        let uri_str = input.input.as_str();
        let remainder = uri_str
            .strip_prefix(PREFIX)
            .context(SecretsManagerUriSnafu {
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
        use resolver_error::*;

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

        resp.secret_string()
            .map(str::to_string)
            .context(SecretsManagerStringMissingSnafu {
                secret_id: self.secret_id.clone(),
            })
    }
}

/// Uri Resolver that fetches a parameter by full SSM ARN.
///
/// Accepts `ssm://arn:aws:ssm:<region>:<account_id>:parameter/<name>`,
/// uses default AWS SDK credential chain (with region override and
/// and returns the decrypted value.
pub struct SsmArn {
    region: String,
    full_arn: String,
}

impl TryFrom<&SettingsInput> for SsmArn {
    type Error = ResolverError;
    fn try_from(input: &SettingsInput) -> ResolverResult<Self> {
        use resolver_error::*;

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
        use resolver_error::*;

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
        let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let client = ssm::Client::new(&config);
        use resolver_error::*;

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
pub enum ResolverError {
    //Arn
    #[snafu(display("Invalid ARN '{}': {}", input_source, reason))]
    InvalidArnFormat {
        input_source: String,
        reason: String,
    },

    //Stdin
    #[snafu(display("Failed to read standard input: {}", source))]
    StdinRead { source: std::io::Error },

    //File
    #[snafu(display("Given invalid file URI '{}'", input_source))]
    FileUri { input_source: String },

    #[snafu(display("Failed to read given file '{}': {}", input_source, source))]
    FileRead {
        input_source: String,
        source: std::io::Error,
    },

    //HTTP(S)
    #[snafu(display("Given invalid HTTP(S) URI '{}'", input_source))]
    InvalidHTTPUri { input_source: String },

    #[snafu(display(
        "HTTP object at {uri} is too large ({size} bytes, maximum is {max_size} bytes)"
    ))]
    HttpObjectTooLarge {
        size: u64,
        max_size: u64,
        uri: String,
    },

    #[snafu(display("Failed to perform HTTP GET to '{}': {}", uri, source))]
    HttpRequest { uri: String, source: reqwest::Error },

    #[snafu(display("Non-success HTTP status from '{}': {}", uri, status))]
    HttpStatus {
        uri: String,
        status: reqwest::StatusCode,
    },

    #[snafu(display("Failed to read HTTP response body from '{}': {}", uri, source))]
    HttpBody { uri: String, source: reqwest::Error },

    //S3
    #[snafu(display("Failed to HEAD S3 object s3://{bucket}/{key}"))]
    S3Head {
        source: aws_sdk_s3::error::SdkError<
            aws_sdk_s3::operation::head_object::HeadObjectError,
            aws_sdk_s3::config::http::HttpResponse,
        >,
        bucket: String,
        key: String,
    },

    #[snafu(display(
        "S3 object s3://{bucket}/{key} is too large ({size} bytes, maximum is {max_size} bytes)"
    ))]
    S3ObjectTooLarge {
        size: u64,
        max_size: u64,
        bucket: String,
        key: String,
    },

    #[snafu(display("Invalid S3 URI scheme for '{}', expected s3://", input_source))]
    S3UriScheme { input_source: String },

    #[snafu(display("Invalid S3 URI '{}': missing bucket name", input_source))]
    S3UriMissingBucket { input_source: String },

    #[snafu(display("Invalid S3 URI '{}': missing key name", input_source))]
    S3UriMissingKey { input_source: String },

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

    #[snafu(display("No Content-Length for S3 object {bucket}/{key}"))]
    S3MissingContentLength { bucket: String, key: String },

    //Secrets Manager
    #[snafu(display(
        "Invalid Secrets Manager URI scheme for '{}', expected secretsmanager://",
        input_source
    ))]
    SecretsManagerUri { input_source: String },

    #[snafu(display(
        "Failed to fetch secret '{}' from Secrets Manager: {}",
        secret_id,
        source
    ))]
    SecretsManagerGet {
        secret_id: String,
        source: aws_sdk_secretsmanager::error::SdkError<
            aws_sdk_secretsmanager::operation::get_secret_value::GetSecretValueError,
        >,
    },

    #[snafu(display("Secrets Manager secret '{}' did not return a string value", secret_id))]
    SecretsManagerStringMissing { secret_id: String },

    #[snafu(display(
        "Invalid Secrets Manager ARN scheme for '{}', expected arn:aws:secretsmanager:…",
        input_source
    ))]
    SecretsManagerArn { input_source: String },

    //SSM
    #[snafu(display(
        "Invalid SSM ARN scheme for '{}', expected arn:aws:ssm:…",
        input_source
    ))]
    SsmArn { input_source: String },

    #[snafu(display("Failed to fetch parameter '{}' from SSM ARN", parameter_name))]
    SsmArnGetParameter {
        parameter_name: String,
        source:
            aws_sdk_ssm::error::SdkError<aws_sdk_ssm::operation::get_parameter::GetParameterError>,
    },
    #[snafu(display("SSM ARN parameter '{}' did not return a string value", parameter_name))]
    SsmArnValueMissing { parameter_name: String },

    #[snafu(display("Invalid SSM URI scheme for '{}', expected ssm://", input_source))]
    SsmUri { input_source: String },

    #[snafu(display("Failed to fetch parameter '{}' from SSM: {}", parameter_name, source))]
    SsmGetParameter {
        parameter_name: String,
        source:
            aws_sdk_ssm::error::SdkError<aws_sdk_ssm::operation::get_parameter::GetParameterError>,
    },

    #[snafu(display("SSM parameter '{}' did not return a string value", parameter_name))]
    SsmParameterMissing { parameter_name: String },

    //UTF8Decode
    #[snafu(display("Failed to decode HTTP response as UTF-8 for {uri}"))]
    Utf8Decode {
        source: std::string::FromUtf8Error,
        uri: String,
    },
}
pub type ResolverResult<T> = std::result::Result<T, ResolverError>;

#[cfg(test)]
mod tests {
    use super::{FileUri, UriResolver};
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    /// Verify that FileUri::resolve() reads the full contents of a real file.
    #[tokio::test(flavor = "multi_thread")]
    async fn file_uri_reads_file_content() -> Result<(), Box<dyn std::error::Error>> {
        // 1) Create a temp file and write some content
        let mut tmp = NamedTempFile::new()?;
        write!(tmp, "test, tempfile!")?;

        // 2) Build a FileUri pointing at that path
        let path: PathBuf = tmp.path().into();
        let file_uri = FileUri { path: path.clone() };

        // 3) Resolve and assert we get back exactly what we wrote
        let result = file_uri.resolve().await?;
        assert_eq!(result, "test, tempfile!");

        Ok(())
    }
}

#[cfg(test)]
mod parse_uri_tests {
    use super::{
        FileUri, HttpUri, S3Uri, SecretsManagerArn, SecretsManagerUri, SsmArn, SsmUri, StdinUri,
    };
    use crate::apply::SettingsInput;
    use std::convert::TryFrom;
    use test_case::test_case;

    //StdinUri
    #[test_case("-"; "stdin_ok")]
    fn parse_stdin(input: &str) {
        let settings = SettingsInput::new(input);
        let uri = StdinUri::try_from(&settings).expect("should parse stdin");
        let _ = uri;
    }

    //StdinUri negative cases
    #[test_case(""; "empty_input")]
    #[test_case(" -"; "leading_space")]
    #[test_case("--"; "double_dash")]
    fn parse_stdin_fail(input: &str) {
        let settings = SettingsInput::new(input);
        assert!(
            StdinUri::try_from(&settings).is_err(),
            "only `-` should parse as stdin"
        );
    }

    //FileUri
    #[test_case("file:///tmp/foo", "/tmp/foo"; "file_ok")]
    fn parse_file(input: &str, expected_path: &str) {
        let settings = SettingsInput::new(input);
        let uri = FileUri::try_from(&settings).expect("should parse file URI");
        assert_eq!(
            uri.path.to_str().unwrap(),
            expected_path,
            "file:// path must match"
        );
    }

    //FileUri negative cases
    #[test_case("file_:/"; "weird_path")]
    #[test_case("file://no/leading/slash"; "no_leading_slash")]
    fn parse_file_fail(input: &str) {
        let settings = SettingsInput::new(input);
        assert!(
            FileUri::try_from(&settings).is_err(),
            "invalid file URI should fail"
        );
    }

    //HttpUri
    #[test_case("http://example.com/foo",  "http://example.com/foo";  "http_ok")]
    #[test_case("https://example.com/bar", "https://example.com/bar"; "https_ok")]
    fn parse_http(input: &str, expected: &str) {
        let settings = SettingsInput::new(input);
        let uri = HttpUri::try_from(&settings).expect("should parse HTTP URI");
        assert_eq!(uri.url.as_str(), expected, "HTTP URI must round‑trip");
    }

    //HttpUri negative cases
    #[test_case("ftp://example.com";           "unsupported_scheme")]
    #[test_case("http://";                     "empty_authority")]
    #[test_case("https:// ";                   "space_after_scheme")]
    fn parse_http_fail(input: &str) {
        let settings = SettingsInput::new(input);
        assert!(
            HttpUri::try_from(&settings).is_err(),
            "invalid HTTP URI should fail"
        );
    }

    //S3Uri
    #[test_case("s3://bucket/key", "bucket", "key"; "s3_ok")]
    fn parse_s3(input: &str, exp_bucket: &str, exp_key: &str) {
        let settings = SettingsInput::new(input);
        let uri = S3Uri::try_from(&settings).expect("should parse S3 URI");
        assert_eq!(uri.bucket, exp_bucket, "S3 bucket");
        assert_eq!(uri.key, exp_key, "S3 key");
    }

    //S3Uri negative cases
    #[test_case("s3://bucket";                 "missing_key")]
    #[test_case("s3:/bucket/key";             "malformed_scheme")]
    #[test_case("s3://";                      "empty_bucket_and_key")]
    fn parse_s3_fail(input: &str) {
        let settings = SettingsInput::new(input);
        assert!(
            S3Uri::try_from(&settings).is_err(),
            "invalid S3 URI should fail"
        );
    }

    // SecretsManagerArn
    #[test_case(
         "arn:aws:secretsmanager:us-east-1:111122223333:secret:mysecret",
         "us-east-1", "arn:aws:secretsmanager:us-east-1:111122223333:secret:mysecret";
         "secretsmanager_arn_ok"
    )]
    fn parse_secretsmanager_arn(input: &str, exp_region: &str, exp_id: &str) {
        let settings = SettingsInput::new(input);
        let uri = SecretsManagerArn::try_from(&settings).expect("should parse SecretsManager ARN");
        assert_eq!(uri.region, exp_region, "SecretsManager ARN region");
        assert_eq!(uri.full_arn, exp_id, "SecretsManager ARN secret id");
    }

    //SecretsManagerArn negative case
    #[test_case(
        "arn:aws:ssm:us-west-2:123456789012:parameter/myparam";
        "ssm_arn_not_secretsmanager"
    )]
    fn parse_secretsmanager_arn_fail(input: &str) {
        let settings = SettingsInput::new(input);
        assert!(
            SecretsManagerArn::try_from(&settings).is_err(),
            "SSM ARN should not parse as SecretsManagerArn"
        );
    }

    //SecretsManagerUri
    #[test_case("secretsmanager://mysecret", "mysecret"; "secrets_ok")]
    fn parse_secrets(input: &str, exp_id: &str) {
        let settings = SettingsInput::new(input);
        let uri = SecretsManagerUri::try_from(&settings).expect("should parse SecretsManager URI");
        assert_eq!(uri.secret_id, exp_id, "secret_id");
    }

    //SecretsManagerUri negative cases
    #[test_case("secretsmanager:/mysecret";    "missing_double_slash")]
    #[test_case("secretsmanager://";           "empty_secret_id")]
    #[test_case("ssm://mysecret";              "wrong_scheme")]
    #[test_case("arn:aws:secretsmanager:us-east-1:111122223333:secret:foo"; "arn_not_uri")]
    fn parse_secrets_uri_fail(input: &str) {
        let settings = SettingsInput::new(input);
        assert!(
            SecretsManagerUri::try_from(&settings).is_err(),
            "invalid SecretsManager URI should fail"
        );
    }

    // SsmArn
    #[test_case(
         "arn:aws:ssm:us-west-2:123456789012:parameter/myparam",
         "us-west-2", "arn:aws:ssm:us-west-2:123456789012:parameter/myparam";
         "ssm_arn_ok"
    )]
    fn parse_ssm_arn(input: &str, exp_region: &str, exp_param: &str) {
        let settings = SettingsInput::new(input);
        let uri = SsmArn::try_from(&settings).expect("should parse SSM ARN");
        assert_eq!(uri.region, exp_region, "SSM ARN region");
        assert_eq!(uri.full_arn, exp_param, "SSM ARN parameter");
    }

    //SsmArn negative case
    #[test_case(
        "arn:aws:secretsmanager:us-east-1:111122223333:secret:mysecret";
        "secretsmanager_arn_not_ssm"
    )]
    fn parse_ssm_arn_fail(input: &str) {
        let settings = SettingsInput::new(input);
        assert!(
            SsmArn::try_from(&settings).is_err(),
            "invalid SSM ARN should fail"
        );
    }

    //SsmUri
    #[test_case("ssm://parameter", "parameter"; "ssm_ok")]
    fn parse_ssm(input: &str, exp_param: &str) {
        let settings = SettingsInput::new(input);
        let uri = SsmUri::try_from(&settings).expect("should parse SSM URI");
        assert_eq!(uri.parameter_name, exp_param, "parameter name");
    }

    //SsmUri negative cases
    #[test_case("ssm:/parameter";             "missing_double_slash")]
    #[test_case("ssm://";                      "empty_parameter")]
    #[test_case("secretsmanager://parameter";  "wrong_scheme")]
    #[test_case("arn:aws:ssm:us-west-2:123:parameter/myparam"; "arn_not_uri")]
    fn parse_ssm_uri_fail(input: &str) {
        let settings = SettingsInput::new(input);
        assert!(
            SsmUri::try_from(&settings).is_err(),
            "invalid SSM URI should fail"
        );
    }
}

#[cfg(test)]
mod s3_uri_tests {
    use super::S3Uri;
    use crate::apply::SettingsInput;
    use std::convert::TryFrom;
    use test_case::test_case;
    #[test_case("s3://testbucket/🚀⚡️.json", "testbucket", "🚀⚡️.json"; "s3_emoji")]
    #[test_case("s3://testbucket/#hashstart.json", "testbucket", "#hashstart.json"; "hash_start")]
    #[test_case("s3://testbucket/@atstart.json", "testbucket", "@atstart.json"; "at_start")]
    #[test_case("s3://testbucket/'singlequotes'.json", "testbucket", "'singlequotes'.json"; "single_quotes")]
    #[test_case("s3://testbucket/$dollarstart.json", "testbucket", "$dollarstart.json"; "dollar_start")]
    #[test_case("s3://testbucket/doublejson.json.json", "testbucket", "doublejson.json.json"; "double_dot_json")]
    #[test_case("s3://testbucket/file with spaces.json", "testbucket", "file with spaces.json"; "key_with_spaces")]
    #[test_case("s3://testbucket/file@#$%^&*.json", "testbucket", "file@#$%^&*.json"; "symbols")]
    #[test_case("s3://testbucket/fileñ.json", "testbucket", "fileñ.json"; "n_tilde")]
    #[test_case("s3://testbucket/filepunc,;:`.json", "testbucket", "filepunc,;:`.json"; "punctuation")]
    #[test_case("s3://testbucket/?question?marks?.json", "testbucket", "?question?marks?.json"; "question_marks")]
    #[test_case("s3://testbucket/fileü漢字.json", "testbucket", "fileü漢字.json"; "other_language")]

    fn parse_s3(input: &str, exp_bucket: &str, exp_key: &str) {
        let settings = SettingsInput::new(input);
        let uri = S3Uri::try_from(&settings).expect("should parse S3 URI");
        assert_eq!(uri.bucket, exp_bucket, "S3 bucket");
        assert_eq!(uri.key, exp_key, "S3 key");
    }
}
