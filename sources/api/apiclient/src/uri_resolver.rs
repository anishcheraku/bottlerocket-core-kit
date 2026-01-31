//! Defines `UriResolver`, an async trait for fetching UTF-8 text from various URI schemes.
//! Concrete types parse and resolve a single scheme via `TryFrom` + `resolve()`:
//!  - `StdinUri` for "-", `FileUri` for `file://`, `HttpUri` for `http(s)://`
//!  - `S3Uri` for `s3://`, `SecretsManagerUri` for `secretsmanager://`, `SsmUri` for `ssm://`
//!
//! To add a new scheme, implement its `TryFrom` and `UriResolver::resolve()`.  
use async_trait::async_trait;
use base64::{engine, Engine as _};
use reqwest::Url;
use resolver_error::*;
use snafu::{ensure, OptionExt, ResultExt, Snafu};
use std::any::Any;
use std::convert::TryFrom;
use std::path::PathBuf;
use tokio::io::AsyncReadExt;

/// Maximum allowed object size for remote URI resolvers (2 MiB).
/// Applies to: http://, https://, s3://, ssm://, secretsmanager://, and ARN forms.
/// Matches actix-web's default JSON payload limit in apiserver.
pub const MAX_SIZE_BYTES: u64 = 2 * 1024 * 1024;

/// Reads HTTP response body with a size limit, streaming chunks to avoid unbounded allocation.
/// Checks Content-Length header first for early rejection, then streams with hard limit.
pub async fn read_bounded_response(
    mut resp: reqwest::Response,
    max_size: u64,
    uri: &str,
) -> Result<Vec<u8>, ResolverError> {
    if let Some(content_length) = resp.content_length() {
        if content_length > max_size {
            return HttpObjectTooLargeSnafu {
                size: content_length,
                max_size,
                uri,
            }
            .fail();
        }
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = resp.chunk().await.context(HttpBodySnafu { uri })? {
        if bytes.len() as u64 + chunk.len() as u64 > max_size {
            return HttpObjectTooLargeSnafu {
                size: bytes.len() as u64 + chunk.len() as u64,
                max_size,
                uri,
            }
            .fail();
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(feature = "tls")]
pub use crate::cloud_resolvers::{S3Uri, SecretsManagerArn, SecretsManagerUri, SsmArn, SsmUri};

pub struct SettingsInput {
    pub input: String,
    pub parsed_url: Option<Url>,
}

impl SettingsInput {
    pub fn new(input: impl Into<String>) -> Self {
        let input = input.into();
        let parsed_url = match Url::parse(&input) {
            Ok(url) => Some(url),
            Err(err) => {
                log::debug!("URL parse failed for '{}': {}", input, err);
                None
            }
        };
        SettingsInput { input, parsed_url }
    }
}

macro_rules! try_resolvers {
    ($input:expr, $($resolver_type:ty),+ $(,)?) => {
        $(
            if let Ok(r) = <$resolver_type>::try_from($input) {
                log::debug!("select_resolver: picked {}", stringify!($resolver_type));
                return Ok(Box::new(r));
            }
        )+
    };
}

pub fn select_resolver(input: &SettingsInput) -> ResolverResult<Box<dyn UriResolver>> {
    try_resolvers!(input, StdinUri, Base64Uri, FileUri, HttpUri,);

    #[cfg(feature = "tls")]
    try_resolvers!(
        input,
        S3Uri,
        SecretsManagerArn,
        SecretsManagerUri,
        SsmArn,
        SsmUri,
    );

    resolver_error::NoResolverSnafu {
        input_source: input.input.clone(),
    }
    .fail()
}

#[async_trait]
pub trait UriResolver: Any {
    async fn resolve(&self) -> ResolverResult<String>;
}

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

/// Reads all content from an async reader into a String.
/// Extracted for testability (stdin cannot be mocked in unit tests).
async fn read_from_async<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut R,
) -> ResolverResult<String> {
    let mut buf = String::new();
    reader
        .read_to_string(&mut buf)
        .await
        .context(resolver_error::StdinReadSnafu)?;
    Ok(buf)
}

#[async_trait]
impl UriResolver for StdinUri {
    async fn resolve(&self) -> ResolverResult<String> {
        read_from_async(&mut tokio::io::stdin()).await
    }
}

pub struct Base64Uri {
    encoded_data: String,
}

impl TryFrom<&SettingsInput> for Base64Uri {
    type Error = ResolverError;
    fn try_from(input: &SettingsInput) -> ResolverResult<Self> {
        use resolver_error::*;
        const PREFIX: &str = "base64:";
        let encoded_data = input.input.strip_prefix(PREFIX).context(Base64UriSnafu {
            input_source: input.input.clone(),
        })?;
        ensure!(
            !encoded_data.is_empty(),
            Base64UriSnafu {
                input_source: input.input.clone()
            }
        );
        Ok(Base64Uri {
            encoded_data: encoded_data.to_string(),
        })
    }
}

#[async_trait]
impl UriResolver for Base64Uri {
    async fn resolve(&self) -> ResolverResult<String> {
        use resolver_error::*;
        let bytes = engine::general_purpose::STANDARD
            .decode(&self.encoded_data)
            .context(Base64DecodeSnafu {
                input_source: format!("base64:{}", self.encoded_data),
            })?;
        String::from_utf8(bytes).context(Utf8DecodeSnafu {
            uri: format!("base64:{}", self.encoded_data),
        })
    }
}

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
        let bytes = read_bounded_response(resp, MAX_SIZE_BYTES, &self.url.to_string()).await?;
        String::from_utf8(bytes).context(Utf8DecodeSnafu {
            uri: self.url.to_string(),
        })
    }
}

#[derive(Debug, Snafu)]
#[snafu(module, visibility(pub(crate)))]
pub enum ResolverError {
    #[snafu(display("No URI resolver found for '{}'", input_source))]
    NoResolver { input_source: String },

    #[snafu(display("Invalid ARN '{}': {}", input_source, reason))]
    InvalidArnFormat { input_source: String, reason: String },

    #[snafu(display("Invalid base64 URI scheme for '{}', expected base64:", input_source))]
    Base64Uri { input_source: String },

    #[snafu(display("Failed to decode base64 data from '{}': {}", input_source, source))]
    Base64Decode {
        input_source: String,
        source: base64::DecodeError,
    },

    #[snafu(display("Failed to read standard input: {}", source))]
    StdinRead { source: std::io::Error },

    #[snafu(display("Given invalid file URI '{}'", input_source))]
    FileUri { input_source: String },

    #[snafu(display("Failed to read given file '{}': {}", input_source, source))]
    FileRead { input_source: String, source: std::io::Error },

    #[snafu(display("Given invalid HTTP(S) URI '{}'", input_source))]
    InvalidHTTPUri { input_source: String },

    #[snafu(display("HTTP object at {uri} is too large ({size} bytes, maximum is {max_size} bytes)"))]
    HttpObjectTooLarge { size: u64, max_size: u64, uri: String },

    #[snafu(display("Failed to perform HTTP GET to '{}': {}", uri, source))]
    HttpRequest { uri: String, source: reqwest::Error },

    #[snafu(display("Non-success HTTP status from '{}': {}", uri, status))]
    HttpStatus { uri: String, status: reqwest::StatusCode },

    #[snafu(display("Failed to read HTTP response body from '{}': {}", uri, source))]
    HttpBody { uri: String, source: reqwest::Error },

    #[snafu(display("Failed to decode as UTF-8 for {uri}"))]
    Utf8Decode { source: std::string::FromUtf8Error, uri: String },

    #[cfg(feature = "tls")]
    #[snafu(display("Cloud resolver error: {}", source))]
    Cloud { source: crate::cloud_resolvers::CloudError },
}

pub type ResolverResult<T> = std::result::Result<T, ResolverError>;

#[cfg(feature = "tls")]
impl From<crate::cloud_resolvers::CloudError> for ResolverError {
    fn from(e: crate::cloud_resolvers::CloudError) -> Self {
        ResolverError::Cloud { source: e }
    }
}

#[cfg(test)]
mod tests {
    use super::{Base64Uri, FileUri, UriResolver};
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    #[tokio::test(flavor = "multi_thread")]
    async fn file_uri_reads_file_content() -> Result<(), Box<dyn std::error::Error>> {
        let mut tmp = NamedTempFile::new()?;
        write!(tmp, "test, tempfile!")?;
        let path: PathBuf = tmp.path().into();
        let file_uri = FileUri { path: path.clone() };
        let result = file_uri.resolve().await?;
        assert_eq!(result, "test, tempfile!");
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn base64_uri_decodes_valid_data() -> Result<(), Box<dyn std::error::Error>> {
        let base64_uri = Base64Uri {
            encoded_data: "SGVsbG8gV29ybGQ=".to_string(),
        };
        let result = base64_uri.resolve().await?;
        assert_eq!(result, "Hello World");
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn base64_uri_fails_on_invalid_base64() {
        let base64_uri = Base64Uri {
            encoded_data: "not!valid!base64!".to_string(),
        };
        let result = base64_uri.resolve().await;
        assert!(result.is_err(), "invalid base64 should fail");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn base64_uri_fails_on_non_utf8() {
        let base64_uri = Base64Uri {
            encoded_data: "/w==".to_string(),
        };
        let result = base64_uri.resolve().await;
        assert!(result.is_err(), "non-UTF8 data should fail");
    }
}

#[cfg(test)]
mod parse_uri_tests {
    use super::{Base64Uri, FileUri, HttpUri, SettingsInput, StdinUri};
    #[cfg(feature = "tls")]
    use super::{S3Uri, SecretsManagerArn, SecretsManagerUri, SsmArn, SsmUri};
    use std::convert::TryFrom;
    use test_case::test_case;

    #[test_case("-"; "stdin_ok")]
    fn parse_stdin(input: &str) {
        let settings = SettingsInput::new(input);
        let uri = StdinUri::try_from(&settings).expect("should parse stdin");
        let _ = uri;
    }

    #[test_case(""; "empty_input")]
    #[test_case(" -"; "leading_space")]
    #[test_case("--"; "double_dash")]
    fn parse_stdin_fail(input: &str) {
        let settings = SettingsInput::new(input);
        assert!(StdinUri::try_from(&settings).is_err(), "only `-` should parse as stdin");
    }

    #[test_case("file:///tmp/foo", "/tmp/foo"; "file_ok")]
    fn parse_file(input: &str, expected_path: &str) {
        let settings = SettingsInput::new(input);
        let uri = FileUri::try_from(&settings).expect("should parse file URI");
        assert_eq!(uri.path.to_str().unwrap(), expected_path, "file:// path must match");
    }

    #[test_case("file_:/"; "weird_path")]
    #[test_case("file://no/leading/slash"; "no_leading_slash")]
    fn parse_file_fail(input: &str) {
        let settings = SettingsInput::new(input);
        assert!(FileUri::try_from(&settings).is_err(), "invalid file URI should fail");
    }

    #[test_case("http://example.com/foo",  "http://example.com/foo";  "http_ok")]
    #[test_case("https://example.com/bar", "https://example.com/bar"; "https_ok")]
    fn parse_http(input: &str, expected: &str) {
        let settings = SettingsInput::new(input);
        let uri = HttpUri::try_from(&settings).expect("should parse HTTP URI");
        assert_eq!(uri.url.as_str(), expected, "HTTP URI must round‑trip");
    }

    #[test_case("ftp://example.com";           "unsupported_scheme")]
    #[test_case("http://";                     "empty_authority")]
    #[test_case("https:// ";                   "space_after_scheme")]
    fn parse_http_fail(input: &str) {
        let settings = SettingsInput::new(input);
        assert!(HttpUri::try_from(&settings).is_err(), "invalid HTTP URI should fail");
    }

    #[cfg(feature = "tls")]
    #[test_case("s3://bucket/key", "bucket", "key"; "s3_ok")]
    fn parse_s3(input: &str, exp_bucket: &str, exp_key: &str) {
        let settings = SettingsInput::new(input);
        let uri = S3Uri::try_from(&settings).expect("should parse S3 URI");
        assert_eq!(uri.bucket, exp_bucket, "S3 bucket");
        assert_eq!(uri.key, exp_key, "S3 key");
    }

    #[cfg(feature = "tls")]
    #[test_case("s3://bucket";                 "missing_key")]
    #[test_case("s3:/bucket/key";             "malformed_scheme")]
    #[test_case("s3://";                      "empty_bucket_and_key")]
    fn parse_s3_fail(input: &str) {
        let settings = SettingsInput::new(input);
        assert!(S3Uri::try_from(&settings).is_err(), "invalid S3 URI should fail");
    }

    #[cfg(feature = "tls")]
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

    #[cfg(feature = "tls")]
    #[test_case("arn:aws:ssm:us-west-2:123456789012:parameter/myparam"; "ssm_arn_not_secretsmanager")]
    fn parse_secretsmanager_arn_fail(input: &str) {
        let settings = SettingsInput::new(input);
        assert!(SecretsManagerArn::try_from(&settings).is_err(), "SSM ARN should not parse as SecretsManagerArn");
    }

    #[cfg(feature = "tls")]
    #[test_case("secretsmanager://mysecret", "mysecret"; "secrets_ok")]
    fn parse_secrets(input: &str, exp_id: &str) {
        let settings = SettingsInput::new(input);
        let uri = SecretsManagerUri::try_from(&settings).expect("should parse SecretsManager URI");
        assert_eq!(uri.secret_id, exp_id, "secret_id");
    }

    #[cfg(feature = "tls")]
    #[test_case("secretsmanager:/mysecret";    "missing_double_slash")]
    #[test_case("secretsmanager://";           "empty_secret_id")]
    #[test_case("ssm://mysecret";              "wrong_scheme")]
    #[test_case("arn:aws:secretsmanager:us-east-1:111122223333:secret:foo"; "arn_not_uri")]
    fn parse_secrets_uri_fail(input: &str) {
        let settings = SettingsInput::new(input);
        assert!(SecretsManagerUri::try_from(&settings).is_err(), "invalid SecretsManager URI should fail");
    }

    #[cfg(feature = "tls")]
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

    #[cfg(feature = "tls")]
    #[test_case("arn:aws:secretsmanager:us-east-1:111122223333:secret:mysecret"; "secretsmanager_arn_not_ssm")]
    fn parse_ssm_arn_fail(input: &str) {
        let settings = SettingsInput::new(input);
        assert!(SsmArn::try_from(&settings).is_err(), "invalid SSM ARN should fail");
    }

    #[cfg(feature = "tls")]
    #[test_case("ssm://parameter", "parameter"; "ssm_ok")]
    fn parse_ssm(input: &str, exp_param: &str) {
        let settings = SettingsInput::new(input);
        let uri = SsmUri::try_from(&settings).expect("should parse SSM URI");
        assert_eq!(uri.parameter_name, exp_param, "parameter name");
    }

    #[cfg(feature = "tls")]
    #[test_case("ssm:/parameter";             "missing_double_slash")]
    #[test_case("ssm://";                      "empty_parameter")]
    #[test_case("secretsmanager://parameter";  "wrong_scheme")]
    #[test_case("arn:aws:ssm:us-west-2:123:parameter/myparam"; "arn_not_uri")]
    fn parse_ssm_uri_fail(input: &str) {
        let settings = SettingsInput::new(input);
        assert!(SsmUri::try_from(&settings).is_err(), "invalid SSM URI should fail");
    }

    #[test_case("base64:SGVsbG8gV29ybGQ=", "SGVsbG8gV29ybGQ="; "base64_ok")]
    fn parse_base64(input: &str, exp_data: &str) {
        let settings = SettingsInput::new(input);
        let uri = Base64Uri::try_from(&settings).expect("should parse base64 URI");
        assert_eq!(uri.encoded_data, exp_data, "base64 encoded data");
    }

    #[test_case("base64";                      "missing_colon")]
    #[test_case("base64:";                     "empty_data")]
    #[test_case("file://data";                 "wrong_scheme")]
    fn parse_base64_fail(input: &str) {
        let settings = SettingsInput::new(input);
        assert!(
            Base64Uri::try_from(&settings).is_err(),
            "invalid base64 URI should fail"
        );
    }
}

#[cfg(all(test, feature = "tls"))]
mod s3_uri_tests {
    use super::{S3Uri, SettingsInput};
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
    #[test_case("s3://testbucket/fileñ.json", "testbucket", "fileñ.json"; "n_tilde")]
    #[test_case("s3://testbucket/filepunc,;:`.json", "testbucket", "filepunc,;:`.json"; "punctuation")]
    #[test_case("s3://testbucket/?question?marks?.json", "testbucket", "?question?marks?.json"; "question_marks")]
    #[test_case("s3://testbucket/fileü漢字.json", "testbucket", "fileü漢字.json"; "other_language")]
    fn parse_s3(input: &str, exp_bucket: &str, exp_key: &str) {
        let settings = SettingsInput::new(input);
        let uri = S3Uri::try_from(&settings).expect("should parse S3 URI");
        assert_eq!(uri.bucket, exp_bucket, "S3 bucket");
        assert_eq!(uri.key, exp_key, "S3 key");
    }
}
