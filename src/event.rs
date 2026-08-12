use serde::Deserialize;

use crate::side::Side;

// Mirrors the two message shapes documented in coinbase_feed.h's comments:
// the "channel" field picks which shape the rest of the object has.
#[derive(Debug, Deserialize)]
#[serde(tag = "channel")]
pub enum ChannelMessage {
    #[serde(rename = "l2_data")]
    L2Data {
        // sequence_num is captured but not yet read directly -- it's
        // the field flagged for future gap detection (a dropped
        // message currently desyncs the book silently). Deferred, not
        // forgotten -- see the snapshot_count comment in feed.rs for
        // the related discussion.
        #[allow(dead_code)]
        sequence_num: u64,
        #[allow(dead_code)]
        timestamp: String,
        events: Vec<L2Event>,
    },
    #[serde(rename = "subscriptions")]
    Subscriptions {
        #[allow(dead_code)]
        sequence_num: u64,
        #[allow(dead_code)]
        timestamp: String,
        events: Vec<SubscriptionEvent>,
    },
}

#[derive(Debug, Deserialize)]
pub struct L2Event {
    pub product_id: String,
    #[serde(rename = "type")]
    pub event_type: String, // "snapshot" | "update"
    pub updates: Vec<PriceLevelUpdate>,
}

#[derive(Debug, Deserialize)]
pub struct PriceLevelUpdate {
    pub side: Side,
    #[allow(dead_code)]
    pub event_time: String,
    pub price_level: String,  // string on the wire -- convert to fixed-point yourself
    pub new_quantity: String, // "0" means the price level should be removed
}

#[derive(Debug, Deserialize)]
pub struct SubscriptionEvent {
    pub subscriptions: SubscriptionDetail,
}

#[derive(Debug, Deserialize)]
pub struct SubscriptionDetail {
    pub level2: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_l2_data_snapshot() {
        // real message shape, confirmed against coinbase_feed.h's
        // comments and the live feed earlier in this project
        let json = r#"{
            "channel": "l2_data",
            "sequence_num": 0,
            "timestamp": "2026-02-27T13:54:08.898945082Z",
            "events": [{
                "type": "snapshot",
                "product_id": "BTC-USD",
                "updates": [
                    {"side": "bid", "event_time": "2026-02-27T13:54:08.557825Z", "price_level": "66244.35", "new_quantity": "0.00874179"}
                ]
            }]
        }"#;

        let msg: ChannelMessage = serde_json::from_str(json).unwrap();
        match msg {
            ChannelMessage::L2Data { events, .. } => {
                assert_eq!(events.len(), 1);
                assert_eq!(events[0].product_id, "BTC-USD");
                assert_eq!(events[0].event_type, "snapshot");
                assert_eq!(events[0].updates.len(), 1);
                assert_eq!(events[0].updates[0].side, Side::Bid);
                assert_eq!(events[0].updates[0].price_level, "66244.35");
                assert_eq!(events[0].updates[0].new_quantity, "0.00874179");
            }
            ChannelMessage::Subscriptions { .. } => panic!("expected L2Data"),
        }
    }

    #[test]
    fn deserializes_subscriptions_message() {
        let json = r#"{
            "channel": "subscriptions",
            "sequence_num": 1,
            "timestamp": "2026-02-27T13:54:08.898945082Z",
            "events": [{"subscriptions": {"level2": ["BTC-USD", "ETH-USD"]}}]
        }"#;

        let msg: ChannelMessage = serde_json::from_str(json).unwrap();
        match msg {
            ChannelMessage::Subscriptions { events, .. } => {
                assert_eq!(events.len(), 1);
                assert_eq!(
                    events[0].subscriptions.level2,
                    vec!["BTC-USD".to_string(), "ETH-USD".to_string()]
                );
            }
            ChannelMessage::L2Data { .. } => panic!("expected Subscriptions"),
        }
    }

    #[test]
    fn rejects_message_with_no_matching_channel() {
        let json = r#"{"channel": "heartbeats", "sequence_num": 0, "timestamp": "x", "events": []}"#;
        let result: Result<ChannelMessage, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }
}
