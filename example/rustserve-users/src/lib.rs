use rustserve::Controller;
use rustserve::Entity;
use rustserve::ErrorHandler;
use rustserve::Guard;
use rustserve::Outcome;
use rustserve::{Create, Read, Update};

use http::{Request, Response};

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

struct HttpGuard {}

impl Guard for HttpGuard {
    fn verify<'a>(
        self: Arc<Self>,
        req: Request<&'a [u8]>,
        params: HashMap<String, String>,
        entity: Arc<dyn Entity>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Outcome<'a>>> + Send + 'a>> {
        Box::pin(async move {
            let has_entity_id = params.contains_key(&entity.clone().prefix());

            match req.method().as_str() {
                "PUT" if !has_entity_id => Ok(Outcome::Respond(
                    entity
                        .not_found(anyhow::Error::msg("PUT requires an id").into())
                        .await?,
                )),
                "POST" if has_entity_id => Ok(Outcome::Respond(
                    entity
                        .bad_request(anyhow::Error::msg("POST does not accept an id").into())
                        .await?,
                )),
                _ => Ok(Outcome::Forward(req, params)),
            }
        })
    }
}

pub fn extract_query_params(query: &str) -> HashMap<String, String> {
    let mut hash_map = HashMap::new();

    query.split("&").for_each(|param| {
        let key_value = param.split("=").collect::<Vec<_>>();
        if key_value.len() == 2 {
            hash_map.insert(key_value[0].into(), key_value[1].into());
        }
    });

    hash_map
}

struct MyController {}

impl Entity for MyController {}

impl ErrorHandler for MyController {}

impl rustserve::ErrorHandlerDefault for MyController {}

impl Controller for MyController {
    fn get<'a>(
        self: Arc<Self>,
        req: Request<&'a [u8]>,
        params: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
        self.handle_get_request(req, params)
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

impl<'a> Read<'a> for MyController {
    type Res = String;

    fn get_one(
        self: Arc<Self>,
        _req: Request<&'a [u8]>,
        _params: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Self::Res>>> + Send + 'a>> {
        Box::pin(async move {
            Ok(Response::builder()
                .status(200)
                .body(String::from("get_one response"))?)
        })
    }

    fn get_many(
        self: Arc<Self>,
        _req: Request<&'a [u8]>,
        _params: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<Self::Res>>>> + Send + 'a>> {
        Box::pin(async move {
            Ok(Response::builder()
                .status(200)
                .body(vec![String::from("get_many"), String::from("response")])?)
        })
    }
}

impl<'a> Create<'a> for MyController {
    type Req = String;
    type Res = String;

    fn create(
        self: Arc<Self>,
        _req: Request<Self::Req>,
        _params: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Self::Res>>> + Send + 'a>> {
        Box::pin(async move {
            Ok(Response::builder()
                .status(200)
                .body(String::from("create response"))?)
        })
    }
}

impl<'a> Update<'a> for MyController {
    type Req = String;
    type Res = String;

    fn update(
        self: Arc<Self>,
        _req: Request<Self::Req>,
        _params: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Self::Res>>> + Send + 'a>> {
        Box::pin(async move {
            Ok(Response::builder()
                .status(200)
                .body(String::from("update response"))?)
        })
    }
}

#[test]
fn test() {
    use rustserve::Protect;
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
        guards: vec![http_guard.clone()],
    });

    let routes = vec![
        Route::new("/:version/resource", my_controller.clone()),
        Route::new(
            format!("/:version/resource/:{}", my_controller.clone().prefix()),
            my_controller.clone(),
        ),
    ];

    let body_bytes = vec![];

    tracing::subscriber::with_default(subscriber, || {
        let req = Request::builder()
            .uri("/1/resource/")
            .method("GET")
            .body(&body_bytes[..])
            .unwrap();
        let res = futures::executor::block_on(rustserve::route_request(req, &routes)).unwrap();
        assert!(res.status() == 200);
        assert!(res.body() == b"[\"get_many\",\"response\"]");

        let req = Request::builder()
            .uri("/1/resource/1")
            .method("GET")
            .body(&body_bytes[..])
            .unwrap();
        let res = futures::executor::block_on(rustserve::route_request(req, &routes)).unwrap();
        assert!(res.status() == 200);
        assert!(res.body() == b"\"get_one response\"");

        let body = serde_json::to_vec("test").unwrap();
        let req = Request::builder()
            .uri("/1/resource")
            .method("POST")
            .body(&body[..])
            .unwrap();
        let res = futures::executor::block_on(rustserve::route_request(req, &routes)).unwrap();
        assert!(res.status() == 200);
        assert!(res.body() == b"\"create response\"");

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
    });
}
