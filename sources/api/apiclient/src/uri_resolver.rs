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

/// Anything that can fetch itself as a UTF-8 `String`.
#[async_trait]
pub trait UriResolver: Any {
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
        use resolver_error::*;

        let url = input.parsed_url.clone().context(FileUriSnafu {
            input_source: input.input.clone(),
        })?;

        // only accept file://
        ensure!(
            url.scheme() == "file",
            FileUriSnafu {
                input_source: url.to_string()
            }
        );

        // convert to PathBuf or error
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
        let url = input.parsed_url.clone().context(InvalidHTTPUriSnafu {
            input_source: input.input.clone(),
        })?;
        use resolver_error::*;
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
        resp.text().await.context(HttpBodySnafu {
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
        let bytes = resp.body.collect().await.context(S3BodySnafu {
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
            SecretsManagerUriSnafu {
                input_source: input.input.clone()
            }
        );
        // strip the prefix and ensure there's actually an ID
        let id = &input.input.as_str().strip_prefix("secretsmanager://");
        ensure!(
            id.is_some(),
            SecretsManagerUriSnafu {
                input_source: input.input.clone()
            }
        );
        Ok(SecretsManagerUri {
            secret_id: id.unwrap_or_default().to_string(),
        })
    }
}

#[async_trait]
impl UriResolver for SecretsManagerUri {
    async fn resolve(&self) -> ResolverResult<String> {
        use aws_config::{self};
        use aws_sdk_secretsmanager;
        use resolver_error::*;

        // 1) load AWS config (region/account via env)
        let cfg = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let client = aws_sdk_secretsmanager::Client::new(&cfg);

        // 2) fetch the secret, propagating any SdkError into SecretsManagerGet
        let resp = client
            .get_secret_value()
            .secret_id(self.secret_id.clone())
            .send()
            .await
            .context(SecretsManagerGetSnafu {
                secret_id: self.secret_id.clone(),
            })?;

        // 3) extract the string payload, or error if it was missing
        resp.secret_string()
            .map(str::to_string)
            .context(SecretsManagerStringMissingSnafu {
                secret_id: self.secret_id.clone(),
            })
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
            SsmUriSnafu {
                input_source: input.input.as_str().to_string()
            }
        );

        // strip the prefix and ensure there's actually a name
        let name = &input.input.as_str()["ssm://".len()..];
        ensure!(
            !name.is_empty(),
            SsmUriSnafu {
                input_source: input.input.as_str().to_string()
            }
        );

        Ok(SsmUri {
            parameter_name: name.to_string(),
        })
    }
}

#[async_trait]
impl UriResolver for SsmUri {
    async fn resolve(&self) -> ResolverResult<String> {
        // use default region chain
        let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let client = ssm::Client::new(&config);
        use resolver_error::*;

        // fetch the parameter, with decryption
        let resp = client
            .get_parameter()
            .name(self.parameter_name.clone())
            .with_decryption(true)
            .send()
            .await
            .context(SsmGetParameterSnafu {
                parameter_name: self.parameter_name.clone(),
            })?;

        // extract the string value
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
    #[snafu(display("Failed to read standard input: {}", source))]
    StdinRead { source: std::io::Error },

    #[snafu(display("Given invalid file URI '{}'", input_source))]
    FileUri { input_source: String },

    #[snafu(display("Failed to read given file '{}': {}", input_source, source))]
    FileRead {
        input_source: String,
        source: std::io::Error,
    },

    #[snafu(display("Given invalid HTTP(S) URI '{}'", input_source))]
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
}
pub type ResolverResult<T> = std::result::Result<T, ResolverError>;

#[cfg(test)]
mod tests {
    use super::{FileUri, UriResolver};
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    /// Verify that FileUri::resolve() reads the full contents of a real file.
    #[tokio::test]
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
    use super::{FileUri, HttpUri, S3Uri, SecretsManagerUri, SsmUri, StdinUri};
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

    //HttpUri
    #[test_case("http://example.com/foo",  "http://example.com/foo";  "http_ok")]
    #[test_case("https://example.com/bar", "https://example.com/bar"; "https_ok")]
    fn parse_http(input: &str, expected: &str) {
        let settings = SettingsInput::new(input);
        let uri = HttpUri::try_from(&settings).expect("should parse HTTP URI");
        assert_eq!(uri.url.as_str(), expected, "HTTP URI must round‑trip");
    }

    //S3Uri
    #[test_case("s3://bucket/key", "bucket", "key"; "s3_ok")]
    fn parse_s3(input: &str, exp_bucket: &str, exp_key: &str) {
        let settings = SettingsInput::new(input);
        let uri = S3Uri::try_from(&settings).expect("should parse S3 URI");
        assert_eq!(uri.bucket, exp_bucket, "S3 bucket");
        assert_eq!(uri.key, exp_key, "S3 key");
    }

    //SecretsManagerUri
    #[test_case("secretsmanager://mysecret", "mysecret"; "secrets_ok")]
    fn parse_secrets(input: &str, exp_id: &str) {
        let settings = SettingsInput::new(input);
        let uri = SecretsManagerUri::try_from(&settings).expect("should parse SecretsManager URI");
        assert_eq!(uri.secret_id, exp_id, "secret_id");
    }

    //SsmUri
    #[test_case("ssm://parameter", "parameter"; "ssm_ok")]
    fn parse_ssm(input: &str, exp_param: &str) {
        let settings = SettingsInput::new(input);
        let uri = SsmUri::try_from(&settings).expect("should parse SSM URI");
        assert_eq!(uri.parameter_name, exp_param, "parameter name");
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
    #[test_case("s3://testbucket/fileü漢字.json", "testbucket", "fileü漢字.json"; "other_language")]

    fn parse_s3(input: &str, exp_bucket: &str, exp_key: &str) {
        let settings = SettingsInput::new(input);
        let uri = S3Uri::try_from(&settings).expect("should parse S3 URI");
        assert_eq!(uri.bucket, exp_bucket, "S3 bucket");
        assert_eq!(uri.key, exp_key, "S3 key");
    }
}
