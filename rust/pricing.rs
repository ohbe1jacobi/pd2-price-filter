//! Sequential, per-name PD2 Trader prices using the original rolling window.
use crate::types::Price;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Hardcore,
    Softcore,
}

pub fn parse_average(data: &Value) -> Result<Option<Price>, String> {
    let object = data
        .as_object()
        .ok_or("average response must be an object")?;
    let median_samples = match object.get("sampleCount") {
        None | Some(Value::Null) => 0,
        Some(value) => value
            .as_u64()
            .ok_or("sampleCount must be a nonnegative integer")?,
    };
    let Some(value) = object.get("medianPrice").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let median = value
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0)
        .ok_or("medianPrice must be finite and nonnegative")?;
    Ok(Some(Price {
        median,
        median_samples,
    }))
}

const AVERAGE_URL: &str = "https://pd2trader.com/item-prices/average";
const REQUEST_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);
const MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;

pub struct Client {
    agent: ureq::Agent,
    #[cfg(test)]
    base_override: Option<String>,
}
impl Client {
    pub fn new() -> Result<Self, String> {
        let config = ureq::Agent::config_builder()
            .https_only(true)
            .timeout_global(Some(REQUEST_DEADLINE))
            .http_status_as_error(false)
            .user_agent(concat!(
                "pd2-price-filter/",
                env!("CARGO_PKG_VERSION"),
                " (runtime price and base-code discovery)"
            ))
            .build();
        Ok(Self {
            agent: config.into(),
            #[cfg(test)]
            base_override: None,
        })
    }
    pub fn fetch_average(&self, name: &str, mode: Mode) -> Result<Option<Price>, String> {
        let params = [
            ("itemName", name),
            (
                "isHardcore",
                if mode == Mode::Hardcore {
                    "true"
                } else {
                    "false"
                },
            ),
            ("isLadder", "true"),
            ("hours", "720"),
        ];
        match self.request_json(AVERAGE_URL, &params, true)? {
            None => Ok(None),
            Some(data) => {
                parse_average(&data).map_err(|e| format!("pd2trader average {name}: {e}"))
            }
        }
    }
    pub(crate) fn get_json(&self, url: &str, params: &[(&str, &str)]) -> Result<Value, String> {
        self.request_json(url, params, false)?
            .ok_or_else(|| format!("{url}: missing response"))
    }
    fn request_json(
        &self,
        url: &str,
        params: &[(&str, &str)],
        no_data_404: bool,
    ) -> Result<Option<Value>, String> {
        #[cfg(test)]
        let overridden = self.base_override.as_ref().map(|base| {
            let path = url
                .split_once("://")
                .unwrap()
                .1
                .split_once('/')
                .map(|(_, p)| p)
                .unwrap_or("");
            format!("{base}/{path}")
        });
        #[cfg(test)]
        let url = overridden.as_deref().unwrap_or(url);
        let mut request = self.agent.get(url).header("Accept", "application/json");
        for &(key, value) in params {
            request = request.query(key, value);
        }
        let mut response = request
            .call()
            .map_err(|e| format!("{url}: request failed: {e}"))?;
        let status = response.status().as_u16();
        if status == 404 && no_data_404 {
            return Ok(None);
        }
        if !(200..300).contains(&status) {
            return Err(format!("{url}: HTTP {status}"));
        }
        let bytes = response
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE_BYTES)
            .read_to_vec()
            .map_err(|e| {
                format!("{url}: response read failed (limit {MAX_RESPONSE_BYTES} bytes): {e}")
            })?;
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| format!("{url}: invalid JSON: {e}"))
    }
    #[cfg(test)]
    pub(crate) fn fixture(base: &str, deadline: std::time::Duration) -> Self {
        assert!(base.starts_with("http://127.0.0.1:"));
        let config = ureq::Agent::config_builder()
            .https_only(false)
            .proxy(None)
            .timeout_global(Some(deadline))
            .http_status_as_error(false)
            .user_agent(concat!(
                "pd2-price-filter/",
                env!("CARGO_PKG_VERSION"),
                " (offline fixture)"
            ))
            .build();
        Self {
            agent: config.into(),
            base_override: Some(base.into()),
        }
    }
}

#[cfg(test)]
pub(crate) mod test_http {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
        thread,
        time::Duration,
    };
    /// A real loopback HTTP server answering one request. Never reaches the public network.
    pub(crate) fn serve(
        status: u16,
        body: String,
        delay: Duration,
    ) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let (base, requests, handle) = serve_seq(vec![(status, body)], delay);
        (base, requests, handle)
    }

    /// Serves `responses` in order, one connection each, and records every request line.
    pub(crate) fn serve_seq(
        responses: Vec<(u16, String)>,
        delay: Duration,
    ) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            for (status, body) in responses {
                let until = std::time::Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                && std::time::Instant::now() < until =>
                        {
                            thread::sleep(Duration::from_millis(2))
                        }
                        Err(e) => panic!("fixture accept: {e}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    if stream.read(&mut byte).unwrap() == 0 {
                        break;
                    }
                    request.push(byte[0]);
                }
                let _ = tx.send(String::from_utf8(request).unwrap());
                thread::sleep(delay);
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        (base, rx, handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn http_average_uses_per_name_query_and_distinguishes_no_data_from_failure() {
        use std::time::Duration;
        for mode in [Mode::Hardcore, Mode::Softcore] {
            for (status, body) in [
                (200, r#"{"medianPrice":2,"sampleCount":1}"#),
                (404, "no data"),
                (500, "outage"),
                (200, "not json"),
                (200, r#"{"medianPrice":1,"sampleCount":-1}"#),
            ] {
                let (base, request, server) = test_http::serve(status, body.into(), Duration::ZERO);
                let result = Client::fixture(&base, Duration::from_secs(2))
                    .fetch_average("A & B+雪's", mode);
                match (status, body) {
                    (404, _) => assert_eq!(result.unwrap(), None),
                    (200, b) if b.contains("sampleCount\":1}") => {
                        assert_eq!(result.unwrap().unwrap().median, 2.0)
                    }
                    _ => assert!(result.is_err()),
                }
                let request = request.recv_timeout(Duration::from_secs(2)).unwrap();
                assert!(
                    request.starts_with("GET /item-prices/average?"),
                    "{request}"
                );
                assert!(
                    request.contains("itemName=A%20%26%20B%2B%E9%9B%AA%27s"),
                    "{request}"
                );
                assert!(request.contains(if mode == Mode::Hardcore {
                    "isHardcore=true"
                } else {
                    "isHardcore=false"
                }));
                assert!(request.contains("isLadder=true"));
                assert!(request.contains("hours=720"));
                assert!(
                    request
                        .to_ascii_lowercase()
                        .contains("user-agent: pd2-price-filter/")
                );
                server.join().unwrap();
            }
        }
    }
    #[test]
    fn http_average_fails_on_500_malformed_body_and_deadline() {
        use std::time::Duration;
        for (status, body) in [
            (500, "outage"),
            (503, ""),
            (200, ""),
            (200, "{"),
            (200, "[]"),
        ] {
            let (base, _, server) = test_http::serve(status, body.into(), Duration::ZERO);
            assert!(
                Client::fixture(&base, Duration::from_secs(2))
                    .fetch_average("Shako", Mode::Softcore)
                    .is_err(),
                "accepted {status} {body}"
            );
            server.join().unwrap();
        }
        let (base, _, server) = test_http::serve(
            200,
            r#"{"medianPrice":1}"#.into(),
            Duration::from_millis(400),
        );
        let started = std::time::Instant::now();
        assert!(
            Client::fixture(&base, Duration::from_millis(50))
                .fetch_average("Shako", Mode::Softcore)
                .is_err()
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        server.join().unwrap();
    }
    #[test]
    fn average_rejects_invalid_shapes_numbers_and_counts() {
        for value in [
            json!(null),
            json!([]),
            json!("bad"),
            json!({"medianPrice":-1}),
            json!({"medianPrice":"2"}),
            json!({"medianPrice":true}),
            json!({"medianPrice":1,"sampleCount":-1}),
            json!({"medianPrice":1,"sampleCount":1.5}),
            json!({"medianPrice":1,"sampleCount":"2"}),
            json!({"sampleCount":false}),
        ] {
            assert!(parse_average(&value).is_err(), "accepted {value}");
        }
    }
    #[test]
    fn average_matches_original_mapping_and_defaults() {
        assert_eq!(
            parse_average(&json!({"medianPrice":0.1,"sampleCount":490})).unwrap(),
            Some(Price {
                median: 0.1,
                median_samples: 490
            })
        );
        assert_eq!(parse_average(&json!({})).unwrap(), None);
        assert_eq!(parse_average(&json!({"medianPrice":null})).unwrap(), None);
        assert_eq!(
            parse_average(&json!({"medianPrice":2}))
                .unwrap()
                .unwrap()
                .median_samples,
            0
        );
        assert_eq!(
            parse_average(&json!({"medianPrice":0,"sampleCount":null}))
                .unwrap()
                .unwrap()
                .median_samples,
            0
        );
        assert_eq!(Mode::default(), Mode::Hardcore);
        assert_eq!(
            serde_json::to_string(&Mode::Hardcore).unwrap(),
            "\"hardcore\""
        );
    }
}
