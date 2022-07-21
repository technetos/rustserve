use rustserve::base::{Base, Create, Read, RequestParser, ResponseFormatter, Update};
use rustserve::error::{ErrorHandler, HttpError, HttpErrorDefault};
use rustserve::Controller;

use http::{Request, Response};

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use futures::future::BoxFuture;

type ResultFuture<'a, T> = Pin<Box<dyn Future<Output = anyhow::Result<T>> + Send + 'a>>;

struct MyController {}

impl Base for MyController {}

impl ErrorHandler for MyController {}

impl HttpErrorDefault for MyController {}

impl Controller for MyController {
    fn get<'a>(
        self: Arc<Self>,
        req: Request<&'a [u8]>,
        params: HashMap<String, String>,
    ) -> ResultFuture<'a, Response<Vec<u8>>> {
        Box::pin(async move {
            Ok(if params.get(&self.clone().prefix()).is_some() {
                <MyController as Read<'a, ReadResponse>>::read(self.clone(), req, params).await?
            } else {
                <MyController as Read<'a, Paginated<ReadResponse>>>::read(self.clone(), req, params)
                    .await?
            })
        })
    }

    fn post<'a>(
        self: Arc<Self>,
        req: Request<&'a [u8]>,
        params: HashMap<String, String>,
    ) -> ResultFuture<'a, Response<Vec<u8>>> {
        Box::pin(async move {
            self.clone()
                .create(self.parse(req).await?, params)
                .await
        })
    }

    fn put<'a>(
        self: Arc<Self>,
        req: Request<&'a [u8]>,
        params: HashMap<String, String>,
    ) -> ResultFuture<'a, Response<Vec<u8>>> {
        Box::pin(async move {
            self.clone()
                .update(self.parse(req).await?, params)
                .await
        })
    }
}

#[derive(serde::Serialize)]
pub struct ReadResponse {
    msg: String,
}

impl<'a> ResponseFormatter<'a, ReadResponse> for MyController {}

impl<'a> Read<'a, ReadResponse> for MyController {
    fn read(
        self: Arc<Self>,
        _req: Request<&'a [u8]>,
        _params: HashMap<String, String>,
    ) -> ResultFuture<'a, Response<Vec<u8>>> {
        Box::pin(async move {
            Ok(self
                .format(Response::builder().status(200).body(ReadResponse {
                    msg: String::from("read one response"),
                })?)
                .await?)
        })
    }
}

#[derive(serde::Serialize)]
pub struct Paginated<T> {
    content: Vec<T>,
    #[serde(flatten)]
    pagination: Pagination,
}

#[derive(serde::Serialize)]
pub struct Pagination {
    offset: u64,
    limit: u64,
    total: u64,
}

impl<'a> ResponseFormatter<'a, Paginated<ReadResponse>> for MyController {}

impl<'a> Read<'a, Paginated<ReadResponse>> for MyController {
    fn read(
        self: Arc<Self>,
        req: Request<&'a [u8]>,
        _params: HashMap<String, String>,
    ) -> ResultFuture<'a, Response<Vec<u8>>> {
        Box::pin(async move {
            let query_params = self.clone().extract_query_params(req.uri().query());

            let offset = query_params
                .get("offset")
                .map(|offset| offset.parse::<usize>())
                .unwrap_or(Ok(0))?;

            let limit = query_params
                .get("limit")
                .map(|limit| limit.parse::<usize>())
                .unwrap_or(Ok(0))?;

            let content = vec![
                ReadResponse {
                    msg: String::from("read many"),
                },
                ReadResponse {
                    msg: String::from("response"),
                },
            ];

            let total = content.len();

            if offset > total {
                return self
                    .bad_request(anyhow::Error::msg("offset greater than total").into())
                    .await;
            }

            let res_body = Paginated {
                pagination: Pagination {
                    offset: offset as u64,
                    limit: limit as u64,
                    total: total as u64,
                },
                content: content.into_iter().skip(offset).take(limit).collect(),
            };

            Ok(self
                .format(Response::builder().status(200).body(res_body)?)
                .await?)
        })
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct CreateRequest {
    msg: String,
}

#[derive(serde::Serialize)]
pub struct CreateResponse {
    msg: String,
}

impl<'a> RequestParser<'a, CreateRequest> for MyController {}
impl<'a> ResponseFormatter<'a, CreateResponse> for MyController {}

impl<'a> Create<'a, CreateRequest, CreateResponse> for MyController {
    fn create(
        self: Arc<Self>,
        _req: Request<CreateRequest>,
        _params: HashMap<String, String>,
    ) -> ResultFuture<'a, Response<Vec<u8>>> {
        Box::pin(async move {
            Ok(self
                .format(Response::builder().status(200).body(CreateResponse {
                    msg: String::from("create response"),
                })?)
                .await?)
        })
    }
}

impl<'a> RequestParser<'a, String> for MyController {}
impl<'a> ResponseFormatter<'a, String> for MyController {}

impl<'a> Update<'a, String, String> for MyController {
    fn update(
        self: Arc<Self>,
        _req: Request<String>,
        _params: HashMap<String, String>,
    ) -> ResultFuture<'a, Response<Vec<u8>>> {
        Box::pin(async move {
            Ok(self
               .format(
                    Response::builder()
                        .status(200)
                        .body(String::from("update response"))?,
                )
                .await?)
        })
    }
}

#[test]
fn test() {
    use rustserve::Route;
    use tracing::Level;

    let my_controller = Arc::new(MyController {});

    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        // all spans/events with a level higher than TRACE (e.g, debug, info, warn, etc.)
        // will be written to stdout.
        .with_max_level(Level::TRACE)
        // builds the subscriber.
        .finish();

    let routes = Arc::new(vec![
        Route::new("/:version/resource", my_controller.clone()),
        Route::new(
            format!("/:version/resource/:{}", my_controller.clone().prefix()),
            my_controller.clone(),
        ),
    ]);

    tracing::subscriber::with_default(subscriber, || {
        let body = vec![];
        let req = Request::builder()
            .uri("/1/resource/?limit=1")
            .method("GET")
            .body(&body[..])
            .unwrap();
        let res = futures::executor::block_on(rustserve::route_request(req, routes.clone())).unwrap();
        dbg!(res.body().iter().map(|ch| *ch as char).collect::<String>());
        assert!(res.status() == 200);
        assert!(res.body() == b"[{\"msg\":\"read many\"},{\"msg\":\"response\"}]");

        let body = vec![];
        let req = Request::builder()
            .uri("/1/resource/1")
            .method("GET")
            .body(&body[..])
            .unwrap();
        let res = futures::executor::block_on(rustserve::route_request(req, routes.clone())).unwrap();
        assert!(res.status() == 200);
        assert!(res.body() == b"{\"msg\":\"read one response\"}");

        let body = serde_json::to_vec(&CreateRequest { msg: "test".into() }).unwrap();
        let req = Request::builder()
            .uri("/1/resource?a=1")
            .method("POST")
            .body(&body[..])
            .unwrap();
        let res = futures::executor::block_on(rustserve::route_request(req, routes.clone())).unwrap();
        assert!(res.status() == 200);
        assert!(res.body() == b"{\"msg\":\"create response\"}");

        let body = serde_json::to_vec("test").unwrap();
        let req = Request::builder()
            .uri("/1/resource/1")
            .method("PUT")
            .body(&body[..])
            .unwrap();
        let res = futures::executor::block_on(rustserve::route_request(req, routes.clone())).unwrap();
        assert!(res.status() == 200);
        assert!(res.body() == b"\"update response\"");

        let body = serde_json::to_vec("test").unwrap();
        let req = Request::builder()
            .uri("/1/resource/1")
            .method("POST")
            .body(&body[..])
            .unwrap();
        let res = futures::executor::block_on(rustserve::route_request(req, routes.clone())).unwrap();
        assert!(res.status() == 400);
        assert!(res.body() == b"\"POST does not accept an id\"");

        let body = serde_json::to_vec("test").unwrap();
        let req = Request::builder()
            .uri("/1/resource")
            .method("PUT")
            .body(&body[..])
            .unwrap();
        let res = futures::executor::block_on(rustserve::route_request(req, routes.clone())).unwrap();
        assert!(res.status() == 404);
        assert!(res.body() == b"\"PUT requires an id\"");
    });
}
