use futures::future::BoxFuture;
use http::{
    header::{HeaderName, HeaderValue},
    Request, Response,
};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, info};

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
// Filters

pub enum RequestFilterOutcome<'a> {
    Pass(Request<&'a [u8]>, HashMap<String, String>),
    Fail(Response<Vec<u8>>),
}

pub enum ResponseFilterOutcome {
    Pass(Response<Vec<u8>>),
    Fail(Response<Vec<u8>>),
}

pub trait Filter: Send + Sync {
    fn filter_request<'a>(
        self: Arc<Self>,
        _: Request<&'a [u8]>,
        _: HashMap<String, String>,
    ) -> BoxFuture<'a, anyhow::Result<RequestFilterOutcome<'a>>>;

    fn filter_response<'a>(
        self: Arc<Self>,
        _: Response<Vec<u8>>,
    ) -> BoxFuture<'a, anyhow::Result<ResponseFilterOutcome>>;
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
///
/// Filters are run on the request on the route in order and then in reverse on the response
pub struct Route {
    dynamic_segments: Vec<DynamicSegment>,
    static_segments: Vec<StaticSegment>,
    controller: Arc<dyn Controller>,
    filters: Vec<Arc<dyn Filter>>,
}

impl Route {
    pub fn new(s: impl Into<String>, controller: Arc<dyn Controller>) -> Self {
        Self::filtered(s, controller, vec![])
    }

    pub fn filtered(
        s: impl Into<String>,
        controller: Arc<dyn Controller>,
        filters: Vec<Arc<dyn Filter>>,
    ) -> Self {
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
            filters,
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

    pub fn full_path(&self) -> String {
        let size = self.static_segments.len() + self.dynamic_segments.len();
        let mut combined = Vec::with_capacity(size);

        for _ in 0..size {
            combined.push(String::new());
        }

        for segment in &self.static_segments[..] {
            combined[segment.position] = segment.content.clone();
        }
        for segment in &self.dynamic_segments[..] {
            combined[segment.position] = format!(":{}", segment.name);
        }

        format!("/{}", combined.join("/"))
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
/// controller associated to the route if the route exists.
pub async fn route_request<'a>(
    req: Request<&'a [u8]>,
    routes: Arc<Vec<Route>>,
) -> anyhow::Result<Response<Vec<u8>>> {
    let path = String::from(req.uri().path());
    let method = String::from(req.method().as_str());

    let res = if let Some(route) = routes.iter().find(|route| **route == RawRoute::new(&path)) {
        let controller = route.controller.clone();

        let mut parameters = route.extract_params(&path);
        let mut request = req;

        for filter in &route.filters {
            match filter.clone().filter_request(request, parameters).await? {
                RequestFilterOutcome::Pass(req, params) => {
                    request = req;
                    parameters = params;
                }
                RequestFilterOutcome::Fail(res) => return Ok(res),
            }
        }

        let controller = controller.clone();

        let res = match &method[..] {
            "GET" => controller.get(request, parameters),
            "HEAD" => controller.head(request, parameters),
            "POST" => controller.post(request, parameters),
            "PUT" => controller.put(request, parameters),
            "DELETE" => controller.delete(request, parameters),
            "OPTIONS" => controller.options(request, parameters),
            "PATCH" => controller.patch(request, parameters),
            _ => Box::pin(async move {
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
                .map_err(|e: &(dyn std::error::Error + Send + Sync)| anyhow::Error::from(e))
        } else {
            let mut response = res.unwrap();
            for filter in route.filters.iter().rev() {
                match filter.clone().filter_response(response).await? {
                    ResponseFilterOutcome::Pass(res) => {
                        response = res;
                    }
                    ResponseFilterOutcome::Fail(res) => return Ok(res),
                }
            }
            Ok(response)
        }
    } else {
        Ok(Response::builder().status(404).body(vec![])?)
    }?;

    let req_method_path = format!("{method} {path}");

    let res_status = res.status();
    let diagnostic_str = format!("{req_method_path} => {res_status}");
    match res_status.as_u16() {
        400..=599 => error!("{diagnostic_str}"),
        100..=399 => info!("{diagnostic_str}"),
        _ => error!("{diagnostic_str}"),
    }

    Ok(res)
}

/// Parameter name in the URI for identifying the resource.  Defaults to `id`.
///
/// If you want to change the id field name for a resource, simply implement the `id` method.
pub trait IdParam: Send + Sync {
    fn id() -> String {
        "id".into()
    }
}

pub trait NotFound: Send + Sync {
    fn not_found() -> anyhow::Result<Response<Vec<u8>>> {
        Ok(Response::builder().status(404).body(vec![])?)
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

/// Parse an incoming `Request<&'a [u8]>` into a `Request<Payload>`.
///
/// Assumes JSON formatting, if you need other deserialization formats, implement the parse
/// method for your controller and payload type.
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

/// Convert a Response<Payload> into a Response<Vec<u8>>
///
/// Assumes JSON body format, automatically adds Content-Type: application/json and
/// Content-Length headers.  If you need a different headers, implement the headers()
/// associated method.  If you need a different serialization format, implement the `reply`
/// method.
pub trait Reply<'a, Payload>: Send + 'a
where
    Payload: serde::Serialize + Send + 'a,
{
    /// Sets default headers on the Response before sending the Response to the client.
    fn headers() -> HashMap<String, String> {
        HashMap::from([("content-type".into(), "application/json".into())])
    }

    fn reply(self: Arc<Self>, body: Payload) -> BoxFuture<'a, anyhow::Result<Response<Vec<u8>>>> {
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
            }

            Ok(builder.body(body_bytes)?)
        })
    }
}

/// Convert a `Response<Error>` into a `Response<Vec<u8>>`
///
/// Basically the same thing as `Reply` but supports error codes.
///
/// Assumes JSON body format, automatically adds Content-Type: application/json and
/// Content-Length headers.  If you need a different headers, implement the headers()
/// associated method.  If you need a different serialization format, implement the `error`
/// method.
pub trait Error<'a, Payload, const CODE: u16>: Send + 'a
where
    Payload: serde::Serialize + Send + 'a,
{
    fn headers() -> HashMap<String, String> {
        HashMap::from([("content-type".into(), "application/json".into())])
    }

    fn error(self: Arc<Self>, body: Payload) -> BoxFuture<'a, anyhow::Result<Response<Vec<u8>>>> {
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

/// Sets additional headers on the request.
pub trait AdditionalServiceHeaders<'a, ReqPayload, ResPayload> {
    fn additional_headers(self: Arc<Self>) -> BoxFuture<'a, anyhow::Result<HashMap<String, String>>> {
        Box::pin(async move { Ok(HashMap::new()) })
    }
}

pub trait ServiceResponse<'a, ReqPayload, ResPayload>
where
    ReqPayload: serde::Serialize + Send + 'a,
    ResPayload: for<'de> serde::Deserialize<'de> + Send + 'a,
{
    /// Convert a `Response<&'a [u8]>` to a `Response<'a, ResPayload>`
    fn parse_response(
        self: Arc<Self>,
        res: Response<Vec<u8>>,
    ) -> BoxFuture<'a, anyhow::Result<Response<ResPayload>>> {
        Box::pin(async move {
            let (parts, bytes) = res.into_parts();
            let body = serde_json::from_slice(&bytes)?;
            Ok(Response::from_parts(parts, body))
        })
    }
}

/// Construct a request to an http service and parse the response.
pub trait ServiceRequest<'a, ReqPayload, ResPayload>: ServiceResponse<'a, ReqPayload, ResPayload> + AdditionalServiceHeaders<'a, ReqPayload, ResPayload> + Sync + Send + 'a
where
    ReqPayload: serde::Serialize + Send + 'a,
    ResPayload: for<'de> serde::Deserialize<'de> + Send + 'a,
{

    /// Sets the URI on the Request.
    fn addr(self: Arc<Self>) -> BoxFuture<'a, anyhow::Result<String>>;

    /// Sets the method on the Request.
    fn method() -> http::Method;

    fn service_name() -> &'static str;

    /// Sets default headers on the Request.
    fn headers(self: Arc<Self>) -> BoxFuture<'a, anyhow::Result<HashMap<String, String>>> {
        Box::pin(async move {
            let mut hash_map = HashMap::from([(
                "content-type".into(),
                "application/json".into(),
            )]);

            hash_map.extend(self.additional_headers().await?);

            Ok(hash_map)
        })
    }

    fn create_request(
        self: Arc<Self>,
        path: &'a str,
        payload: ReqPayload,
    ) -> BoxFuture<'a, anyhow::Result<Request<Vec<u8>>>> {
        Box::pin(async move {
            let addr = self.clone().addr().await?;

            let uri = http::Uri::builder()
                .scheme("https")
                .authority(if addr.contains(":") { &addr.split(":").nth(0).unwrap()[..] } else { &addr })
                .path_and_query(path)
                .build()
                .unwrap();

            let mut request_builder = http::request::Builder::new()
                .method(Self::method())
                .uri(uri);

            let headers_mut = request_builder.headers_mut().unwrap();
            {
                for (k, v) in self.clone().headers().await? {
                    headers_mut.insert(
                        HeaderName::from_bytes(k.as_bytes())?,
                        HeaderValue::from_str(&v)?,
                    );
                }
            }

            Ok(request_builder.body(serde_json::to_vec(&payload)?)?)
        })
    }
}
