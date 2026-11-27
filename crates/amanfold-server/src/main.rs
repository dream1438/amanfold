//! `amanfold-server` command line.
//!
//! ```text
//! amanfold-server keygen > secrets.json      # once per deployment
//! amanfold-server serve                       # reads settings from the environment
//! ```
//!
//! Environment for `serve` (see docs/SERVER.md):
//!
//! | Variable | Default | |
//! | --- | --- | --- |
//! | `AMANFOLD_LISTEN` | `127.0.0.1:8080` | Address to listen on (put TLS in front). |
//! | `AMANFOLD_DB` | `amanfold.db` | SQLite database file. |
//! | `AMANFOLD_SECRETS_FILE` | (required) | Output of `keygen`. |
//! | `AMANFOLD_SMTP_URL` | | `smtps://user:pass@host` for real email. |
//! | `AMANFOLD_MAIL_FROM` | | Sender, e.g. `Amanfold <no-reply@amanfold.com>`. |
//! | `AMANFOLD_TRUST_PROXY` | `0` | `1` behind a reverse proxy that sets `X-Forwarded-For`. |
//! | `AMANFOLD_DEV` | `0` | `1` prints email codes to the log instead of sending. |
//! | `AMANFOLD_NO_EMAIL_VERIFICATION` | `0` | `1` for a private self-hosted server. |

use std::{net::SocketAddr, sync::Arc};

use amanfold_server::{
    AppState, Config, Secrets, app,
    mail::{LogMailer, Mailer, SmtpMailer},
    store, wire,
};

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

fn flag(k: &str) -> bool {
    env(k).is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("amanfold-server: {msg}");
    std::process::exit(1)
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("keygen") => {
            let s = Secrets::generate();
            println!("{}", s.to_json().as_str());
            eprintln!(
                "OPAQUE public key (pin this in the apps): {}",
                wire::b64(&s.opaque.public_key())
            );
        }
        Some("serve") => serve(),
        _ => {
            eprintln!("usage: amanfold-server keygen | serve");
            std::process::exit(2);
        }
    }
}

fn serve() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=info".into()),
        )
        .init();

    let path =
        env("AMANFOLD_SECRETS_FILE").unwrap_or_else(|| fail("AMANFOLD_SECRETS_FILE is not set"));
    let raw = zeroize::Zeroizing::new(
        std::fs::read_to_string(&path).unwrap_or_else(|e| fail(format!("{path}: {e}"))),
    );
    let secrets = Secrets::from_json(&raw).unwrap_or_else(|e| fail(e));

    let cfg = Config {
        require_email_verification: !flag("AMANFOLD_NO_EMAIL_VERIFICATION"),
        trust_proxy: flag("AMANFOLD_TRUST_PROXY"),
        ..Config::default()
    };

    let mailer: Arc<dyn Mailer> = match (env("AMANFOLD_SMTP_URL"), env("AMANFOLD_MAIL_FROM")) {
        (Some(url), Some(from)) => {
            Arc::new(SmtpMailer::new(&url, &from).unwrap_or_else(|e| fail(e)))
        }
        _ if flag("AMANFOLD_DEV") || !cfg.require_email_verification => Arc::new(LogMailer),
        _ => fail(
            "set AMANFOLD_SMTP_URL and AMANFOLD_MAIL_FROM, or AMANFOLD_DEV=1 for local development",
        ),
    };

    let db_path = env("AMANFOLD_DB").unwrap_or_else(|| "amanfold.db".into());
    let db = store::open(&db_path).unwrap_or_else(|e| fail(format!("database {db_path}: {e}")));
    let addr: SocketAddr = env("AMANFOLD_LISTEN")
        .unwrap_or_else(|| "127.0.0.1:8080".into())
        .parse()
        .unwrap_or_else(|e| fail(format!("AMANFOLD_LISTEN: {e}")));

    let state = AppState::new(cfg, secrets, db, mailer);
    let rt = tokio::runtime::Runtime::new().unwrap_or_else(|e| fail(e));
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .unwrap_or_else(|e| fail(format!("listen {addr}: {e}")));
        tracing::info!(%addr, "amanfold-server listening");
        axum::serve(
            listener,
            app(state).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .unwrap_or_else(|e| fail(e));
    });
}
