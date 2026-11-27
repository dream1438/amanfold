//! Server login with OPAQUE (RFC 9807).
//!
//! OPAQUE is a password-authenticated key exchange: the client proves it knows
//! the login key without ever sending it, and the server stores a record from
//! which the login key cannot be recovered without an online guess against the
//! server for every try. Combined with the Secret Key, a stolen record is
//! useless: the input to OPAQUE already carries 128 bits the server never had.
//!
//! The OPAQUE input is the `auth_key` from [`crate::kdf::derive_account_secrets`],
//! which is already stretched by Argon2id, so the OPAQUE key-stretching
//! function is the identity. It is derived under a different HKDF label from
//! the unlock key, so nothing the server sees helps decrypt the vault.
//!
//! Cipher suite: Ristretto255 OPRF, 3DH key exchange over Ristretto255 with
//! SHA-512.
//!
//! Message flow (all messages are opaque byte strings for the transport):
//!
//! ```text
//! Registration            client                          server
//!   client_registration_start  ── request ──►  server_registration_start
//!   client_registration_finish ◄─ response ──
//!                              ── upload  ──►  server_registration_finish → store record
//! Login
//!   client_login_start         ── request ──►  server_login_start (keeps state)
//!   client_login_finish        ◄─ response ──
//!                              ── finalize ─►  server_login_finish → session key
//! ```
//!
//! Both sides end with the same 64-byte session key, used to authenticate the
//! session (for example to derive an API token). A wrong password, wrong
//! Secret Key or impostor server makes `client_login_finish` fail.

use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use opaque_ke::{
    CipherSuite, ClientLogin, ClientLoginFinishParameters, ClientRegistration,
    ClientRegistrationFinishParameters, CredentialFinalization, CredentialRequest,
    CredentialResponse, Identifiers, RegistrationRequest, RegistrationResponse, RegistrationUpload,
    Ristretto255, ServerLogin, ServerLoginParameters, ServerRegistration, ServerSetup, TripleDh,
    ksf::Identity,
};
use rand_core::OsRng;
use sha2::Sha256;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
    Error, Result,
    account::PreLogin,
    aead::Context,
    kdf::{KdfParams, SALT_LEN},
    keys::{AccountKeys, PublicKeys},
    secret::SecretBytes,
};

/// Amanfold's OPAQUE cipher suite.
pub struct Suite;

impl CipherSuite for Suite {
    type OprfCs = Ristretto255;
    type KeyExchange = TripleDh<Ristretto255, sha2::Sha512>;
    type Ksf = Identity;
}

/// Server identity bound into every exchange. A record registered for one
/// service cannot be replayed against another.
pub const SERVER_IDENTITY: &[u8] = b"amanfold.com";

fn identifiers(credential_id: &[u8]) -> Identifiers<'_> {
    Identifiers {
        client: Some(credential_id),
        server: Some(SERVER_IDENTITY),
    }
}

fn proto<E>(_: E) -> Error {
    Error::Decrypt
}

/// Session key shared by client and server after a successful login.
pub type SessionKey = SecretBytes<64>;

fn session_key(b: &[u8]) -> SessionKey {
    let mut a = [0u8; 64];
    a.copy_from_slice(b);
    SessionKey::from_array(a)
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// Client state between registration messages. Keep in memory only.
pub struct ClientRegistrationState(ClientRegistration<Suite>);

/// Client state between login messages. Keep in memory only.
pub struct ClientLoginState(ClientLogin<Suite>);

/// Step 1 of registration (also used to re-register after a password change).
pub fn client_registration_start(
    auth_key: &SecretBytes<32>,
) -> Result<(ClientRegistrationState, Vec<u8>)> {
    let r = ClientRegistration::<Suite>::start(&mut OsRng, auth_key.expose()).map_err(proto)?;
    Ok((
        ClientRegistrationState(r.state),
        r.message.serialize().to_vec(),
    ))
}

/// Step 3 of registration. Returns the upload message for the server.
///
/// `credential_id` must be the same value the server uses (the account id).
pub fn client_registration_finish(
    state: ClientRegistrationState,
    auth_key: &SecretBytes<32>,
    response: &[u8],
    credential_id: &Uuid,
) -> Result<Vec<u8>> {
    let resp = RegistrationResponse::<Suite>::deserialize(response).map_err(proto)?;
    let r = state
        .0
        .finish(
            &mut OsRng,
            auth_key.expose(),
            resp,
            ClientRegistrationFinishParameters::new(identifiers(credential_id.as_bytes()), None),
        )
        .map_err(proto)?;
    Ok(r.message.serialize().to_vec())
}

/// Step 1 of login.
pub fn client_login_start(auth_key: &SecretBytes<32>) -> Result<(ClientLoginState, Vec<u8>)> {
    let r = ClientLogin::<Suite>::start(&mut OsRng, auth_key.expose()).map_err(proto)?;
    Ok((ClientLoginState(r.state), r.message.serialize().to_vec()))
}

/// Result of a successful client login.
#[derive(Debug)]
pub struct ClientLoginOutcome {
    /// Send to the server to complete the login.
    pub finalization: Vec<u8>,
    /// Shared session key.
    pub session_key: SessionKey,
    /// The server's long-term OPAQUE public key, as stored in the user's
    /// own record. Apps pin this and alert if it ever changes.
    pub server_public_key: Vec<u8>,
}

/// Step 3 of login. Fails with [`Error::Decrypt`] on a wrong password, wrong
/// Secret Key, wrong account or an impostor server.
///
/// If `pinned_server_key` is given, the login also fails unless the server
/// proved possession of that exact key.
pub fn client_login_finish(
    state: ClientLoginState,
    auth_key: &SecretBytes<32>,
    response: &[u8],
    credential_id: &Uuid,
    pinned_server_key: Option<&[u8]>,
) -> Result<ClientLoginOutcome> {
    let resp = CredentialResponse::<Suite>::deserialize(response).map_err(proto)?;
    let r = state
        .0
        .finish(
            &mut OsRng,
            auth_key.expose(),
            resp,
            ClientLoginFinishParameters::new(None, identifiers(credential_id.as_bytes()), None),
        )
        .map_err(proto)?;
    let server_public_key = r.server_s_pk.serialize().to_vec();
    if let Some(pin) = pinned_server_key {
        if !bool::from(subtle::ConstantTimeEq::ct_eq(
            pin,
            server_public_key.as_slice(),
        )) {
            return Err(Error::Decrypt);
        }
    }
    Ok(ClientLoginOutcome {
        finalization: r.message.serialize().to_vec(),
        session_key: session_key(&r.session_key),
        server_public_key,
    })
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

/// The server's long-term OPAQUE secrets (key pair and OPRF seed). Store in an
/// HSM or secret manager; losing it forces every user to re-register.
pub struct ServerKeys(ServerSetup<Suite>);

impl ServerKeys {
    /// Generate new server keys (once per deployment).
    pub fn generate() -> Self {
        Self(ServerSetup::<Suite>::new(&mut OsRng))
    }
    /// Serialise for secret storage.
    pub fn to_bytes(&self) -> Zeroizing<Vec<u8>> {
        Zeroizing::new(self.0.serialize().to_vec())
    }
    /// Load from secret storage.
    pub fn from_bytes(b: &[u8]) -> Result<Self> {
        ServerSetup::<Suite>::deserialize(b)
            .map(Self)
            .map_err(|_| Error::Malformed("server keys"))
    }
    /// The public key clients pin.
    pub fn public_key(&self) -> Vec<u8> {
        self.0.keypair().public().serialize().to_vec()
    }
}

/// Step 2 of registration.
pub fn server_registration_start(
    keys: &ServerKeys,
    request: &[u8],
    credential_id: &Uuid,
) -> Result<Vec<u8>> {
    let req = RegistrationRequest::<Suite>::deserialize(request).map_err(proto)?;
    let r = ServerRegistration::<Suite>::start(&keys.0, req, credential_id.as_bytes())
        .map_err(proto)?;
    Ok(r.message.serialize().to_vec())
}

/// Step 4 of registration. Returns the password record to store for the account.
pub fn server_registration_finish(upload: &[u8]) -> Result<Vec<u8>> {
    let up = RegistrationUpload::<Suite>::deserialize(upload).map_err(proto)?;
    Ok(ServerRegistration::<Suite>::finish(up).serialize().to_vec())
}

/// Server state between login messages. Keep server-side only (it contains
/// key material); expire it after a short timeout.
pub struct ServerLoginState(ServerLogin<Suite>);

impl ServerLoginState {
    /// Serialise for a short-lived server-side store.
    pub fn to_bytes(&self) -> Zeroizing<Vec<u8>> {
        Zeroizing::new(self.0.serialize().to_vec())
    }
    /// Restore.
    pub fn from_bytes(b: &[u8]) -> Result<Self> {
        ServerLogin::<Suite>::deserialize(b)
            .map(Self)
            .map_err(|_| Error::Malformed("login state"))
    }
}

/// Step 2 of login. Pass `record: None` for an unknown account: OPAQUE then
/// answers with a fake response indistinguishable from a real one, so the
/// endpoint does not reveal which accounts exist.
pub fn server_login_start(
    keys: &ServerKeys,
    record: Option<&[u8]>,
    request: &[u8],
    credential_id: &Uuid,
) -> Result<(ServerLoginState, Vec<u8>)> {
    let rec = match record {
        Some(b) => Some(
            ServerRegistration::<Suite>::deserialize(b).map_err(|_| Error::Malformed("record"))?,
        ),
        None => None,
    };
    let req = CredentialRequest::<Suite>::deserialize(request).map_err(proto)?;
    let params = ServerLoginParameters {
        identifiers: identifiers(credential_id.as_bytes()),
        ..Default::default()
    };
    let r = ServerLogin::start(
        &mut OsRng,
        &keys.0,
        rec,
        req,
        credential_id.as_bytes(),
        params,
    )
    .map_err(proto)?;
    Ok((ServerLoginState(r.state), r.message.serialize().to_vec()))
}

/// Step 4 of login. Success means the client knew the login key.
pub fn server_login_finish(
    state: ServerLoginState,
    finalization: &[u8],
    credential_id: &Uuid,
) -> Result<SessionKey> {
    let fin = CredentialFinalization::<Suite>::deserialize(finalization).map_err(proto)?;
    let params = ServerLoginParameters {
        identifiers: identifiers(credential_id.as_bytes()),
        ..Default::default()
    };
    let r = state.0.finish(fin, params).map_err(proto)?;
    Ok(session_key(&r.session_key))
}

/// Bearer token for API requests after a login, derived from the OPAQUE
/// session key by both sides. The server stores only a hash of it, and it
/// never travels in the login messages, so only a party that completed the
/// OPAQUE exchange can hold it.
pub fn api_token(session_key: &SessionKey) -> SecretBytes<32> {
    let hk = Hkdf::<Sha256>::new(None, session_key.expose());
    let mut out = [0u8; 32];
    hk.expand(b"amanfold/v1/api-token", &mut out)
        .expect("32 bytes is a valid HKDF length");
    SecretBytes::from_array(out)
}

/// Length of a server challenge for [`sign_challenge`].
pub const CHALLENGE_LEN: usize = 32;

fn challenge_ctx(account_id: &Uuid, purpose: &str) -> Context {
    Context::new("server-challenge")
        .with(account_id.as_bytes())
        .with(purpose.as_bytes())
}

/// Prove to the server that this device holds the account's private keys,
/// by signing a fresh random challenge the server issued. Used for account
/// recovery: unlocking with the recovery words gives the keys, and this
/// signature lets the server accept a new login (OPAQUE re-registration)
/// without the old password. `purpose` names the operation (for example
/// `"recover"`) so a signature for one cannot be used for another.
pub fn sign_challenge(
    keys: &AccountKeys,
    account_id: &Uuid,
    purpose: &str,
    challenge: &[u8],
) -> Result<[u8; 64]> {
    if challenge.len() != CHALLENGE_LEN {
        return Err(Error::InvalidArgument("challenge length"));
    }
    Ok(keys.sign(&challenge_ctx(account_id, purpose), challenge))
}

/// Server side of [`sign_challenge`].
pub fn verify_challenge(
    public: &PublicKeys,
    account_id: &Uuid,
    purpose: &str,
    challenge: &[u8],
    signature: &[u8],
) -> Result<()> {
    let sig: [u8; 64] = signature.try_into().map_err(|_| Error::BadSignature)?;
    if challenge.len() != CHALLENGE_LEN {
        return Err(Error::BadSignature);
    }
    public.verify(&challenge_ctx(account_id, purpose), challenge, &sig)
}

/// Deterministic, plausible pre-login parameters for a username that has no
/// account, so the pre-login endpoint cannot be used to test which emails are
/// registered. `server_secret` is a long-term secret separate from the OPAQUE
/// keys; `username` should be normalised (lower-case email) by the caller.
pub fn fake_prelogin(server_secret: &[u8; 32], username: &str) -> PreLogin {
    let mac = |label: &[u8]| {
        let mut m = <Hmac<Sha256> as Mac>::new_from_slice(server_secret).expect("any key length");
        m.update(label);
        m.update(username.as_bytes());
        m.finalize().into_bytes()
    };
    let id = mac(b"amanfold/v1/fake-account-id");
    let salt = mac(b"amanfold/v1/fake-salt");
    let mut id16 = [0u8; 16];
    id16.copy_from_slice(&id[..16]);
    let mut s = [0u8; SALT_LEN];
    s.copy_from_slice(&salt[..SALT_LEN]);
    PreLogin {
        account_id: uuid::Builder::from_random_bytes(id16).into_uuid(),
        kdf: KdfParams::DEFAULT,
        salt: s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn register(keys: &ServerKeys, auth: &SecretBytes<32>, id: &Uuid) -> Vec<u8> {
        let (st, req) = client_registration_start(auth).unwrap();
        let resp = server_registration_start(keys, &req, id).unwrap();
        let up = client_registration_finish(st, auth, &resp, id).unwrap();
        server_registration_finish(&up).unwrap()
    }

    fn login(
        keys: &ServerKeys,
        record: Option<&[u8]>,
        auth: &SecretBytes<32>,
        id: &Uuid,
        pin: Option<&[u8]>,
    ) -> Result<(SessionKey, SessionKey)> {
        let (cst, req) = client_login_start(auth)?;
        let (sst, resp) = server_login_start(keys, record, &req, id)?;
        // Server state survives a round trip through storage.
        let sst = ServerLoginState::from_bytes(&sst.to_bytes())?;
        let out = client_login_finish(cst, auth, &resp, id, pin)?;
        let skey = server_login_finish(sst, &out.finalization, id)?;
        Ok((out.session_key, skey))
    }

    #[test]
    fn register_and_login() {
        let keys = ServerKeys::generate();
        let id = crate::secret::random_uuid();
        let auth = SecretBytes::<32>::random();
        let rec = register(&keys, &auth, &id);
        let (c, s) = login(&keys, Some(&rec), &auth, &id, Some(&keys.public_key())).unwrap();
        assert_eq!(c, s);
        // Fresh session key each time.
        let (c2, _) = login(&keys, Some(&rec), &auth, &id, None).unwrap();
        assert_ne!(c, c2);
    }

    #[test]
    fn wrong_key_fails() {
        let keys = ServerKeys::generate();
        let id = crate::secret::random_uuid();
        let rec = register(&keys, &SecretBytes::random(), &id);
        assert_eq!(
            login(&keys, Some(&rec), &SecretBytes::random(), &id, None).unwrap_err(),
            Error::Decrypt
        );
    }

    #[test]
    fn record_is_bound_to_account_and_server() {
        let keys = ServerKeys::generate();
        let id = crate::secret::random_uuid();
        let auth = SecretBytes::<32>::random();
        let rec = register(&keys, &auth, &id);
        // Same record presented under another account id.
        assert!(
            login(
                &keys,
                Some(&rec),
                &auth,
                &crate::secret::random_uuid(),
                None
            )
            .is_err()
        );
        // Impostor server with different keys but the stolen record.
        assert!(login(&ServerKeys::generate(), Some(&rec), &auth, &id, None).is_err());
    }

    #[test]
    fn pinning_detects_key_change() {
        let keys = ServerKeys::generate();
        let id = crate::secret::random_uuid();
        let auth = SecretBytes::<32>::random();
        let rec = register(&keys, &auth, &id);
        let other = ServerKeys::generate().public_key();
        assert!(login(&keys, Some(&rec), &auth, &id, Some(&other)).is_err());
    }

    #[test]
    fn unknown_account_gets_a_response_but_cannot_log_in() {
        let keys = ServerKeys::generate();
        let id = crate::secret::random_uuid();
        let auth = SecretBytes::<32>::random();
        let real = register(&keys, &auth, &crate::secret::random_uuid());
        let (_, req) = client_login_start(&auth).unwrap();
        let (_, fake_resp) = server_login_start(&keys, None, &req, &id).unwrap();
        let (_, real_resp) = server_login_start(&keys, Some(&real), &req, &id).unwrap();
        assert_eq!(fake_resp.len(), real_resp.len());
        assert!(login(&keys, None, &auth, &id, None).is_err());
    }

    #[test]
    fn server_keys_round_trip() {
        let keys = ServerKeys::generate();
        let back = ServerKeys::from_bytes(&keys.to_bytes()).unwrap();
        assert_eq!(keys.public_key(), back.public_key());
    }

    #[test]
    fn fake_prelogin_is_stable_and_distinct() {
        let secret = [9u8; 32];
        let a = fake_prelogin(&secret, "a@example.com");
        assert_eq!(a, fake_prelogin(&secret, "a@example.com"));
        assert_ne!(a, fake_prelogin(&secret, "b@example.com"));
        assert_ne!(a, fake_prelogin(&[8u8; 32], "a@example.com"));
        assert_eq!(a.kdf, KdfParams::DEFAULT);
    }

    #[test]
    fn garbage_messages_are_errors() {
        let keys = ServerKeys::generate();
        let id = crate::secret::random_uuid();
        assert!(server_registration_start(&keys, b"nope", &id).is_err());
        assert!(server_registration_finish(&[0u8; 10]).is_err());
        assert!(server_login_start(&keys, None, &[1u8; 3], &id).is_err());
        assert!(server_login_start(&keys, Some(b"bad"), &[1u8; 3], &id).is_err());
        let (st, _) = client_login_start(&SecretBytes::random()).unwrap();
        assert!(client_login_finish(st, &SecretBytes::random(), &[0u8; 5], &id, None).is_err());
    }

    #[test]
    fn api_token_is_shared_and_distinct_from_session_key() {
        let keys = ServerKeys::generate();
        let id = crate::secret::random_uuid();
        let auth = SecretBytes::<32>::random();
        let rec = register(&keys, &auth, &id);
        let (c, s) = login(&keys, Some(&rec), &auth, &id, None).unwrap();
        assert_eq!(api_token(&c), api_token(&s));
        assert_ne!(api_token(&c).expose()[..], c.expose()[..32]);
        let (c2, _) = login(&keys, Some(&rec), &auth, &id, None).unwrap();
        assert_ne!(api_token(&c), api_token(&c2));
    }

    #[test]
    fn challenge_signature_binds_account_purpose_and_challenge() {
        let k = AccountKeys::generate();
        let id = crate::secret::random_uuid();
        let ch = [7u8; CHALLENGE_LEN];
        let sig = sign_challenge(&k, &id, "recover", &ch).unwrap();
        assert!(verify_challenge(&k.public(), &id, "recover", &ch, &sig).is_ok());
        assert!(verify_challenge(&k.public(), &id, "delete", &ch, &sig).is_err());
        let other = crate::secret::random_uuid();
        assert!(verify_challenge(&k.public(), &other, "recover", &ch, &sig).is_err());
        assert!(
            verify_challenge(&k.public(), &id, "recover", &[8u8; CHALLENGE_LEN], &sig).is_err()
        );
        let stranger = AccountKeys::generate();
        assert!(verify_challenge(&stranger.public(), &id, "recover", &ch, &sig).is_err());
        assert!(verify_challenge(&k.public(), &id, "recover", &ch, &sig[..10]).is_err());
        assert!(sign_challenge(&k, &id, "recover", &[1u8; 5]).is_err());
    }
}
