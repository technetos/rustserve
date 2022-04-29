use http::{Request, Response};
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
    Forward(Request<&'a [u8]>),
    Respond(Response<Vec<u8>>),
}

pub trait Guard: Send + Sync {
    fn verify<'a>(
        self: Arc<Self>,
        _: Request<&'a [u8]>,
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
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
            Box::pin(async move {
                let mut req = request;
                for guard in self.clone().guards.iter() {
                    match guard.clone().verify(req).await {
                        Outcome::Forward(forwarded_req) => {
                            req = forwarded_req;
                            continue;
                        }
                        Outcome::Respond(res) => return Ok(res),
                    }
                }

                self.controller.clone().$controller_method(req).await
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
}

impl Route {
    pub fn new(s: impl Into<String>) -> Self {
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
        }
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
