use std::env;
use std::fs::write;
use std::process::exit;

use serde_json::{json, Value};

#[tokio::main]
async fn main() {
    let github_output_path = env::var("GITHUB_OUTPUT").unwrap();

    let args: Vec<String> = env::args().collect();
    let url = &args[1];
    if let Err(e) = check_basics(url).await {
        eprintln!("Error: {e}");
        write(github_output_path, format!("error={e}")).unwrap();
        exit(1);
    }
}

async fn check_basics(url: &str) -> Result<(), &'static str> {
    let client = reqwest::Client::new();
    let res = client
        .post(url)
        .json(&json!({
            "query": "query{__typename}",
        }))
        .send()
        .await
        .or(Err("Could not reach server"))?;
    let body: Value = res.json().await.or(Err("Could not parse response"))?;
    if body == json!({"data": {"__typename": "Query"}}) {
        Ok(())
    } else {
        Err("Server does not seem to be a GraphQL server")
    }
}
