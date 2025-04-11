use std::marker::PhantomData;

use crate::Layer;

/// Configuration for [`Tx`](crate::Tx) extractors.
///
/// Use `Config` to configure and create a [`State`] and [`Layer`].
///
/// Access the `Config` API from [`Tx::config`](crate::Tx::config).
///
/// ```
/// # async fn foo() {
/// # let pool: sqlx::SqlitePool = todo!();
/// type Tx = axum_sqlx_tx::Tx<sqlx::Sqlite>;
///
/// let config = Tx::config(pool);
/// # }
/// ```
pub struct Config<DB: sqlx::Database, LayerError> {
    pool: sqlx::Pool<DB>,
    _layer_error: PhantomData<LayerError>,
}

impl<DB: sqlx::Database, LayerError> Config<DB, LayerError>
where
    LayerError: axum_core::response::IntoResponse,
    sqlx::Error: Into<LayerError>,
{
    /// Change the layer error type.
    pub fn layer_error<E>(self) -> Config<DB, E>
    where
        sqlx::Error: Into<E>,
    {
        Config {
            pool: self.pool,
            _layer_error: PhantomData,
        }
    }

    /// Create a [`State`] and [`Layer`] to enable the [`Tx`](crate::Tx) extractor.
    pub fn setup(self) -> (sqlx::Pool<DB>, Layer<DB, LayerError>) {
        let layer = Layer::from(self.pool.clone());
        (self.pool, layer)
    }
}

impl<DB: sqlx::Database, LayerError> From<sqlx::Pool<DB>> for Config<DB, LayerError>
where
    LayerError: axum_core::response::IntoResponse,
    sqlx::Error: Into<LayerError>,
{
    fn from(value: sqlx::Pool<DB>) -> Self {
        Self {
            pool: value,
            _layer_error: PhantomData,
        }
    }
}
