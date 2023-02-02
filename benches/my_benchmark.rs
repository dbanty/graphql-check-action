use criterion::{black_box, criterion_group, criterion_main, Criterion};
use graphql_check_action::{run_checks, Auth, Introspection, Subgraph};
use std::sync::Arc;

fn criterion_benchmark(c: &mut Criterion) {
    const BASE_URL: &str = "https://graphql-test.up.railway.app";
    const TOKEN: &str = env!("GRAPHQL_TOKEN");

    let auth = Auth::Enabled {
        header: Arc::new(format!("Authorization: Bearer {TOKEN}")),
    };
    let runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("simple_public_server", |b| {
        let url = format!("{BASE_URL}/graphql");
        b.to_async(&runtime).iter(|| {
            run_checks(
                black_box(&url),
                black_box(Auth::Disabled),
                black_box(Subgraph::NotASubgraph),
                black_box(Introspection::Allow),
            )
        })
    });

    c.bench_function("standard_graphql_server", |b| {
        let url = format!("{BASE_URL}/graphql-auth");
        b.to_async(&runtime).iter(|| {
            run_checks(
                black_box(&url),
                black_box(auth.clone()),
                black_box(Subgraph::NotASubgraph),
                black_box(Introspection::Disallow),
            )
        })
    });

    c.bench_function("subgraph_server", |b| {
        let url = format!("{BASE_URL}/subgraph-auth");
        b.to_async(&runtime).iter(|| {
            run_checks(
                black_box(&url),
                black_box(auth.clone()),
                black_box(Subgraph::Secure),
                black_box(Introspection::Allow),
            )
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
