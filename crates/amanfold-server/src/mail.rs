//! Outgoing email: verification codes, recovery codes and invitations.
//!
//! Mail never contains vault data. Sending happens in the background so the
//! response time does not reveal whether an address has an account.

use std::sync::Mutex;

use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor, message::Mailbox};

/// One email to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mail {
    /// Code to finish signing up.
    SignupCode {
        /// The code.
        code: String,
    },
    /// Someone tried to sign up with an address that already has an account.
    AlreadyRegistered,
    /// Code to start account recovery.
    RecoveryCode {
        /// The code.
        code: String,
    },
    /// An invitation to a shared vault.
    Invitation {
        /// Who invited (their account email).
        from: String,
    },
    /// Confirm a beta waitlist sign-up from the website.
    WaitlistConfirm {
        /// Link that confirms the sign-up.
        confirm: String,
        /// Link that removes the address from the list.
        leave: String,
        /// Write the email in Arabic.
        arabic: bool,
    },
}

impl Mail {
    fn render(&self) -> (String, String) {
        match self {
            Mail::SignupCode { code } => (
                "Your Amanfold sign-up code".into(),
                format!(
                    "Your Amanfold sign-up code is {code}\n\nIt expires in 15 minutes. \
                     If you did not ask for it, ignore this email."
                ),
            ),
            Mail::AlreadyRegistered => (
                "Amanfold sign-up attempt".into(),
                "Someone tried to create an Amanfold account with this email address, \
                 but it already has one. If it was you, sign in instead. If you have \
                 forgotten your master password, use your recovery kit."
                    .into(),
            ),
            Mail::RecoveryCode { code } => (
                "Your Amanfold recovery code".into(),
                format!(
                    "Your Amanfold account recovery code is {code}\n\nIt expires in 15 minutes. \
                     You will also need the 24 recovery words and the Secret Key from your \
                     recovery kit. If you did not ask for this, ignore this email: nobody can \
                     recover your account without the kit."
                ),
            ),
            Mail::Invitation { from } => (
                "You were invited to a shared Amanfold vault".into(),
                format!(
                    "{from} invited you to a shared vault on Amanfold. Open the Amanfold app \
                     to accept. Before the vault opens, compare safety numbers with {from} \
                     in person or on a call."
                ),
            ),
            Mail::WaitlistConfirm {
                confirm,
                leave,
                arabic: false,
            } => (
                "Confirm your place in the Amanfold beta".into(),
                format!(
                    "Thanks for your interest in Amanfold.\n\n\
                     To join the beta waitlist, open this link:\n{confirm}\n\n\
                     If you did not ask for this, ignore this email and we will forget \
                     your address within 7 days.\n\n\
                     We will only email you about the beta. To leave the list at any \
                     time:\n{leave}"
                ),
            ),
            Mail::WaitlistConfirm {
                confirm,
                leave,
                arabic: true,
            } => (
                "أكّد انضمامك إلى النسخة التجريبية من أمان فولد".into(),
                format!(
                    "شكرًا لاهتمامك بأمان فولد.\n\n\
                     للانضمام إلى قائمة انتظار النسخة التجريبية، افتح هذا الرابط:\n{confirm}\n\n\
                     إن لم تطلب ذلك، تجاهل هذه الرسالة وسنحذف عنوانك خلال 7 أيام.\n\n\
                     لن نراسلك إلا بشأن النسخة التجريبية. لمغادرة القائمة في أي وقت:\n{leave}"
                ),
            ),
        }
    }
}

/// Where mail goes.
pub trait Mailer: Send + Sync {
    /// Queue `mail` for `to`. Must not block.
    fn send(&self, to: &str, mail: Mail);
}

/// Development only: writes mail (including codes) to the log.
pub struct LogMailer;

impl Mailer for LogMailer {
    fn send(&self, to: &str, mail: Mail) {
        let (subject, body) = mail.render();
        tracing::warn!(%to, %subject, %body, "DEV MAILER: not sent");
    }
}

/// Keeps mail in memory (tests).
#[derive(Default)]
pub struct MemoryMailer {
    sent: Mutex<Vec<(String, Mail)>>,
}

impl MemoryMailer {
    /// Everything sent so far.
    pub fn sent(&self) -> Vec<(String, Mail)> {
        self.sent.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    /// The last code mailed to `to`, if any.
    pub fn last_code(&self, to: &str) -> Option<String> {
        self.sent().into_iter().rev().find_map(|(t, m)| match m {
            Mail::SignupCode { code } | Mail::RecoveryCode { code } if t == to => Some(code),
            _ => None,
        })
    }
}

impl Mailer for MemoryMailer {
    fn send(&self, to: &str, mail: Mail) {
        self.sent
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((to.to_string(), mail));
    }
}

/// SMTP delivery (any provider), TLS required.
pub struct SmtpMailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl SmtpMailer {
    /// `url` like `smtps://user:password@smtp.example.com` (implicit TLS) or
    /// `smtp://user:password@smtp.example.com?tls=required` (STARTTLS).
    pub fn new(url: &str, from: &str) -> Result<Self, String> {
        let transport = AsyncSmtpTransport::<Tokio1Executor>::from_url(url)
            .map_err(|e| format!("smtp url: {e}"))?
            .build();
        let from = from.parse().map_err(|e| format!("mail from: {e}"))?;
        Ok(Self { transport, from })
    }
}

impl Mailer for SmtpMailer {
    fn send(&self, to: &str, mail: Mail) {
        let (subject, body) = mail.render();
        let to_box: Mailbox = match to.parse() {
            Ok(m) => m,
            Err(_) => return,
        };
        let msg = match Message::builder()
            .from(self.from.clone())
            .to(to_box)
            .subject(subject)
            .body(body)
        {
            Ok(m) => m,
            Err(e) => {
                tracing::error!(error = %e, "could not build email");
                return;
            }
        };
        let t = self.transport.clone();
        tokio::spawn(async move {
            if let Err(e) = t.send(msg).await {
                tracing::error!(error = %e, "email delivery failed");
            }
        });
    }
}
