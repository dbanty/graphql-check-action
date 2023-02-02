use graphql_check_action::{run_checks, Auth, Error, Introspection, Subgraph};
use itertools::Itertools;
use std::env;
use std::fs::write;
use std::process::exit;

fn main() {
    let github_output_path = env::var("GITHUB_OUTPUT").unwrap();

    let args: Vec<String> = env::args().collect();
    let url = &args[1];
    let auth = match args[2].as_str() {
        "" => Auth::Disabled,
        header => Auth::Enabled { header },
    };
    let subgraph_input = &args[3];
    let allow_introspection = &args[4];
    let insecure_subgraph = &args[5];

    let mut errors = Vec::new();

    let subgraph_required = parse_boolean(subgraph_input, "subgraph").unwrap_or_else(|err| {
        errors.push(err);
        false
    });
    let allow_insecure_subgraph = parse_boolean(insecure_subgraph, "insecure_subgraph")
        .unwrap_or_else(|err| {
            errors.push(err);
            false
        });
    let subgraph = match (subgraph_required, allow_insecure_subgraph) {
        (true, true) => Subgraph::Insecure,
        (true, false) => Subgraph::Secure,
        (false, _) => Subgraph::NotASubgraph,
    };
    let introspection = match allow_introspection.as_str() {
        "true" => Introspection::Allow,
        "false" => Introspection::Disallow,
        "" => match subgraph {
            Subgraph::NotASubgraph => Introspection::Disallow,
            Subgraph::Secure | Subgraph::Insecure => Introspection::Allow,
        },
        _ => {
            errors.push(Error::BadBoolean("allow_introspection"));
            Introspection::Allow
        }
    };
    if let Some(errs) = run_checks(url, auth, subgraph, introspection).err() {
        errors.extend(errs)
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

fn parse_boolean(value: &str, name: &'static str) -> Result<bool, Error> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(Error::BadBoolean(name)),
    }
}
