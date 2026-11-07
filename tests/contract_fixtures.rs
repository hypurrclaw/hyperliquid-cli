//! The fixtures derived from the command registry, checked in one test.
//!
//! `HYPERLIQUID_UPDATE_CONTRACTS=1 cargo test --test contract_fixtures` rewrites
//! all of them from the one generator in `contract_support`.

mod contract_support;

use contract_support::{assert_json_fixture, registry_fixtures};

#[test]
fn registry_fixtures_match_the_registry() {
    for (name, fixture) in registry_fixtures() {
        assert_json_fixture(name, &fixture);
    }
}
