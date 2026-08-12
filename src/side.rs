use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Bid,
    Offer,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_bid() {
        let side: Side = serde_json::from_str("\"bid\"").unwrap();
        assert_eq!(side, Side::Bid);
    }

    #[test]
    fn deserializes_offer() {
        let side: Side = serde_json::from_str("\"offer\"").unwrap();
        assert_eq!(side, Side::Offer);
    }

    #[test]
    fn rejects_unrecognized_value() {
        let result: Result<Side, _> = serde_json::from_str("\"buy\"");
        assert!(result.is_err());
    }
}
