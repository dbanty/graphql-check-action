use reqwest::StatusCode;
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
    let auth = &args[2];

    let mut errors = Vec::new();
    let mut unauthed_err = basic_query(url, None).await.err();
    if !auth.is_empty() {
        if let Some(authed_err) = basic_query(url, Some(auth)).await.err() {
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

    if !errors.is_empty() {
        let errors_str = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<String>>()
            .join(", ");
        eprintln!("Error: {errors_str}");
        write(github_output_path, format!("error={errors_str}")).unwrap();
        exit(1);
    }
}

#[derive(Debug, PartialEq)]
enum Error {
    BadUri,
    BadStatus(StatusCode),
    CouldNotConnect,
    NotGraphQL,
    GraphQLError(String),
    AuthNotEnforced,
    BadHeader,
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
        }
    }
}

async fn basic_query(url: &str, auth_header: Option<&str>) -> Result<(), Error> {
    let client = reqwest::Client::new();
    let request = client.post(url).json(&json!({
        "query": "query{__typename}",
    }));
    let request = if let Some(auth) = auth_header {
        let (header_name, header_value) = auth.split_once(':').ok_or(Error::BadHeader)?;
        let header_value = header_value.trim();
        request.header(header_name, header_value)
    } else {
        request
    };
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
    if body == json!({"data": {"__typename": "Query"}}) {
        Ok(())
    } else if let Some(obj) = body.get("errors") {
        Err(Error::GraphQLError(obj.to_string()))
    } else {
        Err(Error::NotGraphQL)
    }
}

#[cfg(test)]
mod test_basic_query {
    use super::*;
    use crate::Error::*;

    const BASE_URL: &str = "https://graphql-test.up.railway.app";

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
        assert_eq!(basic_query(&url, Some(env!("GRAPHQL_TOKEN"))).await, Ok(()));
    }

    #[tokio::test]
    async fn subgraph_auth_success() {
        let url = format!("{BASE_URL}/subgraph-auth");
        assert!(basic_query(&url, Some(env!("GRAPHQL_TOKEN"))).await.is_ok());
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
