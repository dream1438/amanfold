//! Server halves of the protocols, so the Swift, Kotlin, Python and
//! JavaScript binding tests can run sign-up, login and sync end to end
//! without a real server. Behind the `test-server` feature: apps must never
//! ship it, and the release build of the bindings does not include it.

use std::sync::{Arc, Mutex};

use amanfold_core::{account, auth};
use uuid::Uuid;

use crate::{AmanfoldError, Result, dec, enc, lock, wrong_step};

/// A stand-in for the Amanfold server's authentication service.
#[cfg_attr(feature = "uniffi", derive(uniffi::Object))]
pub struct TestServer {
    keys: auth::ServerKeys,
    login: Mutex<Option<(Uuid, auth::ServerLoginState)>>,
}

impl Default for TestServer {
    fn default() -> Self {
        Self {
            keys: auth::ServerKeys::generate(),
            login: Mutex::new(None),
        }
    }
}

fn account_id(s: &str) -> Result<Uuid> {
    Uuid::parse_str(s).map_err(|_| crate::invalid("account id"))
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
impl TestServer {
    /// New server with fresh keys.
    #[cfg_attr(feature = "uniffi", uniffi::constructor)]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// The server's OPAQUE public key (what apps pin).
    pub fn public_key(&self) -> Vec<u8> {
        self.keys.public_key()
    }

    /// Pre-login answer for a stored bundle.
    pub fn prelogin(&self, bundle: Vec<u8>) -> Result<Vec<u8>> {
        let b: account::AccountBundle = dec(&bundle)?;
        Ok(enc(&b.prelogin()))
    }

    /// Registration step 1 (server).
    pub fn registration_response(
        &self,
        account_id_text: String,
        request: Vec<u8>,
    ) -> Result<Vec<u8>> {
        Ok(auth::server_registration_start(
            &self.keys,
            &request,
            &account_id(&account_id_text)?,
        )?)
    }

    /// Registration step 2 (server): the record to store.
    pub fn registration_record(&self, upload: Vec<u8>) -> Result<Vec<u8>> {
        Ok(auth::server_registration_finish(&upload)?)
    }

    /// Login step 1 (server).
    pub fn login_response(
        &self,
        account_id_text: String,
        record: Vec<u8>,
        request: Vec<u8>,
    ) -> Result<Vec<u8>> {
        let id = account_id(&account_id_text)?;
        let (st, resp) = auth::server_login_start(&self.keys, Some(&record), &request, &id)?;
        *lock(&self.login) = Some((id, st));
        Ok(resp)
    }

    /// Login step 2 (server): the session key, or `Decrypt` on failure.
    pub fn login_finish(&self, finalization: Vec<u8>) -> Result<Vec<u8>> {
        let (id, st) = lock(&self.login)
            .take()
            .ok_or_else(|| wrong_step("no login in progress"))?;
        let k = auth::server_login_finish(st, &finalization, &id)
            .map_err(|_| AmanfoldError::Decrypt)?;
        Ok(k.expose().to_vec())
    }
}
