//! A [`tower_layer::Layer`] that enables the [`Tx`](crate::Tx) extractor.

use std::marker::PhantomData;

use axum_core::response::IntoResponse;
use bytes::Bytes;
use futures_core::future::BoxFuture;
use http_body::Body;

use crate::extension::Extension;

/// A [`tower_layer::Layer`] that enables the [`Tx`] extractor.
///
/// This layer adds a lazily-initialised transaction to the [request extensions]. The first time the
/// [`Tx`] extractor is used on a request, a connection is acquired from the configured
/// [`sqlx::Pool`] and a transaction is started on it. The same transaction will be returned for
/// subsequent uses of [`Tx`] on the same request. The inner service is then called as normal. Once
/// the inner service responds, the transaction is committed or rolled back depending on the status
/// code of the response.
///
/// [`Tx`]: crate::Tx
/// [request extensions]: https://docs.rs/http/latest/http/struct.Extensions.html
pub struct Layer<DB: sqlx::Database, E> {
    pool: sqlx::Pool<DB>,
    _error: PhantomData<E>,
}

impl<DB: sqlx::Database, E> From<sqlx::Pool<DB>> for Layer<DB, E>
where
    E: IntoResponse,
    sqlx::Error: Into<E>,
{
    fn from(value: sqlx::Pool<DB>) -> Self {
        Self {
            pool: value,
            _error: PhantomData,
        }
    }
}

impl<DB: sqlx::Database, E> Clone for Layer<DB, E> {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            _error: self._error,
        }
    }
}

impl<DB: sqlx::Database, S, E> tower_layer::Layer<S> for Layer<DB, E>
where
    E: IntoResponse,
    sqlx::Error: Into<E>,
{
    type Service = Service<DB, S, E>;

    fn layer(&self, inner: S) -> Self::Service {
        Service {
            pool: self.pool.clone(),
            inner,
            _error: self._error,
        }
    }
}

/// A [`tower_service::Service`] that enables the [`Tx`](crate::Tx) extractor.
///
/// See [`Layer`] for more information.
pub struct Service<DB: sqlx::Database, S, E> {
    pool: sqlx::Pool<DB>,
    inner: S,
    _error: PhantomData<E>,
}

// can't simply derive because `DB` isn't `Clone`
impl<DB: sqlx::Database, S: Clone, E> Clone for Service<DB, S, E> {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            inner: self.inner.clone(),
            _error: self._error,
        }
    }
}

impl<DB: sqlx::Database, S, E, ReqBody, ResBody> tower_service::Service<http::Request<ReqBody>>
    for Service<DB, S, E>
where
    S: tower_service::Service<
        http::Request<ReqBody>,
        Response = http::Response<ResBody>,
        Error = std::convert::Infallible,
    >,
    S::Future: Send + 'static,
    E: IntoResponse,
    sqlx::Error: Into<E>,
    ResBody: Body<Data = Bytes> + Send + 'static,
    ResBody::Error: Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
{
    type Response = http::Response<axum_core::body::Body>;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(|err| match err {})
    }

    fn call(&mut self, mut req: http::Request<ReqBody>) -> Self::Future {
        let ext = Extension::from(self.pool.clone());
        req.extensions_mut().insert(ext.clone());

        let res = self.inner.call(req);

        Box::pin(async move {
            let res = res.await.unwrap(); // inner service is infallible

            if !res.status().is_server_error() && !res.status().is_client_error() {
                if let Err(error) = ext.resolve().await {
                    return Ok(error.into().into_response());
                }
            }

            Ok(res.map(axum_core::body::Body::new))
        })
    }
}

#[cfg(test)]
mod tests {
    use tokio::net::TcpListener;

    use crate::Error;

    use super::Layer;

    // The trait shenanigans required by axum for layers are significant, so this "test" ensures
    // we've got it right.
    #[allow(unused, unreachable_code, clippy::diverging_sub_expression)]
    fn layer_compiles() {
        let pool: sqlx::Pool<sqlx::Sqlite> = todo!();

        let layer = Layer::<_, Error>::from(pool);

        let app = axum::Router::new()
            .route("/", axum::routing::get(|| async { "hello" }))
            .layer(layer);

        let listener: TcpListener = todo!();
        axum::serve(listener, app);
    }
}
