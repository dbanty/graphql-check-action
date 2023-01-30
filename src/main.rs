use itertools::Itertools;
use reqwest::{RequestBuilder, StatusCode};
use std::env;
use std::fmt::Display;
use std::fs::write;
use std::process::exit;

use serde_json::{json, Value};

#[tokio::main]
async fn main() {
    let github_output_path = env::var("GITHUB_OUTPUT").unwrap();

    let args: Vec<String> = env::args().collect();
    let url = &args[1];
    let auth = match args[2].as_str() {
        "" => None,
        auth => Some(auth),
    };
    let subgraph = &args[3];
    let introspection = &args[4];

    let mut errors = Vec::new();
    let mut unauthed_err = basic_query(url, None).await.err();
    if !auth.is_none() {
        if let Some(authed_err) = basic_query(url, auth).await.err() {
            errors.push(authed_err);
        }
        unauthed_err = match unauthed_err {
            Some(Error::GraphQLError(_) | Error::BadStatus(_)) => None,
            None => Some(Error::AuthNotEnforced),
            other_err => other_err,
        }
    }
    if let Some(err) = unauthed_err {
        errors.push(err);
    }

    let subgraph_enabled = match subgraph.as_str() {
        "true" => true,
        "false" => false,
        _ => {
            errors.push(Error::BadSubgraphValue);
            false
        }
    };
    if subgraph_enabled {
        if let Err(err) = check_subgraph(url, auth).await {
            errors.push(err);
        }
    }

    let allow_introspection = match introspection.as_str() {
        "true" => true,
        "false" => false,
        "" => subgraph_enabled,
        _ => {
            errors.push(Error::BadIntrospectionValue);
            true
        }
    };
    if !allow_introspection {
        if let Err(err) = require_introspection_disabled(url, auth).await {
            errors.push(err);
        }
    }

    if !errors.is_empty() {
        let errors_str = errors
            .iter()
            .unique()
            .map(|e| e.to_string())
            .collect::<Vec<String>>()
            .join(", ");
        eprintln!("Error: {errors_str}");
        write(github_output_path, format!("error={errors_str}")).unwrap();
        exit(1);
    }
}

#[derive(Debug, Eq, Hash, PartialEq)]
enum Error {
    BadUri,
    BadStatus(StatusCode),
    CouldNotConnect,
    NotGraphQL,
    GraphQLError(String),
    AuthNotEnforced,
    BadHeader,
    NotASubgraph,
    BadSubgraphValue,
    IntrospectionEnabled,
    BadIntrospectionValue,
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::BadUri => write!(f, "Bad URI"),
            Error::CouldNotConnect => write!(f, "Could not connect"),
            Error::NotGraphQL => write!(f, "Not GraphQL"),
            Error::GraphQLError(err) => write!(f, "Received error from GraphQL server: {err}"),
            Error::AuthNotEnforced => {
                write!(f, "Able to make queries with no authentication header")
            }
            Error::BadHeader => write!(
                f,
                "Provided `auth` input was not a valid header in the format of `name: value`"
            ),
            Error::BadStatus(status) => write!(f, "Got status code: {status}"),
            Error::NotASubgraph => write!(f, "GraphQL endpoint is not a subgraph"),
            Error::BadSubgraphValue => {
                write!(f, "`subgraph` input must be either `true` or `false`")
            }
            Error::IntrospectionEnabled => write!(
                f,
                "Introspection is enabled for the GraphQL server but not allowed"
            ),
            Error::BadIntrospectionValue => {
                write!(
                    f,
                    "`allow_introspection` input must be either `true` or `false`"
                )
            }
        }
    }
}

async fn basic_query(url: &str, auth_header: Option<&str>) -> Result<(), Error> {
    let client = reqwest::Client::new();
    let request = client.post(url).json(&json!({
        "query": "query{__typename}",
    }));
    let request = add_auth(auth_header, request)?;
    let body = get_json(request).await?;
    if body == json!({"data": {"__typename": "Query"}}) {
        Ok(())
    } else {
        Err(Error::NotGraphQL)
    }
}

fn add_auth(auth: Option<&str>, request: RequestBuilder) -> Result<RequestBuilder, Error> {
    if let Some(auth) = auth {
        let (header_name, header_value) = auth.split_once(':').ok_or(Error::BadHeader)?;
        let header_value = header_value.trim();
        Ok(request.header(header_name, header_value))
    } else {
        Ok(request)
    }
}

async fn get_json(request: RequestBuilder) -> Result<Value, Error> {
    let res = request.send().await.map_err(|err| {
        if err.is_builder() {
            Error::BadUri
        } else {
            Error::CouldNotConnect
        }
    })?;
    if let Err(err) = res.error_for_status_ref() {
        return Err(Error::BadStatus(err.status().unwrap()));
    }
    let body: Value = res.json().await.or(Err(Error::NotGraphQL))?;
    if let Some(obj) = body.get("errors") {
        Err(Error::GraphQLError(obj.to_string()))
    } else {
        Ok(body)
    }
}

#[cfg(test)]
mod test_utils {
    pub const BASE_URL: &str = "https://graphql-test.up.railway.app";
    pub const TOKEN: &str = env!("GRAPHQL_TOKEN");

    pub fn auth_header() -> Option<String> {
        Some(format!("Authorization: Bearer {TOKEN}"))
    }
}

#[cfg(test)]
mod test_basic_query {
    use super::test_utils::*;
    use super::*;
    use crate::Error::*;

    #[tokio::test]
    async fn unauth_success() {
        let url = format!("{BASE_URL}/graphql");
        assert!(basic_query(&url, None).await.is_ok());
    }

    #[tokio::test]
    async fn success_subgraph() {
        let url = format!("{BASE_URL}/subgraph");
        assert!(basic_query(&url, None).await.is_ok());
    }

    #[tokio::test]
    async fn bad_url() {
        let url = BASE_URL.to_string();
        let url_without_scheme = url.split('/').nth(2).unwrap();
        assert_eq!(basic_query(url_without_scheme, None).await, Err(BadUri));
    }

    #[tokio::test]
    async fn not_found() {
        let url = "https://doesntexist.dylananthony.com";
        assert_eq!(basic_query(url, None).await, Err(CouldNotConnect));
    }

    #[tokio::test]
    async fn post_not_accepted() {
        let url = format!("{BASE_URL}/no-post");
        assert_eq!(
            basic_query(&url, None).await,
            Err(BadStatus(StatusCode::METHOD_NOT_ALLOWED))
        );
    }

    #[tokio::test]
    async fn no_json_returned() {
        let url = format!("{BASE_URL}/no-json");
        assert_eq!(basic_query(&url, None).await, Err(NotGraphQL));
    }

    #[tokio::test]
    async fn not_graphql() {
        let url = format!("{BASE_URL}/json");
        assert_eq!(basic_query(&url, None).await, Err(NotGraphQL));
    }

    #[tokio::test]
    async fn auth_success() {
        let url = format!("{BASE_URL}/graphql-auth");
        assert_eq!(basic_query(&url, auth_header().as_deref()).await, Ok(()));
    }

    #[tokio::test]
    async fn subgraph_auth_success() {
        let url = format!("{BASE_URL}/subgraph-auth");
        assert!(basic_query(&url, auth_header().as_deref()).await.is_ok());
    }

    #[tokio::test]
    async fn auth_failure() {
        let url = format!("{BASE_URL}/graphql-auth");
        assert!(matches!(
            basic_query(&url, Some("Authorization: Bearer nottherealtoken")).await,
            Err(GraphQLError(_))
        ));
    }

    #[tokio::test]
    async fn missing_auth() {
        let url = format!("{BASE_URL}/graphql-auth");
        match basic_query(&url, None).await {
            Err(BadStatus(StatusCode::BAD_REQUEST)) => (),
            other => panic!("Expected Err(GraphQLError(_)), got {:?}", other),
        }
    }
}

async fn check_subgraph(url: &str, auth: Option<&str>) -> Result<(), Error> {
    let client = reqwest::Client::new();
    let request = client.post(url).json(&json!({
        "query": "query{_service{sdl}}"
    }));
    let request = add_auth(auth, request)?;
    if get_json(request).await.is_ok() {
        Ok(())
    } else {
        Err(Error::NotASubgraph)
    }
}

#[cfg(test)]
mod test_check_subgraph {
    use super::test_utils::*;
    use super::*;
    use crate::Error::NotASubgraph;

    #[tokio::test]
    async fn happy() {
        let url = format!("{BASE_URL}/subgraph");
        check_subgraph(&url, None).await.unwrap();
    }

    #[tokio::test]
    async fn happy_with_auth() {
        let url = format!("{BASE_URL}/subgraph-auth");
        check_subgraph(&url, auth_header().as_deref())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn not_a_subgraph() {
        let url = format!("{BASE_URL}/graphql");
        assert_eq!(check_subgraph(&url, None).await, Err(NotASubgraph));
    }
}

#[cfg(test)]
mod test_require_introspection_disabled {
    use super::test_utils::*;
    use super::*;
    use crate::Error::IntrospectionEnabled;

    #[tokio::test]
    async fn happy() {
        let url = format!("{BASE_URL}/graphql-no-introspection");
        require_introspection_disabled(&url, None).await.unwrap();
    }

    #[tokio::test]
    async fn introspection_enabled() {
        let url = format!("{BASE_URL}/graphql");
        assert_eq!(
            require_introspection_disabled(&url, None).await,
            Err(IntrospectionEnabled)
        );
    }
}

async fn require_introspection_disabled(url: &str, auth: Option<&str>) -> Result<(), Error> {
    let client = reqwest::Client::new();
    let request = client.post(url).json(&json!({
        "query": "query{__schema{types{name}}}"
    }));
    let request = add_auth(auth, request)?;
    match get_json(request).await {
        Ok(value) => {
            if let Some(Object(_)) = value.pointer("/data/__schema") {
                return Err(Error::IntrospectionEnabled);
            }
            Ok(())
        }
        Err(Error::GraphQLError(_)) => Ok(()),
        Err(e) => Err(e),
    }
}
