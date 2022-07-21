use futures::future::BoxFuture;
use http::{
    header::{HeaderName, HeaderValue},
    Request, Response,
};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, info, warn};

// ----------------------------
// Controllers

macro_rules! generate_http_method {
    ($method_name:ident) => {
        fn $method_name<'a>(
            self: Arc<Self>,
            _: Request<&'a [u8]>,
            _: HashMap<String, String>,
        ) -> BoxFuture<'a, anyhow::Result<Response<Vec<u8>>>> {
            Box::pin(async move { Ok(Response::builder().status(404).body(vec![])?) })
        }
    };
}

/// A trait providing methods to handle HTTP requests.  
///
/// Controllers are how RustServe handles HTTP requests.  Routes store an `Arc<dyn Controller>` and
/// controllers implement the HTTP methods for that route. All controller methods have defaults,
/// you don't have to implement any of them.  If you don't implement a method, then requests on that
/// route to that method return a 404, its not there, you didn't implement it.  By implementing a
/// controller method, requests with that HTTP method will be routed to your implementation.  
pub trait Controller: Send + Sync {
    generate_http_method!(get);
    generate_http_method!(head);
    generate_http_method!(delete);
    generate_http_method!(post);
    generate_http_method!(put);
    generate_http_method!(options);
    generate_http_method!(patch);
}

// ----------------------------
// Routing

struct StaticSegment {
    content: String,
    position: usize,
}

struct DynamicSegment {
    name: String,
    position: usize,
}

/// A path and controller reachable by `HTTP` requests
///
/// Routes are defined by the path to be matched against an incoming request and the controller to
/// handle the request.  Path are formatted using `:` to designate a dynamic segment.  For example
/// in the path `/:version/test`, `version` is a dynamic segment, it can be 1, 2, 3, or anything
/// really, and `test` is a static segment that must be matched exactly.  
pub struct Route {
    dynamic_segments: Vec<DynamicSegment>,
    static_segments: Vec<StaticSegment>,
    controller: Arc<dyn Controller>,
}

impl Route {
    pub fn new(s: impl Into<String>, controller: Arc<dyn Controller>) -> Self {
        let input = s.into();

        let (dynamic_segments_vec, static_segments_vec) = input
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

impl<'a> PartialEq<RawRoute> for Route {
    fn eq(&self, other: &RawRoute) -> bool {
        let static_segments = &self.static_segments;
        let n = self.dynamic_segments.len() + static_segments.len();
        n == other.content.len()
            && static_segments.iter().fold(true, |state, segment| {
                state && other.content[segment.position] == segment.content
            })
    }
}

struct RawRoute {
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

impl<'a> PartialEq<Route> for RawRoute {
    fn eq(&self, other: &Route) -> bool {
        other == self
    }
}

/// Route a request to a controller or return not found
///
/// A function providing routing support.  Takes in a request and the routes and delegates to the
/// controller associated to the route if a route exists.  
pub async fn route_request<'a>(
    req: Request<&'a [u8]>,
    routes: Arc<Vec<Route>>,
) -> anyhow::Result<Response<Vec<u8>>> {
    let method = req.method().as_str();
    let path = req.uri().path();

    let req_method_path = format!("{method} {path}");

    let res = if let Some(route) = routes.iter().find(|route| **route == RawRoute::new(path)) {
        let params = route.extract_params(path);
        let controller = &route.controller.clone();
        let res = match method {
            "GET" => controller.clone().get(req, params),
            "HEAD" => controller.clone().head(req, params),
            "POST" => controller.clone().post(req, params),
            "PUT" => controller.clone().put(req, params),
            "DELETE" => controller.clone().delete(req, params),
            "OPTIONS" => controller.clone().options(req, params),
            "PATCH" => controller.clone().patch(req, params),
            _ => Box::pin(async move {
                // content length??
                // content type??
                // this needs a proper formatter
                Response::builder()
                    .status(400)
                    .body(Vec::from(&b"unsupported HTTP method"[..]))
                    .map_err(|e| e.into())
            }),
        }
        .await;

        if let Err(e) = res {
            tracing::error!("{e}");
            Ok(Response::builder().status(500).body(vec![])?)
        } else {
            res
        }
    } else {
        Ok(Response::builder().status(404).body(vec![])?)
    }?;

    let res_status = res.status();
    let diagnostic_str = format!("{req_method_path} => {res_status}");
    match res_status.as_u16() {
        500..=599 => error!("{diagnostic_str}"),
        400..=499 => warn!("{diagnostic_str}"),
        100..=399 => info!("{diagnostic_str}"),
        _ => error!("{diagnostic_str}"),
    }

    Ok(res)
}

// ----------------------------
// Base

pub mod base {
    use super::*;

    pub trait IdParam {
        fn id(self: Arc<Self>) -> String {
            "id".into()
        }
    }

    pub trait QueryParams {
        fn extract_query_params(self: Arc<Self>, query: Option<&str>) -> HashMap<String, String> {
            query.iter().fold(HashMap::new(), |mut hash_map, query| {
                query.split("&").for_each(|param| {
                    let key_value = param.split("=").collect::<Vec<_>>();
                    if key_value.len() == 2 {
                        hash_map.insert(key_value[0].into(), key_value[1].into());
                    }
                });
                hash_map
            })
        }
    }

    pub trait Parse<'a, Payload>: Send + 'a
    where
        Payload: for<'de> serde::Deserialize<'de> + Send + 'a,
    {
        fn parse(
            self: Arc<Self>,
            req: Request<&'a [u8]>,
        ) -> BoxFuture<'a, anyhow::Result<Request<Payload>>> {
            Box::pin(async move {
                let (parts, bytes) = req.into_parts();
                let body = serde_json::from_slice(&bytes)?;
                Ok(Request::from_parts(parts, body))
            })
        }
    }

    pub trait Reply<'a, Payload>: Send + 'a
    where
        Payload: serde::Serialize + Send + 'a,
    {
        fn headers() -> HashMap<String, String> {
            let mut hash_map = HashMap::new();
            hash_map.insert("content-type".into(), "application/json".into());
            hash_map
        }

        fn reply(
            self: Arc<Self>,
            body: Payload,
        ) -> BoxFuture<'a, anyhow::Result<Response<Vec<u8>>>> {
            Box::pin(async move {
                let mut builder = Response::builder().status(200);

                let body_bytes = serde_json::to_vec(&body)?;

                let headers_mut = builder.headers_mut().unwrap();
                {
                    for (k, v) in Self::headers() {
                        headers_mut.insert(
                            HeaderName::from_bytes(k.as_bytes())?,
                            HeaderValue::from_str(&v)?,
                        );
                    }
                    headers_mut.insert("content-length", HeaderValue::from(body_bytes.len()));
                }

                Ok(builder.body(body_bytes)?)
            })
        }
    }

    pub trait Error<'a, Payload, const CODE: u16>: Send + 'a
    where
        Payload: serde::Serialize + Send + 'a,
    {
        fn headers() -> HashMap<String, String> {
            let mut hash_map = HashMap::new();
            hash_map.insert("content-type".into(), "application/json".into());
            hash_map
        }

        fn error(
            self: Arc<Self>,
            body: Payload,
        ) -> BoxFuture<'a, anyhow::Result<Response<Vec<u8>>>> {
            Box::pin(async move {
                let mut builder = Response::builder().status(CODE);

                let headers_mut = builder.headers_mut().unwrap();
                {
                    for (k, v) in Self::headers() {
                        headers_mut.insert(
                            HeaderName::from_bytes(k.as_bytes())?,
                            HeaderValue::from_str(&v)?,
                        );
                    }
                }

                let json_body_bytes = serde_json::to_vec(&body)?;
                Ok(builder.body(json_body_bytes)?)
            })
        }
    }

    pub trait Create<'a, Req, Res>: Parse<'a, Req> + Reply<'a, Res>
    where
        Req: for<'de> serde::Deserialize<'de> + Send + 'a,
        Res: serde::Serialize + Send + 'a,
    {
        fn create(
            self: Arc<Self>,
            req: Request<Req>,
            params: HashMap<String, String>,
        ) -> BoxFuture<'a, anyhow::Result<Response<Vec<u8>>>>;
    }

    pub trait Read<'a, Res>: IdParam + Reply<'a, Res>
    where
        Res: serde::Serialize + Send + 'a,
    {
        fn read(
            self: Arc<Self>,
            _req: Request<&'a [u8]>,
            _params: HashMap<String, String>,
        ) -> BoxFuture<'a, anyhow::Result<Response<Vec<u8>>>>;
    }

    pub trait Update<'a, Req, Res>: IdParam + Parse<'a, Req> + Reply<'a, Res>
    where
        Req: for<'de> serde::Deserialize<'de> + Send + 'a,
        Res: serde::Serialize + Send + 'a,
    {
        fn update(
            self: Arc<Self>,
            req: Request<Req>,
            params: HashMap<String, String>,
        ) -> BoxFuture<'a, anyhow::Result<Response<Vec<u8>>>>;
    }

    pub trait Delete<'a, Res>: IdParam + Reply<'a, Res>
    where
        Res: serde::Serialize + Send + 'a,
    {
        fn delete(
            self: Arc<Self>,
            _req: Request<&'a [u8]>,
            _params: HashMap<String, String>,
        ) -> BoxFuture<'a, anyhow::Result<Response<Vec<u8>>>>;
    }
}
