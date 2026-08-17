use std::collections::HashMap;

use pyo3::prelude::*;
use pyo3::types::PyModule;
use serde::Deserialize;

const API_HOST: &str = "api.coinbase.com";

#[derive(Debug, Deserialize)]
pub struct ProductInfo {
    pub product_id: String,
    pub quote_increment: String,
    pub base_increment: String,
    #[allow(dead_code)]
    pub base_min_size: String,
    #[allow(dead_code)]
    pub base_max_size: String,
}

impl ProductInfo {
    /// Converts a price string as received on the WS feed (e.g.
    /// "63962.27") into an integer number of price ticks, using this
    /// product's quote_increment. ticks = round(price / increment).
    /// The general divide-and-round technique, correct even for
    /// non-decimal tick sizes.
    pub fn price_to_ticks(&self, price: &str) -> i64 {
        // quote_increment.parse::<f64>() returns Result<f64, ParseFloatError>.
        let increment: f64 = match self.quote_increment.parse() {
            Ok(v) => v,
            Err(e) => panic!("invalid quote_increment: {e}"),
        };

        // Same pattern, parsing the price argument instead of a field.
        let price: f64 = match price.parse() {
            Ok(v) => v,
            Err(e) => panic!("invalid price: {e}"),
        };

        (price / increment).round() as i64
    }

    /// Same technique above using expect, for size/quantity via base_increment.
    pub fn size_to_ticks(&self, qty: &str) -> i64 {
        let increment: f64 = self.base_increment.parse().expect("invalid base_increment");
        let qty: f64 = qty.parse().expect("invalid quantity");
        (qty / increment).round() as i64
    }

    /// Inverse of price_to_ticks -- ticks back to a display-ready f64.
    pub fn ticks_to_price(&self, ticks: i64) -> f64 {
        let increment: f64 = self
            .quote_increment
            .parse()
            .expect("invalid quote_increment");
        ticks as f64 * increment
    }

    /// Inverse of size_to_ticks.
    pub fn ticks_to_size(&self, ticks: i64) -> f64 {
        let increment: f64 = self.base_increment.parse().expect("invalid base_increment");
        ticks as f64 * increment
    }

    /// How many decimal places to display prices with for this product,
    /// derived from quote_increment itself (e.g. "0.01" -> 2) rather
    /// than a fixed number
    pub fn price_decimal_places(&self) -> usize {
        decimal_places(&self.quote_increment)
    }

    /// Same idea for size/quantity, via base_increment.
    pub fn size_decimal_places(&self) -> usize {
        decimal_places(&self.base_increment)
    }

    /// Splits "BTC-USD" into ("BTC", "USD") for display headers
    pub fn base_and_quote_currency(&self) -> (&str, &str) {
        match self.product_id.split_once('-') {
            Some((base, quote)) => (base, quote),
            None => (self.product_id.as_str(), ""),
        }
    }
}

/// Counts digits after the decimal point in a Coinbase increment
/// string, e.g. "0.01" -> 2, "0.00000001" -> 8, "1" -> 0. Trailing
/// zeros are trimmed first so "0.10" -> 1, not 2.
fn decimal_places(increment: &str) -> usize {
    match increment.split_once('.') {
        Some((_, frac)) => frac.trim_end_matches('0').len(),
        None => 0,
    }
}

/// Calls coinbase.jwt_generator.format_jwt_uri() + build_rest_jwt() --
/// same sanctioned Python generator as build_ws_jwt in subscription.rs,
/// just the REST variant. REST JWTs carry a "uri" claim identifying the
/// specific request (e.g. "GET api.coinbase.com/api/v3/brokerage/...")
fn build_rest_jwt(method: &str, path: &str, api_key: &str, api_secret: &str) -> PyResult<String> {
    Python::attach(|py| {
        let jwt_generator = PyModule::import(py, "coinbase.jwt_generator")?;

        let uri: String = jwt_generator
            .getattr("format_jwt_uri")?
            .call1((method, path))?
            .extract()?;

        let jwt: String = jwt_generator
            .getattr("build_rest_jwt")?
            .call1((uri, api_key, api_secret))?
            .extract()?;

        Ok(jwt)
    })
}

/// Fetch the raw product JSON for one product_id and return it as a
/// string
pub fn fetch_product_raw(product_id: &str, secrets_dir: &str) -> String {
    let api_key = std::fs::read_to_string(format!("{secrets_dir}/api_key.txt"))
        .expect("failed to read api_key.txt")
        .trim()
        .to_string();
    let api_secret = std::fs::read_to_string(format!("{secrets_dir}/api_secret.pem"))
        .expect("failed to read api_secret.pem");

    let path = format!("/api/v3/brokerage/products/{product_id}");
    let jwt = build_rest_jwt("GET", &path, &api_key, &api_secret)
        .expect("failed to build REST JWT via coinbase.jwt_generator");

    let url = format!("https://{API_HOST}{path}");

    ureq::get(&url)
        .header("Authorization", &format!("Bearer {jwt}"))
        .call()
        .expect("REST request failed")
        .body_mut()
        .read_to_string()
        .expect("failed to read response body")
}

/// Fetch_product_raw + parse into ProductInfo. Kept separate from
/// fetch_product_raw rather than replacing it -- the raw version is
/// still useful standalone for inspecting fields ProductInfo doesn't
/// capture.
pub fn fetch_product(product_id: &str, secrets_dir: &str) -> ProductInfo {
    let raw = fetch_product_raw(product_id, secrets_dir);
    serde_json::from_str(&raw).expect("failed to parse product JSON")
}

/// Per-symbol price/size precision, built once at startup from
/// ProductInfo for every subscribed product. feed::handle_message looks
/// up a product's entry here (by product_id) before converting a
/// PriceLevelUpdate's price_level/new_quantity strings into ticks for
/// PriceBook::book_add.
pub type ProductInfoTable = HashMap<String, ProductInfo>;

/// Builds the lookup table above.
pub fn build_product_info_table(symbols: &[String], secrets_dir: &str) -> ProductInfoTable {
    let mut table = ProductInfoTable::new();
    for symbol in symbols {
        let product_info = fetch_product(symbol, secrets_dir);
        table.insert(symbol.clone(), product_info);
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    fn btc_usd() -> ProductInfo {
        ProductInfo {
            product_id: "BTC-USD".to_string(),
            quote_increment: "0.01".to_string(),
            base_increment: "0.00000001".to_string(),
            base_min_size: "0.00000001".to_string(),
            base_max_size: "3400".to_string(),
        }
    }

    #[test]
    fn price_to_ticks_matches_known_value() {
        // real (price, expected ticks) pair confirmed against the live
        // feed earlier in this project
        let product = btc_usd();
        assert_eq!(product.price_to_ticks("63962.27"), 6396227);
    }

    #[test]
    fn size_to_ticks_matches_known_value() {
        let product = btc_usd();
        assert_eq!(product.size_to_ticks("0.01874522"), 1874522);
    }

    #[test]
    fn ticks_to_price_is_inverse_of_price_to_ticks() {
        let product = btc_usd();
        let ticks = product.price_to_ticks("63962.27");
        assert!((product.ticks_to_price(ticks) - 63962.27).abs() < 1e-9);
    }

    #[test]
    fn ticks_to_size_is_inverse_of_size_to_ticks() {
        let product = btc_usd();
        let ticks = product.size_to_ticks("0.01874522");
        assert!((product.ticks_to_size(ticks) - 0.01874522).abs() < 1e-9);
    }

    #[test]
    fn price_and_size_decimal_places() {
        let product = btc_usd();
        assert_eq!(product.price_decimal_places(), 2);
        assert_eq!(product.size_decimal_places(), 8);
    }

    #[test]
    fn base_and_quote_currency_splits_product_id() {
        let product = btc_usd();
        assert_eq!(product.base_and_quote_currency(), ("BTC", "USD"));
    }

    #[test]
    fn decimal_places_counts_digits_after_decimal_point() {
        assert_eq!(decimal_places("0.01"), 2);
        assert_eq!(decimal_places("0.00000001"), 8);
        assert_eq!(decimal_places("1"), 0);
    }

    #[test]
    fn decimal_places_trims_trailing_zeros() {
        assert_eq!(decimal_places("0.10"), 1);
    }
}
