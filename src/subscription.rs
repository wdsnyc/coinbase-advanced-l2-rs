use pyo3::prelude::*;
use pyo3::types::PyModule;
use serde::Serialize;

#[derive(Serialize)]
struct SubscribeMessage {
    #[serde(rename = "type")]
    msg_type: String,
    product_ids: Vec<String>,
    channel: String,
    jwt: String,
}

/// Calls the same coinbase.jwt_generator.build_ws_jwt() using PyO3.
/// Python::attach owns interpreter/GIL lifetime for the duration of
/// the closure.
fn build_ws_jwt(api_key: &str, api_secret: &str) -> PyResult<String> {
    Python::attach(|py| {
        let jwt_generator = PyModule::import(py, "coinbase.jwt_generator")?;
        let jwt: String = jwt_generator
            .getattr("build_ws_jwt")?
            .call1((api_key, api_secret))?
            .extract()?;
        Ok(jwt)
    })
}

pub fn get_subscribe_msg(symbols: &[String], secrets_dir: &str) -> String {
    let api_key = std::fs::read_to_string(format!("{secrets_dir}/api_key.txt"))
        .expect("failed to read api_key.txt")
        .trim()
        .to_string();
    let api_secret = std::fs::read_to_string(format!("{secrets_dir}/api_secret.pem"))
        .expect("failed to read api_secret.pem");

    let jwt = build_ws_jwt(&api_key, &api_secret)
        .expect("failed to build JWT via coinbase.jwt_generator");

    let msg = SubscribeMessage {
        msg_type: "subscribe".to_string(),
        product_ids: symbols.to_vec(),
        channel: "level2".to_string(),
        jwt,
    };

    serde_json::to_string(&msg).expect("failed to serialize subscribe message")
}
