#![feature(trait_upcasting)]

use http::{Request, Response};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tracing::{error, info, warn};

// ----------------------------
// Errors

macro_rules! generate_http_error_method {
    ($error_name:ident, $code:literal) => {
        fn $error_name(
            self: Arc<Self>,
            _: Option<anyhow::Error>,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send>> {
            Box::pin(async move {
                Ok(Response::builder().status($code).body(vec![])?)
            })
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
    generate_http_error_method!(not_found, 404);
    generate_http_error_method!(bad_request, 400);
    generate_http_error_method!(unauthorized, 401);
    generate_http_error_method!(internal_server_error, 500);
}

pub trait ErrorHandlerDefault {}

impl<T> HttpError for T
where
    T: ErrorHandler + ErrorHandlerDefault,
{
    fn not_found(
        self: Arc<Self>,
        e: Option<anyhow::Error>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send>> {
        Box::pin(async move { self.handle_error(404, e).await })
    }

    fn bad_request(
        self: Arc<Self>,
        e: Option<anyhow::Error>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send>> {
        Box::pin(async move { self.handle_error(400, e).await })
    }

    fn unauthorized(
        self: Arc<Self>,
        e: Option<anyhow::Error>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send>> {
        Box::pin(async move { self.handle_error(401, e).await })
    }

    fn internal_server_error(
        self: Arc<Self>,
        e: Option<anyhow::Error>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send>> {
        Box::pin(async move { self.handle_error(500, e).await })
    }
}

pub trait ErrorHandler: Entity + 'static {
    fn handle_error(
        self: Arc<Self>,
        code: u16,
        e: Option<anyhow::Error>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send>> {
        Box::pin(async move {
            let builder = Response::builder().status(code);

            if let Some(err) = e {
                warn!("{code}: {err}");
                Ok(builder.body(serde_json::to_vec(&format!("{err}"))?)?)
            } else {
                warn!("{code}");
                Ok(builder.body(vec![])?)
            }
        })
    }
}

// ----------------------------
// Entity

pub trait Entity: HttpError + Send + Sync {
    fn prefix(self: Arc<Self>) -> String {
        "id".into()
    }
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
            self.not_found(None)
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
pub trait Controller: Entity + Send + Sync {
    generate_http_method!(get);
    generate_http_method!(head);
    generate_http_method!(delete);
    generate_http_method!(post);
    generate_http_method!(put);
    generate_http_method!(options);
    generate_http_method!(patch);
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
        entity: Arc<dyn Entity>,
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
                                .internal_server_error(Some(e.into()))
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

impl Entity for Protect {
    fn prefix(self: Arc<Self>) -> String {
        self.controller.clone().prefix()
    }
}

impl Controller for Protect {
    evaluate_guards_then_delegate_to_inner_controller!(get);
    evaluate_guards_then_delegate_to_inner_controller!(post);
    evaluate_guards_then_delegate_to_inner_controller!(put);
    evaluate_guards_then_delegate_to_inner_controller!(head);
    evaluate_guards_then_delegate_to_inner_controller!(delete);
    evaluate_guards_then_delegate_to_inner_controller!(options);
    evaluate_guards_then_delegate_to_inner_controller!(patch);
}

macro_rules! delegate_to_inner_http_error_method {
    ($error_name:ident) => {
        fn $error_name(
            self: Arc<Self>,
            e: Option<anyhow::Error>,
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
        info!("{method} {path}");
        let params = route.extract_params(path);
        let controller = route.controller.clone();
        let res = match method {
            "GET" => controller.clone().get(req, params),
            "HEAD" => controller.clone().head(req, params),
            "POST" => controller.clone().post(req, params),
            "PUT" => controller.clone().put(req, params),
            "DELETE" => controller.clone().delete(req, params),
            "OPTIONS" => controller.clone().options(req, params),
            "PATCH" => controller.clone().patch(req, params),
            _ => controller
                .clone()
                .bad_request(anyhow::Error::msg("unsupported HTTP method").into()),
        }
        .await;

        if let Err(e) = res {
            Ok(controller.clone().internal_server_error(e.into()).await?)
        } else {
            res
        }
    } else {
        Ok(Response::builder().status(404).body(vec![])?)
    };

    Ok(res?)
}

// ----------------------------
// Entity CRUD

pub trait Create<'a>: Entity + 'a {
    type Req: for<'de> serde::Deserialize<'de> + Send + 'a;
    type Res: serde::Serialize + Send + 'a;

    fn create(
        self: Arc<Self>,
        req: Request<Self::Req>,
        params: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Self::Res>>> + Send + 'a>>;

    fn serialize(
        self: Arc<Self>,
        res: Response<Self::Res>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
        Box::pin(async move {
            let (parts, body) = res.into_parts();
            let json_body_bytes = serde_json::to_vec(&body)?;
            Ok(Response::from_parts(parts, json_body_bytes))
        })
    }

    fn deserialize(
        self: Arc<Self>,
        req: Request<&'a [u8]>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Request<Self::Req>>> + Send + 'a>> {
        Box::pin(async move {
            let (parts, bytes) = req.into_parts();
            let body = serde_json::from_slice(&bytes)?;
            Ok(Request::from_parts(parts, body))
        })
    }

    fn handle_post_request(
        self: Arc<Self>,
        req: Request<&'a [u8]>,
        params: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
        Box::pin(async move {
            self.clone()
                .serialize(
                    self.clone()
                        .create(self.deserialize(req).await?, params)
                        .await?,
                )
                .await
        })
    }
}

pub trait Read<'a>: Entity + 'a {
    type Res: serde::Serialize + Send + 'a;

    fn get_one(
        self: Arc<Self>,
        _req: Request<&'a [u8]>,
        _params: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Self::Res>>> + Send + 'a>>;

    fn get_many(
        self: Arc<Self>,
        _req: Request<&'a [u8]>,
        _params: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<Self::Res>>>> + Send + 'a>>;

    fn serialize(
        self: Arc<Self>,
        res: Response<Self::Res>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
        Box::pin(async move {
            let (parts, body) = res.into_parts();
            let json_body_bytes = serde_json::to_vec(&body)?;
            Ok(Response::from_parts(parts, json_body_bytes))
        })
    }

    fn serialize_many(
        self: Arc<Self>,
        res: Response<Vec<Self::Res>>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
        Box::pin(async move {
            let (parts, body) = res.into_parts();
            let json_body_bytes = serde_json::to_vec(&body)?;
            Ok(Response::from_parts(parts, json_body_bytes))
        })
    }

    fn handle_get_request(
        self: Arc<Self>,
        req: Request<&'a [u8]>,
        params: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
        Box::pin(async move {
            let prefix = self.clone().prefix();
            if params.contains_key(&prefix) {
                self.clone()
                    .serialize(self.get_one(req, params).await?)
                    .await
            } else {
                self.clone()
                    .serialize_many(self.get_many(req, params).await?)
                    .await
            }
        })
    }
}

pub trait Update<'a>: Entity + 'a {
    type Req: for<'de> serde::Deserialize<'de> + Send + 'a;
    type Res: serde::Serialize + Send + 'a;

    fn update(
        self: Arc<Self>,
        req: Request<Self::Req>,
        params: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Self::Res>>> + Send + 'a>>;

    fn serialize(
        self: Arc<Self>,
        res: Response<Self::Res>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
        Box::pin(async move {
            let (parts, body) = res.into_parts();
            let json_body_bytes = serde_json::to_vec(&body)?;
            Ok(Response::from_parts(parts, json_body_bytes))
        })
    }

    fn deserialize(
        self: Arc<Self>,
        req: Request<&'a [u8]>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Request<Self::Req>>> + Send + 'a>> {
        Box::pin(async move {
            let (parts, bytes) = req.into_parts();
            let body = serde_json::from_slice(&bytes)?;
            Ok(Request::from_parts(parts, body))
        })
    }

    fn handle_put_request(
        self: Arc<Self>,
        req: Request<&'a [u8]>,
        params: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
        Box::pin(async move {
            self.clone()
                .serialize(
                    self.clone()
                        .update(self.deserialize(req).await?, params)
                        .await?,
                )
                .await
        })
    }
}
