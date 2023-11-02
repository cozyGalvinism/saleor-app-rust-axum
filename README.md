# saleor-app-rust-axum

A boilerplate project for setting up a Saleor app using Rust with axum.

Does not come with an [APL](https://docs.saleor.io/docs/3.x/developer/extending/apps/developing-apps/app-sdk/apl#example-implementation),
but implementing one is just as simple as Saleor describes it. You might not even need a proper one!

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
