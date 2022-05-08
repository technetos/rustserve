use rustserve::Controller;
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

struct UserGuard {
    // A mock example of a database of users.  Ideally the UserGuard would have a
    // user_service: Arc<UserService> and would perform actual user checks using
    // that service.
    valid_user_ids: Vec<String>,
}

impl Guard for UserGuard {
    fn verify<'a>(
        self: Arc<Self>,
        req: Request<&'a [u8]>,
        params: HashMap<String, String>,
        errors: Arc<dyn HttpError>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Outcome<'a>>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(user_id) = params.get("user_id") {
                if self.valid_user_ids.contains(user_id) {
                    return Ok(Outcome::Forward(req, params));
                }
            }
            Ok(Outcome::Respond(
                // The error handler passed into verify is the controller that this guard is
                // protecting.  Each controller defines how its errors behave all the way down
                // the call stack.
                errors
                    .unauthorized(anyhow::Error::msg("unauthorized access"))
                    .await?,
            ))
        })
    }
}

// A controller for managing users, creating, updating, so on

pub struct UsersController {}

impl UsersController {
    async fn get_user(self: Arc<Self>, _user_id: String) -> anyhow::Result<Response<()>> {
        Ok(Response::builder()
            .status(200)
            .body(())
            .map_err(anyhow::Error::from)?)
    }

    async fn create_user(self: Arc<Self>, _req: Request<()>) -> anyhow::Result<Response<()>> {
        Ok(Response::builder()
            .status(200)
            .body(())
            .map_err(anyhow::Error::from)?)
    }
}

impl Controller for UsersController {
    fn get<'a>(
        self: Arc<Self>,
        _req: Request<&'a [u8]>,
        params: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
        let user_id = params.get("user_id").unwrap().clone();
        Box::pin(async move { Ok(serialize(self.clone().get_user(user_id).await?)?) })
    }

    fn post<'a>(
        self: Arc<Self>,
        req: Request<&'a [u8]>,
        _params: HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send + 'a>> {
        let req = match deserialize(req) {
            Ok(r) => r,
            Err(e) => return self.clone().bad_request(e.into()),
        };

        Box::pin(async move { Ok(serialize(self.clone().create_user(req).await?)?) })
    }
}

impl HttpError for UsersController {
    fn not_found(
        self: Arc<Self>,
        e: anyhow::Error,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send>> {
        Box::pin(async move {
            Ok(serialize(
                Response::builder().status(404).body(format!("{e}"))?,
            )?)
        })
    }

    fn bad_request(
        self: Arc<Self>,
        e: anyhow::Error,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send>> {
        Box::pin(async move {
            Ok(serialize(
                Response::builder().status(400).body(format!("{e}"))?,
            )?)
        })
    }

    fn unauthorized(
        self: Arc<Self>,
        e: anyhow::Error,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send>> {
        Box::pin(async move {
            Ok(serialize(
                Response::builder().status(401).body(format!("{e}"))?,
            )?)
        })
    }

    fn internal_server_error(
        self: Arc<Self>,
        e: anyhow::Error,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Response<Vec<u8>>>> + Send>> {
        Box::pin(async move {
            Ok(serialize(
                Response::builder().status(500).body(format!("{e}"))?,
            )?)
        })
    }
}
