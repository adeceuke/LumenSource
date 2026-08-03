use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use zeroize::Zeroizing;

#[derive(Clone)]
pub(crate) struct SharedModel {
    pub entry_id: String,
    pub display_name: String,
    pub public_name: String,
    pub backend_model: String,
    pub endpoint: String,
}

#[derive(Clone)]
struct GatewayState {
    token: Arc<Zeroizing<String>>,
    models: Arc<Vec<SharedModel>>,
    client: Client,
}

pub(crate) struct SharingServer {
    pub address: String,
    stop: Option<oneshot::Sender<()>>,
}

impl SharingServer {
    pub fn stop(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExposedModelStatus {
    pub entry_id: String,
    pub display_name: String,
    pub public_name: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharingStatus {
    pub enabled: bool,
    pub running: bool,
    pub allow_other_devices: bool,
    pub token_saved: bool,
    pub address: Option<String>,
    pub exposed_models: Vec<String>,
    pub exposed_model_details: Vec<ExposedModelStatus>,
    pub transport_warning: Option<String>,
}

pub(crate) async fn start(
    port: u16,
    allow_other_devices: bool,
    token: Zeroizing<String>,
    models: Vec<SharedModel>,
) -> Result<SharingServer, String> {
    if models.is_empty() {
        return Err("Select at least one managed model before enabling sharing.".to_owned());
    }
    let ip = if allow_other_devices {
        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
    } else {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    };
    let listener = TcpListener::bind(SocketAddr::new(ip, port))
        .await
        .map_err(|error| format!("Could not bind the sharing gateway on port {port}: {error}"))?;
    let bound_port = listener
        .local_addr()
        .map_err(|error| format!("Could not read the sharing gateway address: {error}"))?
        .port();
    let state = GatewayState {
        token: Arc::new(token),
        models: Arc::new(models),
        client: Client::new(),
    };
    let router = Router::new()
        .route("/v1/models", get(list_models))
        .route("/v1/chat/completions", post(forward_chat))
        .route("/v1/embeddings", post(forward_embeddings))
        .layer(DefaultBodyLimit::max(4 * 1024 * 1024))
        .with_state(state);
    let (stop, stopped) = oneshot::channel();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await;
    });
    let host = if allow_other_devices {
        local_network_address()
            .map(|address| address.to_string())
            .unwrap_or_else(|| "<this-machine>".to_owned())
    } else {
        "127.0.0.1".to_owned()
    };
    Ok(SharingServer {
        address: format!("http://{host}:{bound_port}/v1"),
        stop: Some(stop),
    })
}

async fn list_models(State(state): State<GatewayState>, headers: HeaderMap) -> Response {
    if !authorized(&headers, state.token.as_str()) {
        return unauthorized();
    }
    let data = state
        .models
        .iter()
        .map(|model| {
            json!({
                "id": model.public_name,
                "object": "model",
                "owned_by": "lumen-source",
                "lumen_source_entry_id": model.entry_id,
                "display_name": model.display_name,
            })
        })
        .collect::<Vec<_>>();
    Json(json!({"object": "list", "data": data})).into_response()
}

async fn forward_chat(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    forward(&state, &headers, body, "chat/completions").await
}

async fn forward_embeddings(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    forward(&state, &headers, body, "embeddings").await
}

async fn forward(
    state: &GatewayState,
    headers: &HeaderMap,
    mut body: Value,
    path: &str,
) -> Response {
    if !authorized(headers, state.token.as_str()) {
        return unauthorized();
    }
    let Some(public_name) = body.get("model").and_then(Value::as_str) else {
        return api_error(
            StatusCode::BAD_REQUEST,
            "The request must name an exposed model.",
        );
    };
    let Some(model) = state
        .models
        .iter()
        .find(|model| model.public_name == public_name)
    else {
        return api_error(
            StatusCode::NOT_FOUND,
            "That model is not exposed by Lumen Source.",
        );
    };
    body["model"] = Value::String(model.backend_model.clone());
    let endpoint = format!("{}/v1/{path}", model.endpoint.trim_end_matches('/'));
    let response = match state.client.post(endpoint).json(&body).send().await {
        Ok(response) => response,
        Err(_) => {
            return api_error(
                StatusCode::BAD_GATEWAY,
                "The selected local model service did not respond.",
            )
        }
    };
    let status = response.status();
    let response_headers = response.headers().clone();
    let mut forwarded = Response::new(Body::from_stream(response.bytes_stream()));
    *forwarded.status_mut() = status;
    for name in [header::CONTENT_TYPE, header::CACHE_CONTROL] {
        if let Some(value) = response_headers.get(&name).cloned() {
            forwarded.headers_mut().insert(name, value);
        }
    }
    forwarded
}

fn authorized(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|provided| provided.as_bytes() == expected.as_bytes())
}

fn unauthorized() -> Response {
    let mut response = api_error(
        StatusCode::UNAUTHORIZED,
        "A valid Lumen Source API token is required.",
    );
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        header::HeaderValue::from_static("Bearer"),
    );
    response
}

fn api_error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error": {"message": message}}))).into_response()
}

fn local_network_address() -> Option<IpAddr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    Some(socket.local_addr().ok()?.ip())
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::time::Duration;

    use axum::body::Bytes;
    use futures_util::stream;

    use super::*;

    #[test]
    fn authorization_requires_an_exact_bearer_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_static("Bearer secret"),
        );
        assert!(authorized(&headers, "secret"));
        assert!(!authorized(&headers, "secret2"));
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_static("Basic secret"),
        );
        assert!(!authorized(&headers, "secret"));
    }

    #[tokio::test]
    async fn gateway_forwards_stream_chunks_without_buffering_the_response() {
        async fn stream_response() -> Response {
            let chunks = stream::unfold(0, |step| async move {
                match step {
                    0 => Some((
                        Ok::<Bytes, Infallible>(Bytes::from_static(b"data: first\n\n")),
                        1,
                    )),
                    1 => {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        Some((
                            Ok::<Bytes, Infallible>(Bytes::from_static(b"data: second\n\n")),
                            2,
                        ))
                    }
                    _ => None,
                }
            });
            let mut response = Response::new(Body::from_stream(chunks));
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_static("text/event-stream"),
            );
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                header::HeaderValue::from_static("no-cache"),
            );
            response
        }

        let upstream_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("upstream test listener should bind");
        let upstream_address = upstream_listener
            .local_addr()
            .expect("upstream test address should resolve");
        let upstream = tokio::spawn(async move {
            let _ = axum::serve(
                upstream_listener,
                Router::new().route("/v1/chat/completions", post(stream_response)),
            )
            .await;
        });

        let gateway = start(
            0,
            false,
            Zeroizing::new("secret".to_owned()),
            vec![SharedModel {
                entry_id: "entry".to_owned(),
                display_name: "Test model".to_owned(),
                public_name: "public-model".to_owned(),
                backend_model: "backend-model".to_owned(),
                endpoint: format!("http://{upstream_address}"),
            }],
        )
        .await
        .expect("gateway should start");

        let response = tokio::time::timeout(
            Duration::from_millis(400),
            Client::new()
                .post(format!("{}/chat/completions", gateway.address))
                .bearer_auth("secret")
                .json(&json!({"model": "public-model", "stream": true}))
                .send(),
        )
        .await
        .expect("gateway should return response headers before the delayed chunk")
        .expect("streaming request should succeed");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE),
            Some(&header::HeaderValue::from_static("text/event-stream"))
        );
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL),
            Some(&header::HeaderValue::from_static("no-cache"))
        );

        gateway.stop();
        upstream.abort();
    }
}
