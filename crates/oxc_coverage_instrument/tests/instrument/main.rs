//! Behaviour tests for the `oxc_coverage_instrument` public API: the coverage
//! map and instrumented code emitted for every construct that carries a
//! counter, coverage pragmas, raw TypeScript input, source-map composition,
//! V8-to-Istanbul conversion, and the fixture and conformance corpora.

#[cfg(feature = "ast-api")]
mod ast_api;
mod callback_argument_names;
mod common;
mod compat_profile;
mod concise_arrow;
mod conformance;
mod conformance_suite;
mod error_display;
mod fixture_validation;
mod function_identity_overlay;
mod function_names;
mod if_consequent_location;
mod integration;
mod pragma;
mod real_world;
mod snapshot;
mod source_maps;
mod typescript_direct;
mod typescript_equivalence;
mod v8_to_istanbul;
