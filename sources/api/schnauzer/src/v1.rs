//! Initial Bottlerocket template rendering engine.
//!
//! Uses static handlebars helpers and provides a full, unversioned view of settings to each template.
use crate::helpers;
use handlebars::Handlebars;
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use serde::de::DeserializeOwned;
use snafu::ResultExt;
use std::path::Path;

// https://url.spec.whatwg.org/#query-percent-encode-set
const ENCODE_QUERY_CHARS: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'#').add(b'<').add(b'>');

pub mod error {
    use http::StatusCode;
    use snafu::Snafu;

    #[derive(Debug, Snafu)]
    #[snafu(visibility(pub(super)))]
    pub enum Error {
        #[snafu(display("Error {}ing to {}: {}", method, uri, source))]
        APIRequest {
            method: String,
            uri: String,
            #[snafu(source(from(apiclient::Error, Box::new)))]
            source: Box<apiclient::Error>,
        },

        #[snafu(display("Error {} when {}ing to {}: {}", code, method, uri, response_body))]
        APIResponse {
            method: String,
            uri: String,
            code: StatusCode,
            response_body: String,
        },

        #[snafu(display(
            "Error deserializing response as JSON from {} to '{}': {}",
            method,
            uri,
            source
        ))]
        ResponseJson {
            method: &'static str,
            uri: String,
            source: serde_json::Error,
        },
    }
}
pub use error::Error;
type Result<T> = std::result::Result<T, error::Error>;

/// Simple helper that extends the API client, abstracting the repeated request logic and
/// deserialization from JSON.
pub async fn get_json<T, P, S1, S2, S3>(
    socket_path: P,
    uri: S1,
    // Query parameter name, query parameter value
    query: Option<(S2, S3)>,
) -> Result<T>
where
    T: DeserializeOwned,
    P: AsRef<Path>,
    S1: AsRef<str>,
    S2: AsRef<str>,
    S3: AsRef<str>,
{
    let mut uri = uri.as_ref().to_string();
    // Add (escaped) query parameter, if given
    if let Some((query_param, query_arg)) = query {
        let query_raw = format!("{}={}", query_param.as_ref(), query_arg.as_ref());
        let query_escaped = utf8_percent_encode(&query_raw, ENCODE_QUERY_CHARS);
        uri = format!("{uri}?{query_escaped}");
    }

    let method = "GET";
    trace!("{}ing from {}", method, uri);
    let (code, response_body) = apiclient::raw_request(socket_path, &uri, method, None)
        .await
        .context(error::APIRequestSnafu { method, uri: &uri })?;

    if !code.is_success() {
        return error::APIResponseSnafu {
            method,
            uri,
            code,
            response_body,
        }
        .fail();
    }
    trace!("JSON response: {}", response_body);

    serde_json::from_str(&response_body).context(error::ResponseJsonSnafu { method, uri })
}

/// Requests all settings from the API so they can be used as the data source for a handlebars
/// templating call.
pub async fn get_settings<P>(socket_path: P) -> Result<model::Model>
where
    P: AsRef<Path>,
{
    debug!("Querying API for settings data");
    let settings: model::Model =
        get_json(&socket_path, "/", None as Option<(String, String)>).await?;
    trace!("Model values: {:?}", settings);

    Ok(settings)
}

/// Build a handlebars template registry with our common helper functions.
pub fn build_template_registry() -> Result<handlebars::Handlebars<'static>> {
    let mut template_registry = Handlebars::new();
    // Strict mode will panic if a key exists in the template
    // but isn't provided in the data given to the renderer
    template_registry.set_strict_mode(true);

    // Prefer snake case for helper names (we accidentally created a few with kabob case)
    template_registry.register_helper("base64_decode", Box::new(helpers::base64_decode));
    template_registry.register_helper("join_map", Box::new(helpers::join_map));
    template_registry.register_helper("join_node_taints", Box::new(helpers::join_node_taints));
    template_registry.register_helper("default", Box::new(helpers::default));
    template_registry.register_helper("ecr-prefix", Box::new(helpers::ecr_prefix));
    template_registry.register_helper("aws-config", Box::new(helpers::aws_config));
    template_registry.register_helper("tuf-prefix", Box::new(helpers::tuf_prefix));
    template_registry.register_helper("metadata-prefix", Box::new(helpers::metadata_prefix));
    template_registry.register_helper("host", Box::new(helpers::host));
    template_registry.register_helper("goarch", Box::new(helpers::goarch));
    template_registry.register_helper("join_array", Box::new(helpers::join_array));
    template_registry.register_helper("toml_encode", Box::new(helpers::toml_encode));
    template_registry.register_helper("kube_reserve_cpu", Box::new(helpers::kube_reserve_cpu));
    template_registry.register_helper(
        "kube_reserve_memory",
        Box::new(helpers::kube_reserve_memory),
    );
    template_registry.register_helper("localhost_aliases", Box::new(helpers::localhost_aliases));
    template_registry.register_helper("etc_hosts_entries", Box::new(helpers::etc_hosts_entries));
    template_registry.register_helper("any_enabled", Box::new(helpers::any_enabled));
    template_registry.register_helper("oci_defaults", Box::new(helpers::oci_defaults));
    template_registry.register_helper("negate_or_else", Box::new(helpers::negate_or_else));
    template_registry.register_helper(
        "ecs_metadata_service_limits",
        Box::new(helpers::ecs_metadata_service_limits),
    );

    template_registry.register_helper("is_ipv4", Box::new(helpers::is_ipv4));
    template_registry.register_helper("is_ipv6", Box::new(helpers::is_ipv6));
    template_registry.register_helper("cidr_to_ipaddr", Box::new(helpers::cidr_to_ipaddr));
    template_registry.register_helper("replace_ipv4_octet", Box::new(helpers::replace_ipv4_octet));

    template_registry.register_helper("is_null", Box::new(helpers::IsNull));
    template_registry.register_helper("is_bool", Box::new(helpers::IsBool));
    template_registry.register_helper("is_number", Box::new(helpers::IsNumber));
    template_registry.register_helper("is_string", Box::new(helpers::IsString));
    template_registry.register_helper("is_array", Box::new(helpers::IsArray));
    template_registry.register_helper("is_object", Box::new(helpers::IsObject));

    Ok(template_registry)
}

pub mod cli {
    //! CLI module for schnauzer v1
    use snafu::{ensure, OptionExt, ResultExt};
    use std::collections::HashMap;
    use std::string::String;
    use std::{env, process};

    const API_METADATA_URI_BASE: &str = "/metadata/";

    pub mod error {
        use http::StatusCode;
        use snafu::Snafu;

        #[derive(Debug, Snafu)]
        #[snafu(visibility(pub))]
        pub enum Error {
            #[snafu(display("Error {}ing to {}: {}", method, uri, source))]
            APIRequest {
                method: String,
                uri: String,
                #[snafu(source(from(apiclient::Error, Box::new)))]
                source: Box<apiclient::Error>,
            },

            #[snafu(display("Error {} when {}ing to '{}': {}", code, method, uri, response_body))]
            Response {
                method: String,
                uri: String,
                code: StatusCode,
                response_body: String,
            },

            #[snafu(display("Error deserializing to JSON: {}", source))]
            DeserializeJson { source: serde_json::error::Error },

            #[snafu(display("Error serializing to JSON '{}': {}", output, source))]
            SerializeOutput {
                output: String,
                source: serde_json::error::Error,
            },

            #[snafu(display("Missing metadata {} for key: {}", meta, key))]
            MissingMetadata { meta: String, key: String },

            #[snafu(display("Metadata {} expected to be {}, got: {}", meta, expected, value))]
            MetadataWrongType {
                meta: String,
                expected: String,
                value: String,
            },

            #[snafu(display("Failed to build template registry: {}", source))]
            BuildTemplateRegistry { source: crate::v1::Error },

            #[snafu(display("Failed to get settings from API: {}", source))]
            GetSettings { source: crate::v1::Error },

            #[snafu(display(
                "Failed to render setting '{}' from template '{}': {}",
                setting_name,
                template,
                source
            ))]
            RenderTemplate {
                setting_name: String,
                template: String,
                #[snafu(source(from(handlebars::RenderError, Box::new)))]
                source: Box<handlebars::RenderError>,
            },
        }
    }
    pub use error::Error;
    type Result<T> = std::result::Result<T, error::Error>;

    /// Returns the value of a metadata key for a given data key, erroring if the value is not a
    /// string or is empty.
    async fn get_metadata(key: &str, meta: &str) -> Result<String> {
        let uri = &format!("{API_METADATA_URI_BASE}{meta}?keys={key}");
        let method = "GET";
        let (code, response_body) =
            apiclient::raw_request(constants::API_SOCKET, &uri, method, None)
                .await
                .context(error::APIRequestSnafu { method, uri })?;
        ensure!(
            code.is_success(),
            error::ResponseSnafu {
                method,
                uri,
                code,
                response_body
            }
        );

        // Metadata responses are of the form `{"data_key": METADATA}` so we pull out the value.
        let mut response_map: HashMap<String, serde_json::Value> =
            serde_json::from_str(&response_body).context(error::DeserializeJsonSnafu)?;
        let response_val = response_map
            .remove(key)
            .context(error::MissingMetadataSnafu { meta, key })?;

        // Ensure it's a non-empty string
        let response_str =
            response_val
                .as_str()
                .with_context(|| error::MetadataWrongTypeSnafu {
                    meta,
                    expected: "string",
                    value: response_val.to_string(),
                })?;
        ensure!(
            !response_str.is_empty(),
            error::MissingMetadataSnafu { meta, key }
        );
        Ok(response_str.to_string())
    }

    /// Print usage message.
    fn usage() -> ! {
        let program_name = env::args().next().unwrap_or_else(|| "program".to_string());
        eprintln!("Usage: {program_name} SETTING_KEY");
        process::exit(2);
    }

    /// Parses args for the setting key name.
    fn parse_args(mut args: env::Args) -> String {
        let arg = args.nth(1).unwrap_or_else(|| "--help".to_string());
        if arg == "--help" || arg == "-h" {
            usage()
        }
        arg
    }

    /// Main CLI entry point for schnauzer v1
    pub async fn run() -> Result<()> {
        let setting_name = parse_args(env::args());

        let registry =
            crate::v1::build_template_registry().context(error::BuildTemplateRegistrySnafu)?;
        let template = get_metadata(&setting_name, "templates").await?;
        let settings = crate::v1::get_settings(constants::API_SOCKET)
            .await
            .context(error::GetSettingsSnafu)?;

        let setting =
            registry
                .render_template(&template, &settings)
                .context(error::RenderTemplateSnafu {
                    setting_name,
                    template,
                })?;

        // sundog expects JSON-serialized output so that many types can be represented, allowing the
        // API model to use more accurate types.
        let output = serde_json::to_string(&setting)
            .context(error::SerializeOutputSnafu { output: &setting })?;

        println!("{output}");
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use handlebars::Handlebars;
    use serde_json::json;

    #[test]
    fn render_whitespace() {
        let registry = Handlebars::new();
        // Similar to a proxy configuration file whose rendering behavior changed in handlebars 4.
        let tmpl = r###"
{{#if p}}
VAR1={{p}}
VAR2={{p}}
{{/if}}
LIST_UPPER={{#each a}}{{this}},{{/each}}x,y{{#if b}},{{b}}{{/if}}{{#if c}},.{{c}}{{/if}}
list_lower={{#each a}}{{this}},{{/each}}x,y{{#if b}},{{b}}{{/if}}{{#if c}},.{{c}}{{/if}}
        "###;
        let data = json!({"a": ["a1", "a2"], "b": "b1", "c": "c1", "p": "hi"});
        let expected = r###"
VAR1=hi
VAR2=hi
LIST_UPPER=a1,a2,x,y,b1,.c1
list_lower=a1,a2,x,y,b1,.c1
        "###;

        let result = registry.render_template(tmpl, &data).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn render_newline() {
        let registry = Handlebars::new();
        // Another simple check for whitespace behavior changes in handlebars 4.
        let tmpl = r###"{{#if a}}x{{/if}}
y"###;
        let data = json!({ "a": true});
        let expected = "x
y";

        let result = registry.render_template(tmpl, &data).unwrap();
        assert_eq!(result, expected);
    }
}
