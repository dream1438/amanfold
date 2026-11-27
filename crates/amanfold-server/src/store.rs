//! SQLite storage. Everything the server keeps is listed in docs/SERVER.md;
//! none of it is vault plaintext.

use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS accounts (
    id            BLOB PRIMARY KEY,      -- account id (uuid bytes)
    email         TEXT NOT NULL UNIQUE,  -- normalised
    bundle        BLOB NOT NULL,         -- AccountBundle (CBOR): public keys + wrapped private keys
    public_keys   BLOB NOT NULL,         -- PublicKeys (CBOR), copied from the bundle for lookups
    opaque_record BLOB NOT NULL,         -- OPAQUE registration record
    created_at    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
    id          BLOB PRIMARY KEY,
    account_id  BLOB NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    token_hash  BLOB NOT NULL UNIQUE,    -- SHA-256 of the bearer token
    device_name TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    last_used   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS sessions_account ON sessions(account_id);
CREATE TABLE IF NOT EXISTS email_codes (
    email      TEXT NOT NULL,
    purpose    TEXT NOT NULL,
    code_hash  BLOB NOT NULL,
    expires_at INTEGER NOT NULL,
    attempts   INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (email, purpose)
);
CREATE TABLE IF NOT EXISTS vaults (
    id             BLOB PRIMARY KEY,
    owner_id       BLOB NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    epoch          INTEGER NOT NULL,
    seq            INTEGER NOT NULL DEFAULT 0,  -- sequence number of the stored manifest (0 = none)
    manifest       BLOB,                        -- latest SignedManifest (CBOR)
    bytes          INTEGER NOT NULL DEFAULT 0,  -- stored record bytes (quota)
    needs_rotation INTEGER NOT NULL DEFAULT 0,  -- a member was removed since the last rotation
    created_at     INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS records (
    vault_id BLOB NOT NULL REFERENCES vaults(id) ON DELETE CASCADE,
    item_id  BLOB NOT NULL,
    seq      INTEGER NOT NULL,           -- manifest seq that last wrote it
    digest   BLOB NOT NULL,              -- manifest::record_digest
    record   BLOB NOT NULL,              -- EncryptedRecord (CBOR)
    PRIMARY KEY (vault_id, item_id)
);
CREATE INDEX IF NOT EXISTS records_seq ON records(vault_id, seq);
CREATE TABLE IF NOT EXISTS members (
    vault_id   BLOB NOT NULL REFERENCES vaults(id) ON DELETE CASCADE,
    account_id BLOB NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    role       TEXT NOT NULL,            -- owner | manager | writer | reader
    active     INTEGER NOT NULL,         -- 0 until a grant for the current epoch is uploaded
    added_at   INTEGER NOT NULL,
    PRIMARY KEY (vault_id, account_id)
);
CREATE INDEX IF NOT EXISTS members_account ON members(account_id);
CREATE TABLE IF NOT EXISTS grants (
    vault_id   BLOB NOT NULL REFERENCES vaults(id) ON DELETE CASCADE,
    account_id BLOB NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    epoch      INTEGER NOT NULL,
    body       BLOB NOT NULL,            -- VaultGrant (CBOR), signed by the granter
    PRIMARY KEY (vault_id, account_id, epoch)
);
CREATE TABLE IF NOT EXISTS invites (
    id         BLOB PRIMARY KEY,
    vault_id   BLOB NOT NULL REFERENCES vaults(id) ON DELETE CASCADE,
    email      TEXT NOT NULL,
    role       TEXT NOT NULL,
    invited_by BLOB NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL,
    UNIQUE (vault_id, email)
);
CREATE INDEX IF NOT EXISTS invites_email ON invites(email);
"#;

/// Open (or create) the database at `path` (`:memory:` for tests).
pub fn open(path: &str) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "FULL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "secure_delete", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

/// Vault role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Created the vault; can share, remove members, rotate and delete.
    Owner,
    /// Can share with and remove writers and readers, and rotate.
    Manager,
    /// Can read and write items.
    Writer,
    /// Can read items. Enforced by the server only (see SECURITY.md O2).
    Reader,
}

impl Role {
    /// Text form.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Manager => "manager",
            Role::Writer => "writer",
            Role::Reader => "reader",
        }
    }
    /// Parse the text form.
    pub fn parse(s: &str) -> ApiResult<Self> {
        Ok(match s {
            "owner" => Role::Owner,
            "manager" => Role::Manager,
            "writer" => Role::Writer,
            "reader" => Role::Reader,
            _ => return Err(ApiError::Internal(format!("bad role {s}"))),
        })
    }
    /// May upload records and manifests.
    pub fn can_write(self) -> bool {
        !matches!(self, Role::Reader)
    }
    /// May invite, grant, remove and rotate.
    pub fn can_manage(self) -> bool {
        matches!(self, Role::Owner | Role::Manager)
    }
}

/// A vault row.
#[derive(Debug, Clone)]
pub struct VaultRow {
    /// Owner account.
    pub owner_id: Uuid,
    /// Current key epoch.
    pub epoch: u32,
    /// Current manifest sequence number (0 = none yet).
    pub seq: u64,
    /// Latest manifest.
    pub manifest: Option<Vec<u8>>,
    /// Stored record bytes.
    pub bytes: u64,
    /// A member was removed and the key has not been rotated since.
    pub needs_rotation: bool,
}

/// Load a vault.
pub fn vault(conn: &Connection, id: &Uuid) -> ApiResult<Option<VaultRow>> {
    Ok(conn
        .query_row(
            "SELECT owner_id, epoch, seq, manifest, bytes, needs_rotation FROM vaults WHERE id = ?1",
            params![id.as_bytes()],
            |r| {
                Ok(VaultRow {
                    owner_id: uuid_col(r.get::<_, Vec<u8>>(0)?),
                    epoch: r.get::<_, i64>(1)? as u32,
                    seq: r.get::<_, i64>(2)? as u64,
                    manifest: r.get(3)?,
                    bytes: r.get::<_, i64>(4)? as u64,
                    needs_rotation: r.get::<_, i64>(5)? != 0,
                })
            },
        )
        .optional()?)
}

/// A member's role and whether they hold a current grant.
pub fn membership(
    conn: &Connection,
    vault: &Uuid,
    account: &Uuid,
) -> ApiResult<Option<(Role, bool)>> {
    let row: Option<(String, i64)> = conn
        .query_row(
            "SELECT role, active FROM members WHERE vault_id = ?1 AND account_id = ?2",
            params![vault.as_bytes(), account.as_bytes()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    row.map(|(r, a)| Ok((Role::parse(&r)?, a != 0))).transpose()
}

/// An account's public keys (CBOR) and email.
pub fn account_keys(conn: &Connection, id: &Uuid) -> ApiResult<Option<(Vec<u8>, String)>> {
    Ok(conn
        .query_row(
            "SELECT public_keys, email FROM accounts WHERE id = ?1",
            params![id.as_bytes()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

/// Bytes stored in a uuid column.
pub fn uuid_col(b: Vec<u8>) -> Uuid {
    Uuid::from_slice(&b).unwrap_or_default()
}
