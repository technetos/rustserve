#![feature(trait_upcasting)]

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

/// A trait providing HTTP error methods to Controllers.
///
/// When your controller implements this trait, the controllers implementations will be called when
/// an error occurs.  You can also invoke them yourself through `self.error_method()`. All error
/// methods have defaults, you dont have to implement any of them.  The default implementations
/// only return the error code, no error message of any kind is returned to the client.  When
/// implementing an error method, you have access to the error as an argument `e: anyhow::Error`
/// that you can return to the client. You can also simply return
/// `Response::builder().status(XXX).body(())?` to return any error code with any body that you
/// want.  These methods exist purely for the ability to have different specific error method
/// implementations for each controller and for those implementations to be reachable from code
/// working with a `dyn Controller`. Only very commonly used error methods are defined in this
/// trait, if you feel other error codes should be supported here please make a PR.
pub trait HttpError: Send + Sync {
    generate_http_error_method!(not_found);
    generate_http_error_method!(bad_request);
    generate_http_error_method!(unauthorized);
    generate_http_error_method!(internal_server_error);
}

async fn not_found() -> anyhow::Result<Response<Vec<u8>>> {
    Ok(serialize(Response::builder().status(404).body(())?)?)
}

async fn bad_request() -> anyhow::Result<Response<Vec<u8>>> {
    Ok(serialize(Response::builder().status(400).body(())?)?)
}

async fn unauthorized() -> anyhow::Result<Response<Vec<u8>>> {
    Ok(serialize(Response::builder().status(401).body(())?)?)
}

async fn internal_server_error() -> anyhow::Result<Response<Vec<u8>>> {
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

/// A trait providing methods to handle HTTP requests.  
///
/// Controllers are how RustServe handles HTTP requests.  Routes store an `Arc<dyn Controller>` and
/// controllers implement the HTTP methods for that route. All controller methods have defaults,
/// you dont have to implement any of them.  If you dont implement a method, then requests on that
/// route to that method return a 404, its not there, you didnt implement it.  By implementing a
/// controller method, requests with that HTTP method will be routed to your implementation.  
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

/// Serialize the body of a `Response<T: Serialize>` into a `Response<Vec<u8>>`
///
/// A convienence function for converting an `http::Response<T>` to an `http::Response<Vec<u8>>`
/// using JSON serialization.
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

/// Deserialize the body of a `Request<&[u8]>` into a `Request<T: Deserialize>`
///
/// A convienence function for converting an `http::Request<&[u8]>` to an `http::Request<T>` using
/// JSON deserialization.
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

/// The output of a guard verifying a request.
///
/// Outcome can be a `Forward(req, params)` or a `Respond(res)`.
///
/// + `Forward` returns the request and the parameters parsed from the path.  `Protect` use an
///    array of guards to implement layers of request verification per controller.  The next guard in
///    a `Protect` controller consumes the forwarded request and params for additional verification
///    until all guards have verified the request.  Finally the request is passed to the controller
///    by calling the corresponding `http` method.
/// + `Respond` returns an `http::Response<Vec<u8>>` immediately back to the client.
pub enum Outcome<'a> {
    Forward(Request<&'a [u8]>, HashMap<String, String>),
    Respond(Response<Vec<u8>>),
}

/// A trait providing a filtering mechanism for requests.
///
/// Guards are used by `Protect` to `verify` that a request meets a specific criteria.  Guards are
/// evaluated in a short circut fashion, each guard returns an `Outcome` that can either `Forward`
/// the request or `Respond` immediately.  
pub trait Guard: Send + Sync {
    fn verify<'a>(
        self: Arc<Self>,
        req: Request<&'a [u8]>,
        params: HashMap<String, String>,
        // using experimental feature trait_upcasting to enforce contract and prevent guards from
        // interacting with the controller in anyway other than error methods
        error: Arc<dyn HttpError>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Outcome<'a>>> + Send + 'a>>;
}

/// A wrapper around a `Controller` that evaluates its `Guards` before delegating to the inner
/// `Controller`.
///
/// `Protect` is a composite `Controller` that wraps an inner `Controller` along with a series of
/// `Guards`.  When a request is routed to a _protected_ `Controller` the series of `Guards`
/// evaluate the request in a short circut fashion before delegating to the inner controller.  
pub struct Protect {
    pub guards: Vec<Arc<dyn Guard>>,
    pub controller: Arc<dyn Controller>,
}

macro_rules! evaluate_guards_then_delegate_to_inner_controller {
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
                    match guard
                        .clone()
                        .verify(req, params, self.controller.clone())
                        .await
                    {
                        Ok(Outcome::Forward(forwarded_req, forwarded_params)) => {
                            req = forwarded_req;
                            params = forwarded_params;
                            continue;
                        }
                        Ok(Outcome::Respond(res)) => return Ok(res),
                        Err(e) => {
                            return self
                                .controller
                                .clone()
                                .internal_server_error(e.into())
                                .await
                        }
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
    evaluate_guards_then_delegate_to_inner_controller!(get);
    evaluate_guards_then_delegate_to_inner_controller!(head);
    evaluate_guards_then_delegate_to_inner_controller!(post);
    evaluate_guards_then_delegate_to_inner_controller!(put);
    evaluate_guards_then_delegate_to_inner_controller!(delete);
    evaluate_guards_then_delegate_to_inner_controller!(connect);
    evaluate_guards_then_delegate_to_inner_controller!(options);
    evaluate_guards_then_delegate_to_inner_controller!(trace);
    evaluate_guards_then_delegate_to_inner_controller!(patch);
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
    delegate_to_inner_http_error_method!(bad_request);
    delegate_to_inner_http_error_method!(unauthorized);
    delegate_to_inner_http_error_method!(internal_server_error);
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

impl PartialEq<Route> for RawRoute {
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
