use http::{Request, Response};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

// ----------------------------
// Errors

macro_rules! generate_http_error_method {
    ($error_name:ident) => {
        fn $error_name(
            self: Arc<Self>,
            _: anyhow::Error,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send>> {
            Box::pin($error_name())
        }
    };
}

pub trait HttpError: Send + Sync {
    generate_http_error_method!(not_found);
    generate_http_error_method!(bad_request);
    generate_http_error_method!(internal_server_error);
}

pub async fn not_found() -> anyhow::Result<Response<Vec<u8>>> {
    Ok(serialize(Response::builder().status(404).body(())?)?)
}

pub async fn bad_request() -> anyhow::Result<Response<Vec<u8>>> {
    Ok(serialize(Response::builder().status(400).body(())?)?)
}

pub async fn internal_server_error() -> anyhow::Result<Response<Vec<u8>>> {
    Ok(serialize(Response::builder().status(500).body(())?)?)
}

// ----------------------------
// Controllers

macro_rules! generate_http_method {
    ($method_name:ident) => {
        fn $method_name<'a>(
            self: Arc<Self>,
            _: Request<&'a [u8]>,
            _: HashMap<String, String>,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
            self.not_found(anyhow::Error::msg(""))
        }
    };
}

pub trait Controller: HttpError + Send + Sync {
    generate_http_method!(get);
    generate_http_method!(head);
    generate_http_method!(post);
    generate_http_method!(put);
    generate_http_method!(delete);
    generate_http_method!(connect);
    generate_http_method!(options);
    generate_http_method!(trace);
    generate_http_method!(patch);
}

// ----------------------------
// Serde

pub fn serialize<T>(req: Response<T>) -> anyhow::Result<Response<Vec<u8>>>
where
    T: serde::Serialize,
{
    let (res_parts, res_body) = req.into_parts();
    Ok(Response::from_parts(
        res_parts,
        serde_json::to_vec(&res_body)?,
    ))
}

pub fn deserialize<'a, T>(req: Request<&'a [u8]>) -> anyhow::Result<Request<T>>
where
    T: serde::Deserialize<'a>,
{
    let (req_parts, req_body) = req.into_parts();
    Ok(Request::from_parts(
        req_parts,
        serde_json::from_slice(&req_body)?,
    ))
}

// ----------------------------
// Guards

pub enum Outcome<'a> {
    Forward(Request<&'a [u8]>, HashMap<String, String>),
    Respond(Response<Vec<u8>>),
}

pub trait Guard: Send + Sync {
    fn verify<'a>(
        self: Arc<Self>,
        _: Request<&'a [u8]>,
        _: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = Outcome<'a>> + Send>>;
}

pub struct Protect {
    pub guards: Vec<Arc<dyn Guard>>,
    pub controller: Arc<dyn Controller>,
}

macro_rules! delegate_to_inner_controller {
    ($controller_method:ident) => {
        fn $controller_method<'a>(
            self: Arc<Self>,
            request: Request<&'a [u8]>,
            parameters: HashMap<String, String>,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
            Box::pin(async move {
                let mut req = request;
                let mut params = parameters;
                for guard in self.clone().guards.iter() {
                    match guard.clone().verify(req, params).await {
                        Outcome::Forward(forwarded_req, forwarded_params) => {
                            req = forwarded_req;
                            params = forwarded_params;
                            continue;
                        }
                        Outcome::Respond(res) => return Ok(res),
                    }
                }

                self.controller
                    .clone()
                    .$controller_method(req, params)
                    .await
            })
        }
    };
}

impl Controller for Protect {
    delegate_to_inner_controller!(get);
    delegate_to_inner_controller!(head);
    delegate_to_inner_controller!(post);
    delegate_to_inner_controller!(put);
    delegate_to_inner_controller!(delete);
    delegate_to_inner_controller!(connect);
    delegate_to_inner_controller!(options);
    delegate_to_inner_controller!(trace);
    delegate_to_inner_controller!(patch);
}

macro_rules! delegate_to_inner_http_error_method {
    ($error_name:ident) => {
        fn $error_name(
            self: Arc<Self>,
            e: anyhow::Error,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send>> {
            self.controller.clone().$error_name(e)
        }
    };
}
impl HttpError for Protect {
    delegate_to_inner_http_error_method!(not_found);
    delegate_to_inner_http_error_method!(internal_server_error);
}

// ----------------------------
// Routing

pub struct StaticSegment {
    content: String,
    position: usize,
}

pub struct DynamicSegment {
    name: String,
    position: usize,
}

pub struct Route {
    dynamic_segments: Vec<DynamicSegment>,
    static_segments: Vec<StaticSegment>,
    controller: Arc<dyn Controller>,
}

impl Route {
    pub fn new(s: impl Into<String>, controller: Arc<dyn Controller>) -> Self {
        let input = s.into();

        let (static_segments_vec, dynamic_segments_vec) = input
            .split("/")
            .filter(|s| s.len() > 0)
            .enumerate()
            .partition::<Vec<_>, _>(|entry| entry.1.starts_with(":"));

        let static_segments = static_segments_vec
            .iter()
            .map(|(pos, segment)| StaticSegment {
                content: String::from(*segment),
                position: *pos,
            })
            .collect();

        let dynamic_segments = dynamic_segments_vec
            .iter()
            .map(|(pos, segment)| DynamicSegment {
                name: String::from(&segment[1..]),
                position: *pos,
            })
            .collect();

        Self {
            dynamic_segments,
            static_segments,
            controller,
        }
    }

    pub fn extract_params(&self, path: &str) -> HashMap<String, String> {
        let segments = path.split("/").filter(|s| s != &"").collect::<Vec<_>>();

        self.dynamic_segments
            .iter()
            .fold(HashMap::new(), |mut hash_map, segment| {
                hash_map.insert(
                    segment.name.clone(),
                    String::from(segments[segment.position]),
                );
                hash_map
            })
    }
}

impl PartialEq<RawRoute> for Route {
    fn eq(&self, other: &RawRoute) -> bool {
        let static_segments = &self.static_segments;
        let n = self.dynamic_segments.len() + static_segments.len();
        n == other.content.len()
            && static_segments.iter().fold(true, |state, segment| {
                state && other.content[segment.position] == segment.content
            })
    }
}

pub struct RawRoute {
    content: Vec<String>,
}

impl RawRoute {
    fn new(s: impl Into<String>) -> Self {
        let input = s.into();

        Self {
            content: input
                .split("/")
                .filter(|s| s.len() > 0)
                .map(String::from)
                .collect(),
        }
    }
}

impl PartialEq<Route> for RawRoute {
    fn eq(&self, other: &Route) -> bool {
        other == self
    }
}

pub async fn route_request<'a>(
    req: Request<&'a [u8]>,
    routes: &[Route],
) -> anyhow::Result<Response<Vec<u8>>> {
    let method = req.method().as_str();
    let path = req.uri().path();

    let res = if let Some(route) = routes.iter().find(|route| **route == RawRoute::new(path)) {
        let params = route.extract_params(path);
        let controller = route.controller.clone();
        match method {
            "GET" => controller.get(req, params),
            "HEAD" => controller.head(req, params),
            "POST" => controller.post(req, params),
            "PUT" => controller.put(req, params),
            "DELETE" => controller.delete(req, params),
            "CONNECT" => controller.connect(req, params),
            "OPTIONS" => controller.options(req, params),
            "TRACE" => controller.trace(req, params),
            "PATCH" => controller.patch(req, params),
            _ => controller.bad_request(anyhow::Error::msg("unsupported HTTP method")),
        }
        .await
    } else {
        not_found().await
    };

    Ok(res?)
}
