use rustserve::base::{Base, Create, Read, RequestFormatter, ResponseFormatter, Update};
use rustserve::error::{ErrorHandler, HttpErrorDefault};
use rustserve::safety::{RequestGuard, RequestGuardOutcome};
use rustserve::Controller;

use http::{Request, Response};

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

struct HttpGuard {}

impl RequestGuard for HttpGuard {
    fn verify<'a>(
        self: Arc<Self>,
        req: Request<&'a [u8]>,
        params: HashMap<String, String>,
        base: Arc<dyn Base>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<RequestGuardOutcome<'a>>> + Send + 'a>> {
        Box::pin(async move {
            let has_id = params.contains_key(&base.clone().prefix());

            match req.method().as_str() {
                "PUT" if !has_id => Ok(RequestGuardOutcome::Respond(
                    base.not_found(anyhow::Error::msg("PUT requires an id").into())
                        .await?,
                )),
                "POST" if has_id => Ok(RequestGuardOutcome::Respond(
                    base.bad_request(anyhow::Error::msg("POST does not accept an id").into())
                        .await?,
                )),
                _ => Ok(RequestGuardOutcome::Forward(req, params)),
            }
        })
    }
}

struct MyController {}

impl Base for MyController {}

impl ErrorHandler for MyController {}

impl HttpErrorDefault for MyController {}

impl Controller for MyController {
    fn get<'a>(
        self: Arc<Self>,
        req: Request<&'a [u8]>,
        params: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
        Box::pin(async move {
            Ok(if params.get(&self.clone().prefix()).is_some() {
                let res: Response<ReadResponse> = self.clone().read(req, params).await?;
                self.clone().serialize(res).await?
            } else {
                let res: Response<Vec<ReadResponse>> = self.clone().read(req, params).await?;
                self.clone().serialize(res).await?
            })
        })
    }

    fn post<'a>(
        self: Arc<Self>,
        req: Request<&'a [u8]>,
        params: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
        self.handle_post_request(req, params)
    }

    fn put<'a>(
        self: Arc<Self>,
        req: Request<&'a [u8]>,
        params: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
        self.handle_put_request(req, params)
    }
}

impl<'a> RequestFormatter<'a, String> for MyController {}
impl<'a> ResponseFormatter<'a, String> for MyController {}

#[derive(serde::Serialize)]
pub struct ReadResponse {
    msg: String,
}

impl<'a> ResponseFormatter<'a, ReadResponse> for MyController {}
impl<'a> ResponseFormatter<'a, Vec<ReadResponse>> for MyController {}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct CreateRequest {
    msg: String,
}

#[derive(serde::Serialize)]
pub struct CreateResponse {
    msg: String,
}

impl<'a> RequestFormatter<'a, CreateRequest> for MyController {}
impl<'a> ResponseFormatter<'a, CreateResponse> for MyController {}

impl<'a> Read<'a, ReadResponse> for MyController {
    fn read(
        self: Arc<Self>,
        _req: Request<&'a [u8]>,
        _params: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<ReadResponse>>> + Send + 'a>> {
        Box::pin(async move {
            Ok(Response::builder().status(200).body(ReadResponse {
                msg: String::from("read one response"),
            })?)
        })
    }
}

impl<'a> Read<'a, Vec<ReadResponse>> for MyController {
    fn read(
        self: Arc<Self>,
        _req: Request<&'a [u8]>,
        _params: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<ReadResponse>>>> + Send + 'a>>
    {
        Box::pin(async move {
            Ok(Response::builder().status(200).body(vec![
                ReadResponse {
                    msg: String::from("read many"),
                },
                ReadResponse {
                    msg: String::from("response"),
                },
            ])?)
        })
    }
}

impl<'a> Create<'a, CreateRequest, CreateResponse> for MyController {
    fn create(
        self: Arc<Self>,
        req: Request<CreateRequest>,
        _params: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<CreateResponse>>> + Send + 'a>> {
        Box::pin(async move {
            let query_params = req
                .uri()
                .query()
                .map(|query| self.extract_query_params(query))
                .unwrap_or(HashMap::new());

            dbg!(query_params);
            Ok(Response::builder().status(200).body(CreateResponse {
                msg: String::from("create response"),
            })?)
        })
    }
}

impl<'a> Update<'a, String, String> for MyController {
    fn update(
        self: Arc<Self>,
        _req: Request<String>,
        _params: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<String>>> + Send + 'a>> {
        Box::pin(async move {
            Ok(Response::builder()
                .status(200)
                .body(String::from("update response"))?)
        })
    }
}

#[test]
fn test() {
    use rustserve::safety::Protect;
    use rustserve::Route;
    use tracing::Level;

    let my_controller = Arc::new(MyController {});

    let http_guard = Arc::new(HttpGuard {});

    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        // all spans/events with a level higher than TRACE (e.g, debug, info, warn, etc.)
        // will be written to stdout.
        .with_max_level(Level::TRACE)
        // builds the subscriber.
        .finish();

    let my_controller = Arc::new(Protect {
        controller: my_controller.clone(),
        request_guards: vec![http_guard.clone()],
        response_guards: vec![],
    });

    let routes = vec![
        Route::new("/:version/resource", my_controller.clone()),
        Route::new(
            format!("/:version/resource/:{}", my_controller.clone().prefix()),
            my_controller.clone(),
        ),
    ];

    tracing::subscriber::with_default(subscriber, || {
        let body = vec![];
        let req = Request::builder()
            .uri("/1/resource/")
            .method("GET")
            .body(&body[..])
            .unwrap();
        let res = futures::executor::block_on(rustserve::route_request(req, &routes)).unwrap();
        assert!(res.status() == 200);
        assert!(res.body() == b"[{\"msg\":\"read many\"},{\"msg\":\"response\"}]");

        let body = vec![];
        let req = Request::builder()
            .uri("/1/resource/1")
            .method("GET")
            .body(&body[..])
            .unwrap();
        let res = futures::executor::block_on(rustserve::route_request(req, &routes)).unwrap();
        assert!(res.status() == 200);
        assert!(res.body() == b"{\"msg\":\"read one response\"}");

        let body = serde_json::to_vec(&CreateRequest { msg: "test".into() }).unwrap();
        let req = Request::builder()
            .uri("/1/resource?a=1")
            .method("POST")
            .body(&body[..])
            .unwrap();
        let res = futures::executor::block_on(rustserve::route_request(req, &routes)).unwrap();
        assert!(res.status() == 200);
        assert!(res.body() == b"{\"msg\":\"create response\"}");

        let body = serde_json::to_vec("test").unwrap();
        let req = Request::builder()
            .uri("/1/resource/1")
            .method("PUT")
            .body(&body[..])
            .unwrap();
        let res = futures::executor::block_on(rustserve::route_request(req, &routes)).unwrap();
        assert!(res.status() == 200);
        assert!(res.body() == b"\"update response\"");

        let body = serde_json::to_vec("test").unwrap();
        let req = Request::builder()
            .uri("/1/resource/1")
            .method("POST")
            .body(&body[..])
            .unwrap();
        let res = futures::executor::block_on(rustserve::route_request(req, &routes)).unwrap();
        assert!(res.status() == 400);
        assert!(res.body() == b"\"POST does not accept an id\"");

        let body = serde_json::to_vec("test").unwrap();
        let req = Request::builder()
            .uri("/1/resource")
            .method("PUT")
            .body(&body[..])
            .unwrap();
        let res = futures::executor::block_on(rustserve::route_request(req, &routes)).unwrap();
        assert!(res.status() == 404);
        assert!(res.body() == b"\"PUT requires an id\"");
    });
}
