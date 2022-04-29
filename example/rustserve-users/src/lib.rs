use rustserve::Controller;
use rustserve::Guard;
use rustserve::HttpError;
use rustserve::Outcome;
use rustserve::{deserialize, serialize};

use http::{Request, Response};

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::collections::HashMap;

pub struct UsersController {
}

impl UsersController {
    async fn get_user(
        self: Arc<Self>,
        _user_id: String,
    ) -> anyhow::Result<Response<()>> {
        Ok(Response::builder()
            .status(200)
            .body(())
            .map_err(anyhow::Error::from)?)
    }

    async fn create_user(
        self: Arc<Self>,
        _req: Request<()>,
    ) -> anyhow::Result<Response<()>> {
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
        let user_id = match params.get("user_id") {
            Some(user_id) => user_id,
            None => return self.internal_server_error(anyhow::Error::msg("missing user_id parameter")),
        }.clone();

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
