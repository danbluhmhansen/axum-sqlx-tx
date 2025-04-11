use std::sync::Arc;

use parking_lot::{lock_api::ArcMutexGuard, Mutex, RawMutex};
use sqlx::Transaction;

use crate::Error;

/// The request extension.
pub(crate) struct Extension<DB: sqlx::Database>(Arc<Mutex<LazyTransaction<DB>>>);

impl<DB: sqlx::Database> Extension<DB> {
    pub(crate) async fn acquire(
        &self,
    ) -> Result<ArcMutexGuard<RawMutex, LazyTransaction<DB>>, Error> {
        let mut tx = self.0.try_lock_arc().ok_or(Error::OverlappingExtractors)?;
        tx.acquire().await?;

        Ok(tx)
    }

    pub(crate) async fn resolve(&self) -> Result<(), sqlx::Error> {
        if let Some(mut tx) = self.0.try_lock_arc() {
            tx.resolve().await?;
        }
        Ok(())
    }
}

impl<DB: sqlx::Database> From<sqlx::Pool<DB>> for Extension<DB> {
    fn from(value: sqlx::Pool<DB>) -> Self {
        Self(Arc::new(Mutex::new(LazyTransaction::new(value))))
    }
}

impl<DB: sqlx::Database> Clone for Extension<DB> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

/// The lazy transaction.
pub(crate) struct LazyTransaction<DB: sqlx::Database>(LazyTransactionState<DB>);

enum LazyTransactionState<DB: sqlx::Database> {
    Unacquired(sqlx::Pool<DB>),
    Acquired(Transaction<'static, DB>),
    Resolved,
}

impl<DB: sqlx::Database> LazyTransaction<DB> {
    fn new(pool: sqlx::Pool<DB>) -> Self {
        Self(LazyTransactionState::Unacquired(pool))
    }

    pub(crate) fn as_ref(&self) -> &Transaction<'static, DB> {
        match &self.0 {
            LazyTransactionState::Unacquired { .. } => {
                panic!("BUG: exposed unacquired LazyTransaction")
            }
            LazyTransactionState::Acquired(tx) => tx,
            LazyTransactionState::Resolved => panic!("BUG: exposed resolved LazyTransaction"),
        }
    }

    pub(crate) fn as_mut(&mut self) -> &mut Transaction<'static, DB> {
        match &mut self.0 {
            LazyTransactionState::Unacquired { .. } => {
                panic!("BUG: exposed unacquired LazyTransaction")
            }
            LazyTransactionState::Acquired(tx) => tx,
            LazyTransactionState::Resolved => panic!("BUG: exposed resolved LazyTransaction"),
        }
    }

    async fn acquire(&mut self) -> Result<(), Error> {
        match &self.0 {
            LazyTransactionState::Unacquired(pool) => {
                let tx = pool.begin().await?;
                self.0 = LazyTransactionState::Acquired(tx);
                Ok(())
            }
            LazyTransactionState::Acquired { .. } => Ok(()),
            LazyTransactionState::Resolved => Err(Error::OverlappingExtractors),
        }
    }

    pub(crate) async fn resolve(&mut self) -> Result<(), sqlx::Error> {
        match std::mem::replace(&mut self.0, LazyTransactionState::Resolved) {
            LazyTransactionState::Unacquired { .. } | LazyTransactionState::Resolved => Ok(()),
            LazyTransactionState::Acquired(tx) => tx.commit().await,
        }
    }

    pub(crate) async fn commit(&mut self) -> Result<(), sqlx::Error> {
        match std::mem::replace(&mut self.0, LazyTransactionState::Resolved) {
            LazyTransactionState::Unacquired { .. } => {
                panic!("BUG: tried to commit unacquired transaction")
            }
            LazyTransactionState::Acquired(tx) => tx.commit().await,
            LazyTransactionState::Resolved => panic!("BUG: tried to commit resolved transaction"),
        }
    }
}
