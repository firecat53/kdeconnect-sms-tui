use tokio::sync::mpsc;
use tracing::{debug, warn};
use zbus::message::Body;
use zbus::zvariant::OwnedValue;
use zbus::Connection;

use crate::dbus::conversations::parse_signal_message;
use crate::events::AppEvent;

const KDECONNECT_SERVICE: &str = "org.kde.kdeconnect";
const CONVERSATIONS_INTERFACE: &str = "org.kde.kdeconnect.device.conversations";

/// Spawn a task that listens for conversation D-Bus signals and forwards them as AppEvents.
/// The D-Bus match rule is registered **before** returning so that no signals are lost
/// between spawning the listener and starting to load conversations.
/// Returns a JoinHandle that can be aborted to stop the listener.
pub async fn spawn_signal_listener(
    connection: Connection,
    device_id: String,
    tx: mpsc::UnboundedSender<AppEvent>,
) -> color_eyre::Result<tokio::task::JoinHandle<()>> {
    let device_path = format!("/modules/kdeconnect/devices/{}", device_id);

    // Subscribe to all signals on the conversations interface for this device.
    // This MUST complete before any conversation requests are made, otherwise
    // the reply signals will be missed (race condition).
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(KDECONNECT_SERVICE)?
        .path(device_path.as_str())?
        .interface(CONVERSATIONS_INTERFACE)?
        .build();

    let stream = zbus::MessageStream::for_match_rule(rule, &connection, None).await?;

    debug!("Listening for conversation signals on {}", device_path);

    Ok(tokio::spawn(async move {
        if let Err(e) = listen_signals(stream, tx).await {
            warn!("Signal listener stopped: {}", e);
        }
    }))
}

async fn listen_signals(
    mut stream: zbus::MessageStream,
    tx: mpsc::UnboundedSender<AppEvent>,
) -> color_eyre::Result<()> {
    use futures_lite::StreamExt;

    while let Some(msg) = stream.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                warn!("Error receiving signal: {}", e);
                continue;
            }
        };

        let header = msg.header();
        let member = header.member().map(|m| m.as_str().to_owned());

        match member.as_deref() {
            Some("conversationCreated") => {
                if let Some(event) = parse_variant_signal_body(msg.body()) {
                    debug!("Signal: conversationCreated thread={}", event.thread_id);
                    if tx.send(AppEvent::ConversationCreated(event)).is_err() {
                        break;
                    }
                }
            }
            Some("conversationUpdated") => {
                if let Some(event) = parse_variant_signal_body(msg.body()) {
                    debug!("Signal: conversationUpdated thread={}", event.thread_id);
                    if tx.send(AppEvent::ConversationUpdated(event)).is_err() {
                        break;
                    }
                }
            }
            Some("conversationRemoved") => {
                if let Ok(thread_id) = msg.body().deserialize::<i64>() {
                    debug!("Signal: conversationRemoved thread={}", thread_id);
                    if tx.send(AppEvent::ConversationRemoved(thread_id)).is_err() {
                        break;
                    }
                }
            }
            Some("conversationLoaded") => {
                // Signal carries (conversationID: i64, messageCount: u64)
                let (thread_id, message_count) =
                    msg.body().deserialize::<(i64, u64)>().unwrap_or((0, 0));
                debug!(
                    "Signal: conversationLoaded thread={} count={}",
                    thread_id, message_count
                );
                let _ = tx.send(AppEvent::ConversationLoaded(thread_id, message_count));
            }
            Some("attachmentReceived") => {
                if let Ok((file_path, file_name)) = msg.body().deserialize::<(String, String)>() {
                    debug!(
                        "Signal: attachmentReceived path={} name={}",
                        file_path, file_name
                    );
                    if tx
                        .send(AppEvent::AttachmentReceived(file_path, file_name))
                        .is_err()
                    {
                        break;
                    }
                }
            }
            Some(other) => {
                debug!("Ignoring signal: {}", other);
            }
            None => {}
        }
    }

    Ok(())
}

/// Parse a signal body that contains a QDBusVariant wrapping a message map.
fn parse_variant_signal_body(body: Body) -> Option<crate::models::message::Message> {
    // The signal sends a QDBusVariant (wrapped variant)
    let val: OwnedValue = match body.deserialize() {
        Ok(v) => v,
        Err(e) => {
            warn!("Failed to deserialize signal body: {}", e);
            return None;
        }
    };
    let msg = parse_signal_message(&val);
    if msg.is_none() {
        warn!("Failed to parse signal message from variant");
    }
    msg
}
