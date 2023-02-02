use std::fmt::Display;

use serde_json::Value::Object;
use serde_json::{json, Value};
use ureq::{Request, Response};

pub fn run_checks(
    url: &str,
    auth: Auth,
    subgraph: Subgraph,
    introspection: Introspection,
) -> Result<(), Vec<Error>> {
    let mut errors = Vec::new();

    let basic_err = basic_query(url, Auth::Disabled).err();
    let subgraph_err = check_subgraph(url, auth).err();

    let unauthed_err = if auth.is_enabled() {
        if let Some(authed_err) = basic_query(url, auth).err() {
            errors.push(authed_err);
        }
        match basic_err {
            Some(Error::GraphQLError(_) | Error::BadStatus(_)) => None,
            None => Some(Error::AuthNotEnforced),
            other_err => other_err,
        }
    } else {
        basic_err
    };
    if let Some(err) = unauthed_err {
        errors.push(err);
    }

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

    if let Introspection::Disallow = introspection {
        if let Err(e) = require_introspection_disabled(url, auth) {
            errors.push(e);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Auth<'a> {
    Enabled { header: &'a str },
    Disabled,
}

impl Auth<'_> {
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
    BadStatus(u16),
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

fn basic_query(url: &str, auth: Auth) -> Result<(), Error> {
    let response = make_request(url, auth)?.send_json(json!({
        "query": "query{__typename}",
    }));
    let body = get_json(response)?;
    if let Some(Value::String(_)) = body.pointer("/data/__typename") {
        Ok(())
    } else {
        Err(Error::NotGraphQL)
    }
}

fn make_request(url: &str, auth: Auth) -> Result<Request, Error> {
    let request = ureq::post(url);
    if let Auth::Enabled { header } = auth {
        let (header_name, header_value) = header.split_once(':').ok_or(Error::BadHeader)?;
        let header_value = header_value.trim();
        Ok(request.set(header_name, header_value))
    } else {
        Ok(request)
    }
}

fn get_json(response: Result<Response, ureq::Error>) -> Result<Value, Error> {
    let res = response.map_err(|err| match err {
        ureq::Error::Status(status, _) => Error::BadStatus(status),
        ureq::Error::Transport(t) => match t.kind() {
            ureq::ErrorKind::InvalidUrl | ureq::ErrorKind::UnknownScheme => Error::BadUri,
            _ => Error::CouldNotConnect,
        },
    })?;
    let body: Value = res.into_json().or(Err(Error::NotGraphQL))?;
    if let Some(obj) = body.get("errors") {
        Err(Error::GraphQLError(obj.to_string()))
    } else {
        Ok(body)
    }
}

#[cfg(test)]
mod test_utils {
    use crate::Auth;
    use const_format::formatcp;

    pub const BASE_URL: &str = "https://graphql-test.up.railway.app";
    const TOKEN: &str = env!("GRAPHQL_TOKEN");
    pub const AUTH: Auth<'static> = Auth::Enabled {
        header: formatcp!("Authorization: Bearer {}", TOKEN),
    };
}

#[cfg(test)]
mod test_basic_query {
    use crate::Error::*;

    use super::test_utils::*;
    use super::*;

    #[test]
    fn unauth_success() {
        let url = format!("{BASE_URL}/graphql");
        assert!(basic_query(&url, Auth::Disabled).is_ok());
    }

    #[test]
    fn success_subgraph() {
        let url = format!("{BASE_URL}/subgraph");
        assert!(basic_query(&url, Auth::Disabled).is_ok());
    }

    #[test]
    fn bad_url() {
        let url = BASE_URL.to_string();
        let url_without_scheme = url.split('/').nth(2).unwrap().to_string();
        assert_eq!(
            basic_query(&url_without_scheme, Auth::Disabled),
            Err(BadUri)
        );
    }

    #[test]
    fn not_found() {
        let url = "https://doesntexist.dylananthony.com";
        assert_eq!(basic_query(url, Auth::Disabled), Err(CouldNotConnect));
    }

    #[test]
    fn post_not_accepted() {
        let url = format!("{BASE_URL}/no-post");
        assert_eq!(basic_query(&url, Auth::Disabled), Err(BadStatus(405)));
    }

    #[test]
    fn no_json_returned() {
        let url = format!("{BASE_URL}/no-json");
        assert_eq!(basic_query(&url, Auth::Disabled), Err(NotGraphQL));
    }

    #[test]
    fn not_graphql() {
        let url = format!("{BASE_URL}/json");
        assert_eq!(basic_query(&url, Auth::Disabled), Err(NotGraphQL));
    }

    #[test]
    fn auth_success() {
        let url = format!("{BASE_URL}/graphql-auth");
        assert_eq!(basic_query(&url, AUTH), Ok(()));
    }

    #[test]
    fn subgraph_auth_success() {
        let url = format!("{BASE_URL}/subgraph-auth");
        assert!(basic_query(&url, AUTH).is_ok());
    }

    #[test]
    fn auth_failure() {
        let url = format!("{BASE_URL}/graphql-auth");
        assert!(matches!(
            basic_query(
                &url,
                Auth::Enabled {
                    header: "Authorization: Bearer nottherealtoken"
                }
            ),
            Err(GraphQLError(_))
        ));
    }

    #[test]
    fn missing_auth() {
        let url = format!("{BASE_URL}/graphql-auth");
        match basic_query(&url, Auth::Disabled) {
            Err(BadStatus(400)) => (),
            other => panic!("Expected Err(GraphQLError(_)), got {:?}", other),
        }
    }
}

fn check_subgraph(url: &str, auth: Auth) -> Result<(), Error> {
    let response = make_request(url, auth)?.send_json(json!({
        "query": "query{_service{sdl}}"
    }));
    if get_json(response).is_ok() {
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

    #[test]
    fn happy() {
        let url = format!("{BASE_URL}/subgraph");
        check_subgraph(&url, Auth::Disabled).unwrap();
    }

    #[test]
    fn happy_with_auth() {
        let url = format!("{BASE_URL}/subgraph-auth");
        check_subgraph(&url, AUTH).unwrap();
    }

    #[test]
    fn not_a_subgraph() {
        let url = format!("{BASE_URL}/graphql");
        assert_eq!(check_subgraph(&url, Auth::Disabled), Err(NotASubgraph));
    }
}

#[cfg(test)]
mod test_require_introspection_disabled {
    use crate::Error::IntrospectionEnabled;

    use super::test_utils::*;
    use super::*;

    #[test]
    fn happy() {
        let url = format!("{BASE_URL}/graphql-no-introspection");
        require_introspection_disabled(&url, Auth::Disabled).unwrap();
    }

    #[test]
    fn introspection_enabled() {
        let url = format!("{BASE_URL}/graphql");
        assert_eq!(
            require_introspection_disabled(&url, Auth::Disabled),
            Err(IntrospectionEnabled)
        );
    }
}

fn require_introspection_disabled(url: &str, auth: Auth) -> Result<(), Error> {
    let response = make_request(url, auth)?.send_json(json!({
        "query": "query{__schema{types{name}}}"
    }));
    match get_json(response) {
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
