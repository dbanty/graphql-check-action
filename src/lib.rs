use std::fmt::Display;
use std::sync::Arc;

use reqwest::{RequestBuilder, StatusCode};
use serde_json::Value::Object;
use serde_json::{json, Value};

pub async fn run_checks(
    url: &str,
    auth: Auth,
    subgraph: Subgraph,
    introspection: Introspection,
) -> Result<(), Vec<Error>> {
    let mut errors = Vec::new();
    let url = Arc::new(url.to_string());

    let unauthed_future = tokio::spawn(basic_query(url.clone(), Auth::Disabled));
    let subgraph_future = tokio::spawn(check_subgraph(url.clone(), auth.clone()));
    let introspection_future = if let Introspection::Disallow = introspection {
        Some(tokio::spawn(require_introspection_disabled(
            url.clone(),
            auth.clone(),
        )))
    } else {
        None
    };

    let unauthed_err = if auth.is_enabled() {
        if let Some(authed_err) = basic_query(url.clone(), auth.clone()).await.err() {
            errors.push(authed_err);
        }
        match unauthed_future.await {
            Ok(Err(Error::GraphQLError(_) | Error::BadStatus(_))) => None,
            Ok(Ok(())) => Some(Error::AuthNotEnforced),
            Ok(Err(other_err)) => Some(other_err),
            Err(_) => None,
        }
    } else {
        unauthed_future.await.ok().and_then(|res| res.err())
    };
    if let Some(err) = unauthed_err {
        errors.push(err);
    }

    let subgraph_err = subgraph_future.await.ok().and_then(|res| res.err());
    let is_subgraph = if let Some(err) = subgraph_err {
        if subgraph.required() {
            errors.push(err);
        }
        false
    } else {
        true
    };

    if is_subgraph && !auth.is_enabled() && subgraph.security_required() {
        errors.push(Error::InsecureSubgraph)
    }

    if let Some(fut) = introspection_future {
        if let Ok(Err(e)) = fut.await {
            errors.push(e);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Auth {
    Enabled { header: Arc<String> },
    Disabled,
}

impl Auth {
    pub fn new(header: Option<String>) -> Self {
        if let Some(header) = header {
            Self::Enabled {
                header: Arc::new(header),
            }
        } else {
            Self::Disabled
        }
    }

    const fn is_enabled(&self) -> bool {
        matches!(self, Auth::Enabled { .. })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Subgraph {
    Secure,
    Insecure,
    NotASubgraph,
}

impl Subgraph {
    const fn required(&self) -> bool {
        matches!(self, Subgraph::Secure | Subgraph::Insecure)
    }

    const fn security_required(&self) -> bool {
        matches!(self, Subgraph::Secure | Subgraph::NotASubgraph)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Introspection {
    Allow,
    Disallow,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Error {
    BadUri,
    BadStatus(StatusCode),
    CouldNotConnect,
    NotGraphQL,
    GraphQLError(String),
    AuthNotEnforced,
    BadHeader,
    NotASubgraph,
    BadBoolean(&'static str),
    IntrospectionEnabled,
    InsecureSubgraph,
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
            Error::IntrospectionEnabled => write!(
                f,
                "Introspection is enabled for the GraphQL server but not allowed"
            ),
            Error::BadBoolean(name) => write!(f, "Input `{name}` can only be `true` or `false`"),
            Error::InsecureSubgraph => write!(f, "Subgraph is not protected by authentication"),
        }
    }
}

async fn basic_query(url: Arc<String>, auth: Auth) -> Result<(), Error> {
    let client = reqwest::Client::new();
    let request = client.post(url.as_str()).json(&json!({
        "query": "query{__typename}",
    }));
    let request = add_auth(auth, request)?;
    let body = get_json(request).await?;
    if let Some(Value::String(_)) = body.pointer("/data/__typename") {
        Ok(())
    } else {
        Err(Error::NotGraphQL)
    }
}

fn add_auth(auth: Auth, request: RequestBuilder) -> Result<RequestBuilder, Error> {
    if let Auth::Enabled { header } = auth {
        let (header_name, header_value) = header.split_once(':').ok_or(Error::BadHeader)?;
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
    use crate::Auth;

    pub const BASE_URL: &str = "https://graphql-test.up.railway.app";

    pub fn auth() -> Auth {
        const TOKEN: &str = env!("GRAPHQL_TOKEN");
        Auth::new(Some(format!("Authorization: Bearer {TOKEN}")))
    }
}

#[cfg(test)]
mod test_basic_query {
    use crate::Error::*;

    use super::test_utils::*;
    use super::*;

    #[tokio::test]
    async fn unauth_success() {
        let url = format!("{BASE_URL}/graphql");
        assert!(basic_query(Arc::new(url), Auth::Disabled).await.is_ok());
    }

    #[tokio::test]
    async fn success_subgraph() {
        let url = format!("{BASE_URL}/subgraph");
        assert!(basic_query(Arc::new(url), Auth::Disabled).await.is_ok());
    }

    #[tokio::test]
    async fn bad_url() {
        let url = BASE_URL.to_string();
        let url_without_scheme = url.split('/').nth(2).unwrap().to_string();
        assert_eq!(
            basic_query(Arc::new(url_without_scheme), Auth::Disabled).await,
            Err(BadUri)
        );
    }

    #[tokio::test]
    async fn not_found() {
        let url = "https://doesntexist.dylananthony.com";
        assert_eq!(
            basic_query(Arc::new(url.to_string()), Auth::Disabled).await,
            Err(CouldNotConnect)
        );
    }

    #[tokio::test]
    async fn post_not_accepted() {
        let url = format!("{BASE_URL}/no-post");
        assert_eq!(
            basic_query(Arc::new(url), Auth::Disabled).await,
            Err(BadStatus(StatusCode::METHOD_NOT_ALLOWED))
        );
    }

    #[tokio::test]
    async fn no_json_returned() {
        let url = format!("{BASE_URL}/no-json");
        assert_eq!(
            basic_query(Arc::new(url), Auth::Disabled).await,
            Err(NotGraphQL)
        );
    }

    #[tokio::test]
    async fn not_graphql() {
        let url = format!("{BASE_URL}/json");
        assert_eq!(
            basic_query(Arc::new(url), Auth::Disabled).await,
            Err(NotGraphQL)
        );
    }

    #[tokio::test]
    async fn auth_success() {
        let url = format!("{BASE_URL}/graphql-auth");
        assert_eq!(basic_query(Arc::new(url), auth()).await, Ok(()));
    }

    #[tokio::test]
    async fn subgraph_auth_success() {
        let url = format!("{BASE_URL}/subgraph-auth");
        assert!(basic_query(Arc::new(url), auth()).await.is_ok());
    }

    #[tokio::test]
    async fn auth_failure() {
        let url = format!("{BASE_URL}/graphql-auth");
        assert!(matches!(
            basic_query(
                Arc::new(url),
                Auth::new(Some(String::from("Authorization: Bearer nottherealtoken")))
            )
            .await,
            Err(GraphQLError(_))
        ));
    }

    #[tokio::test]
    async fn missing_auth() {
        let url = format!("{BASE_URL}/graphql-auth");
        match basic_query(Arc::new(url), Auth::Disabled).await {
            Err(BadStatus(StatusCode::BAD_REQUEST)) => (),
            other => panic!("Expected Err(GraphQLError(_)), got {:?}", other),
        }
    }
}

async fn check_subgraph(url: Arc<String>, auth: Auth) -> Result<(), Error> {
    let client = reqwest::Client::new();
    let request = client.post(url.as_str()).json(&json!({
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
    use crate::Error::NotASubgraph;

    use super::test_utils::*;
    use super::*;

    #[tokio::test]
    async fn happy() {
        let url = format!("{BASE_URL}/subgraph");
        check_subgraph(Arc::new(url), Auth::Disabled).await.unwrap();
    }

    #[tokio::test]
    async fn happy_with_auth() {
        let url = format!("{BASE_URL}/subgraph-auth");
        check_subgraph(Arc::new(url), auth()).await.unwrap();
    }

    #[tokio::test]
    async fn not_a_subgraph() {
        let url = format!("{BASE_URL}/graphql");
        assert_eq!(
            check_subgraph(Arc::new(url), Auth::Disabled).await,
            Err(NotASubgraph)
        );
    }
}

#[cfg(test)]
mod test_require_introspection_disabled {
    use crate::Error::IntrospectionEnabled;

    use super::test_utils::*;
    use super::*;

    #[tokio::test]
    async fn happy() {
        let url = format!("{BASE_URL}/graphql-no-introspection");
        require_introspection_disabled(Arc::new(url), Auth::Disabled)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn introspection_enabled() {
        let url = format!("{BASE_URL}/graphql");
        assert_eq!(
            require_introspection_disabled(Arc::new(url), Auth::Disabled).await,
            Err(IntrospectionEnabled)
        );
    }
}

async fn require_introspection_disabled(url: Arc<String>, auth: Auth) -> Result<(), Error> {
    let client = reqwest::Client::new();
    let request = client.post(url.as_str()).json(&json!({
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
