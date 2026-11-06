//! Assets: the names users type, the markets they resolve to, exchange
//! metadata, and the protocol asset-id layout.

pub mod asset_id;
mod market;
mod metadata;
mod query;
mod resolver;

#[cfg(test)]
pub(crate) use market::test_support;
pub use market::{Market, OutcomeAsset, PerpAsset, SpotAsset};
pub use metadata::{
    AssetMetadata, CachedMetadata, METADATA_TTL, MetadataCache, SpotListing, load_spot_markets,
};
pub(crate) use metadata::{find_perp_dex, load_hip3_market};
pub use query::{AssetQuery, parse_asset_query, qualify_dex_asset};
pub use resolver::{AssetError, AssetResolver, suggestions};
