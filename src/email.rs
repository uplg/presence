use anyhow::{Context, Result};
use lettre::message::header::ContentType;
use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

pub async fn send(
    from_address: &str,
    from_name: &str,
    app_password: &str,
    recipients: &[&str],
    subject: &str,
    body: &str,
) -> Result<()> {
    let mailbox: Mailbox = format!("{from_name} <{from_address}>")
        .parse()
        .context("invalid from address")?;

    let mut builder = Message::builder()
        .from(mailbox.clone())
        .sender(mailbox)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN);

    for addr in recipients {
        builder = builder.to(addr.parse().context("invalid recipient address")?);
    }

    let email = builder.body(body.to_string()).context("failed to build email")?;

    let creds = Credentials::new(from_address.to_string(), app_password.to_string());

    let mailer = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay("smtp.gmail.com")
        .context("failed to create SMTP transport")?
        .credentials(creds)
        .build();

    mailer
        .send(email)
        .await
        .context("failed to send email")?;

    Ok(())
}
