# saleor-app-rust-axum

A boilerplate project for setting up a Saleor app using Rust with axum. It includes a file-based APL (if you only plan to support a single Saleor instance),
but you can easily add support for a more sophisticated APL backend (like sqlx) by implementing the `AplStore` trait.

If you plan to make multiple apps, you might want to abstract the Saleor module away into its own library.

## What is in here?

* Axum (as our webserver handler)
* Askama (as our template rendering)
  * Templates use:
    * Tailwind (as our CSS framework)
    * HTMX (as our web "framework")
    * Inter (as font)
* Basic handlers for Saleor (`/api/manifest` and `/api/register`)
* Saleor types in their own module (`src/saleor.rs` and `src/saleor/`)

This repository should easily get you started!

## The GraphQL schema doesn't work on my Saleor version! What do I do?

Install [the graphql-client](https://github.com/graphql-rust/graphql-client/tree/main/graphql_client_cli):

```sh
cargo install graphql_client_cli --force
```

and then download the schema by running the `introspect-schema` command:

```sh
graphql-client introspect-schema --output schema.graphql [YOUR_SALEOR_API_URL]
```

It is recommended that you download the schema in any case, I don't have the time to update it with each Saleor version and you might not run the latest Saleor version anyways. You WILL have to modify the queries if you use a different schema and the current queries aren't working anymore (though the compiler will tell you about that).
