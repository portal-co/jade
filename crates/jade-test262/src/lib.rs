//! Test262 conformance runner for Jade — orchestration half.
//!
//! Implements `docs/test262-plan.md`: this crate discovers tests, parses their
//! frontmatter, compiles them through `jade-vm-frontend` (the to-bytecode pipeline under
//! test), JIT-compiles the bytecode through all three tiers, and dispatches execution to
//! the single Node driver (`packages/jade-js/test262/driver.ts`) that actually runs the
//! code against a tenant + primordial realm. Verdicts, skips, expectations and the
//! capability manifest follow the taxonomy in `docs/test262-plan.md`.

pub mod manifest;
pub mod meta;
pub mod model;
pub mod node;
pub mod pipeline;
pub mod run;
