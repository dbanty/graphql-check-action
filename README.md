# GraphQL Check

This action checks your GraphQL server health after deployment. Specifically, it will check:

1. The endpoint is reachable
2. Introspection is disabled (for non-federated graphs)
3. (Optional) Authentication is required to make queries
4. If subgraph:
    1. (Optional) The subgraph contains required Federation elements

## Usage

```yaml
name: Deploy
on:
  push:
    branches:
      - main
jobs:
  check_graphql:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: dbanty/check-graphql-action@v1
        with:
          endpoint: ${{ vars.PRODUTION_ENDPOINT }}
          auth: "Gateway-Authorization: Bearer ${{ secrets.AUTH_TOKEN }}"  # If not included, auth is not checked
          subgraph: true  # defaults to false
          allow_introspection: true  # Defaults to the value of subgraph
          insecure_subgraph: false  # Defaults to false
```

### Inputs

| Name                  | Description                                                                                                                                                                                                                            | Default             |
|-----------------------|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|---------------------|
| `endpoint`            | The full URL, including scheme (e.g., `https://`) of the GraphQL endpoint                                                                                                                                                              | None                |
| `auth`                | The full header to be included. Providing a value enables the "authentication required" check                                                                                                                                          | None                |
| `subgraph`            | Whether the endpoint is expected to be a [Federation subgraph](https://www.apollographql.com/docs/federation/building-supergraphs/subgraphs-overview#subgraph-specific-fields)                                                         | `false`             |
| `allow_introspection` | Whether the GraphQL server should have introspection enabled. This [should be disabled for non-subgraphs](https://www.apollographql.com/blog/graphql/security/why-you-should-disable-graphql-introspection-in-production/#what-is-it)  | value of `subgraph` |
| `insecure_subgraph`   | Whether it is acceptable for your `auth` to be empty when `subgraph` is `true`. You generally [don't want this](https://www.apollographql.com/docs/technotes/TN0021-graph-security/#only-allow-the-router-to-query-subgraphs-directly) | `false`             |

## Endpoint reachable

This action will fail if making an HTTP POST request to the provided endpoint fails. The request will contain this
query:

```graphql
query {
    __typename
}
```

It expects this response:

```json
{
  "data": {
    "__typename": "Query"
  }
}
```

## Introspection disabled

Unless the `subgraph` input is set to `true`, this action will fail if the GraphQL server responds to an introspection
query. The query will be:

```graphql
query {
    __schema {
        types {
            name
        }
    }
}
```

The GraphQL server must respond with an error for this check to succeed.

## Authentication required

If the `auth` input is provided, this action will fail if the GraphQL server responds to **any** query without the
provided authentication.

## Subgraph

If the `subgraph` input is set to `true`, this action will check that the subgraph contains the required Federation
elements. Specifically, it will run the query:

```graphql
query {
    _service {
        sdl
    }
}
```

**NOTE**: If `subgraph` is `true` and `auth` is not provided, this action will fail—as insecure subgraphs are usually a
mistake. If you need a public, insecure subgraph, you can provide the input `insecure_subgraph: true`.
