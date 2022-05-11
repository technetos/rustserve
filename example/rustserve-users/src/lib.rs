use rustserve::Controller;
use rustserve::Entity;
use rustserve::Guard;
use rustserve::HttpError;
use rustserve::Outcome;
use rustserve::{deserialize, serialize};

use http::{Request, Response};

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

// A guard for any request that requires the user be logged in

#[derive(Debug)]
struct SessionGuard {
    // A mock session token, simply to portray checking against some kind of value or db for a
    // session
    session_token: String,
}

impl Guard for SessionGuard {
    fn verify<'a>(
        self: Arc<Self>,
        req: Request<&'a [u8]>,
        params: HashMap<String, String>,
        entity: Arc<dyn Entity>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Outcome<'a>>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(session_token) = req.headers().get("session_token") {
                if self.session_token == *session_token {
                    return Ok(Outcome::Forward(req, params));
                }
            }
            Ok(Outcome::Respond(
                // The error handler passed into verify is the controller that this guard is
                // protecting.  Each controller defines how its errors behave all the way down
                // the call stack.
                entity
                    .unauthorized(anyhow::Error::msg("unauthorized access").into())
                    .await?,
            ))
        })
    }
}

struct HttpGuard {}

impl Guard for HttpGuard {
    fn verify<'a>(
        self: Arc<Self>,
        req: Request<&'a [u8]>,
        params: HashMap<String, String>,
        entity: Arc<dyn Entity>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Outcome<'a>>> + Send + 'a>> {
        Box::pin(async move {
            match req.method().as_str() {
                "PUT" if params.get(&entity.clone().prefix()).is_none() => {
                    Ok(Outcome::Respond(entity.not_found(None).await?))
                }
                "POST" if params.get(&entity.clone().prefix()).is_some() => {
                    Ok(Outcome::Respond(entity.not_found(None).await?))
                }
                _ => Ok(Outcome::Forward(req, params)),
            }
        })
    }
}

// ---------------------------------------------------

#[derive(Debug)]
pub struct ResourceController {}

impl ResourceController {
    async fn create(self: Arc<Self>, _req: Request<String>) -> anyhow::Result<Response<()>> {
        Ok(Response::builder()
            .status(200)
            .body(())
            .map_err(anyhow::Error::from)?)
    }

    async fn get_one(self: Arc<Self>, id: String) -> anyhow::Result<Response<()>> {
        Ok(Response::builder()
            .status(200)
            .body(())
            .map_err(anyhow::Error::from)?)
    }

    async fn get_all(self: Arc<Self>) -> anyhow::Result<Response<()>> {
        Ok(Response::builder()
            .status(200)
            .body(())
            .map_err(anyhow::Error::from)?)
    }

    async fn update(
        self: Arc<Self>,
        _req: Request<String>,
        id: String,
    ) -> anyhow::Result<Response<()>> {
        Ok(Response::builder()
            .status(200)
            .body(())
            .map_err(anyhow::Error::from)?)
    }
}

impl Entity for ResourceController {
    fn prefix(self: Arc<Self>) -> String {
        "id".into()
    }
}

impl Controller for ResourceController {
    fn get<'a>(
        self: Arc<Self>,
        _req: Request<&'a [u8]>,
        params: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(id) = params.get(&self.clone().prefix()) {
                Ok(serialize(self.clone().get_one(id.clone()).await?)?.map(|_| vec![]))
            } else {
                Ok(serialize(self.clone().get_all().await?)?.map(|_| vec![]))
            }
        })
    }

    fn post<'a>(
        self: Arc<Self>,
        req: Request<&'a [u8]>,
        _params: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
        Box::pin(async move {
            let req = match deserialize(req) {
                Ok(r) => r,
                Err(e) => return self.clone().bad_request(e.into()).await,
            };

            Ok(self.clone().create(req).await?.map(|_| vec![]))
        })
    }

    fn put<'a>(
        self: Arc<Self>,
        req: Request<&'a [u8]>,
        params: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
        Box::pin(async move {
            let req = match deserialize(req) {
                Ok(r) => r,
                Err(e) => return self.clone().bad_request(e.into()).await,
            };
            let id = params.get(&self.clone().prefix()).unwrap().clone();

            Ok(self.clone().update(req, id).await?.map(|_| vec![]))
        })
    }
}

impl HttpError for ResourceController {
    fn not_found(
        self: Arc<Self>,
        e: Option<anyhow::Error>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send>> {
        Box::pin(async move { handle_error(404, e).await })
    }

    fn bad_request(
        self: Arc<Self>,
        e: Option<anyhow::Error>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send>> {
        Box::pin(async move { handle_error(400, e).await })
    }

    fn unauthorized(
        self: Arc<Self>,
        e: Option<anyhow::Error>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send>> {
        Box::pin(async move { handle_error(401, e).await })
    }

    fn internal_server_error(
        self: Arc<Self>,
        e: Option<anyhow::Error>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send>> {
        Box::pin(async move { handle_error(500, e).await })
    }
}

async fn handle_error(code: u16, e: Option<anyhow::Error>) -> anyhow::Result<Response<Vec<u8>>> {
    if let Some(err) = e {
        Ok(serialize(
            Response::builder().status(code).body(format!("{err}"))?,
        )?)
    } else {
        Ok(Response::builder().status(code).body(vec![])?)
    }
}

#[test]
fn test() {
    use rustserve::Protect;
    use rustserve::Route;

    let http_guard = Arc::new(HttpGuard {});

    let session_guard = Arc::new(SessionGuard {
        session_token: String::from("my_session_token"),
    });

    let users_controller = Arc::new(Protect {
        controller: Arc::new(ResourceController {}),
        guards: vec![http_guard.clone(), session_guard.clone()],
    });

    let routes = vec![
        Route::new("/:version/resource", users_controller.clone()),
        Route::new("/:version/resource/:id", users_controller.clone()),
    ];

    let body_bytes = vec![];

    let req = Request::builder()
        .uri("/1/resource/")
        .method("GET")
        .body(&body_bytes[..])
        .unwrap();
    let res = futures::executor::block_on(rustserve::route_request(req, &routes)).unwrap();
    assert!(res.status() == 401);
    assert!(res.body() == b"\"unauthorized access\"");

    let req = Request::builder()
        .uri("/1/resource/")
        .method("GET")
        .header("session_token", "my_session_token")
        .body(&body_bytes[..])
        .unwrap();
    let res = futures::executor::block_on(rustserve::route_request(req, &routes)).unwrap();
    assert!(res.status() == 200);

    let req = Request::builder()
        .uri("/1/resource/1")
        .method("GET")
        .header("session_token", "my_session_token")
        .body(&body_bytes[..])
        .unwrap();
    let res = futures::executor::block_on(rustserve::route_request(req, &routes)).unwrap();
    assert!(res.status() == 200);
    assert!(res.body() == &Vec::<u8>::new());

    let body = serde_json::to_vec("test").unwrap();
    let req = Request::builder()
        .uri("/1/resource")
        .method("POST")
        .body(&body[..])
        .unwrap();
    let res = futures::executor::block_on(rustserve::route_request(req, &routes)).unwrap();
    assert!(res.status() == 401);
    assert!(res.body() == b"\"unauthorized access\"");

    let body = serde_json::to_vec("test").unwrap();
    let req = Request::builder()
        .uri("/1/resource")
        .method("POST")
        .header("session_token", "my_session_token")
        .body(&body[..])
        .unwrap();
    let res = futures::executor::block_on(rustserve::route_request(req, &routes)).unwrap();
    assert!(res.status() == 200);
    assert!(res.body() == &Vec::<u8>::new());

    let body = serde_json::to_vec("test").unwrap();
    let req = Request::builder()
        .uri("/1/resource/1")
        .method("POST")
        .header("session_token", "my_session_token")
        .body(&body[..])
        .unwrap();
    let res = futures::executor::block_on(rustserve::route_request(req, &routes)).unwrap();
    dbg!(res.body());
    assert!(res.status() == 404);
    assert!(res.body() == &Vec::<u8>::new());
}
