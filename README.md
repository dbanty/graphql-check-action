# GraphQL Check

This action checks your GraphQL server health after deployment. Specifically, it will check:

1. The endpoint is reachable
2. Introspection is disabled (for non-federated graphs)
3. Authentication is required to make _any_ query
4. If this is a [federation subgraph], the subgraph contains required Federation elements

### Inputs

| Name                  | Description                                                                                                                          | Default             |
|-----------------------|--------------------------------------------------------------------------------------------------------------------------------------|---------------------|
| `endpoint`            | The full URL, including scheme (e.g., `https://`) of the GraphQL endpoint                                                            | None                |
| `auth`                | The full header to be included. Providing a value enables the "authentication required" check                                        | None                |
| `subgraph`            | Whether the endpoint is expected to be a [Federation subgraph]                                                                       | `false`             |
| `allow_introspection` | Whether the GraphQL server should have introspection enabled. This [should be disabled for non-subgraphs][introspection explanation] | value of `subgraph` |
| `insecure_subgraph`   | Whether it is acceptable for your `auth` to be empty when `subgraph` is `true`. You generally [don't want this][subgraph security]   | `false`             |

## Tests

Here are all the tests that will run, and the config values that affect them.

### Endpoint reachable

This action will always fail if making an HTTP POST request to the provided endpoint fails. The request will contain this query:

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

If the `auth` parameter is provided, that header will be included in the request.

### Introspection disabled

Generally speaking, [introspection should be disabled for non-subgraphs][introspection explanation]. As such, by default this action will fail if the graph is not a [federated subgraph] (checked dynamically) and the server responds with some content to the following query:

```graphql
query {
    __schema {
        types {
            name
        }
    }
}
```

If `__schema` in the response is `null`, this action will pass. You can bypass this check by setting `allow_introspection` to `true`.

### Authentication enforced

If the `auth` input is provided, this action will fail if the GraphQL server responds successfully **any** query without the provided authentication. If the GraphQL server response with a non-200 status code _or_ a GraphQL error, this action will pass.

If subgraph features are detected (by running the "Subgraph compatibility" check), but `auth` is not provided, this check will still fail, as an insecure subgraph is [usually a mistake][subgraph security]. If you need a public, insecure subgraph, you can provide the input `insecure_subgraph: true`.

### Subgraph compatibility

If the `subgraph` input is set to `true`, this action will require that the endpoint is a [federation subgraph]. Specifically, it must return something for `sdl` in this query:

```graphql
query {
    _service {
        sdl
    }
}
```

## Examples

### Standard GraphQL Server

Introspection is disabled and authentication is required for all operations.

```yaml
name: Deploy
on:
  push:
    branches:
      - main
jobs:
  deploy:
    steps:
      - name: Deploy your server
      - name: Wait for deploy to finish
  check_graphql:
    runs-on: ubuntu-latest
    needs: deploy
    steps:
      - uses: actions/checkout@v3
      - uses: dbanty/check-graphql-action@v1
        with:
          endpoint: ${{ vars.PRODUCTION_ENDPOINT }}
          auth: "Authorization: Bearer ${{ secrets.TEST_TOKEN }}"
```

### Public GraphQL Server

While authentication may be required for operations, anyone is allowed to introspect the server and start building queries.

```yaml
name: Deploy
on:
  push:
    branches:
      - main
jobs:
  deploy:
    steps:
      - name: Deploy your server
      - name: Wait for deploy to finish
  check_graphql:
    runs-on: ubuntu-latest
    needs: deploy
    steps:
      - uses: actions/checkout@v3
      - uses: dbanty/check-graphql-action@v1
        with:
          endpoint: ${{ vars.PRODUCTION_ENDPOINT }}
          allow_introspection: true
```

### Federated subgraph

This is the recommended setup for a federated subgraph which, generally speaking, should not be accessible to anything except the router.

```yaml
name: Deploy
on:
  push:
    branches:
      - main
jobs:
  deploy:
    steps:
      - name: Deploy your server
      - name: Wait for deploy to finish
  check_graphql:
    runs-on: ubuntu-latest
    needs: deploy
    steps:
      - uses: actions/checkout@v3
      - uses: dbanty/check-graphql-action@v1
        with:
          endpoint: ${{ vars.PRODUCTION_ENDPOINT }}
          auth: "Gateway-Authorization: Bearer ${{ secrets.AUTH_TOKEN }}"
          subgraph: true
```

[federation subgraph]: https://www.apollographql.com/docs/federation/building-supergraphs/subgraphs-overview#subgraph-specific-fields
[introspection explanation]: https://www.apollographql.com/blog/graphql/security/why-you-should-disable-graphql-introspection-in-production/#what-is-it
[subgraph security]: https://www.apollographql.com/docs/technotes/TN0021-graph-security/#only-allow-the-router-to-query-subgraphs-directly