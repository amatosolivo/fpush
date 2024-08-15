use crate::config::fpush_config::FpushConfig;
use crate::xmpp::error_messages::send_wait_iq_reason_old_prosody;
use crate::xmpp::notification::Notificacion;

use crate::{
    error::{Error, Result},
    xmpp::error_messages::{send_ack_iq, send_error_iq, send_error_policy_iq},
};
use fpush_push::{FpushPushArc, PushRequestError, PushRequestResult};

use futures::{SinkExt, StreamExt};
use log::{error, info, warn};

use tokio::sync::mpsc;
use tokio_xmpp::Component;
// use xmpp::parsers::cert_management::Item;
use xmpp_parsers::{iq::Iq, pubsub::PubSub, Element, Jid};

pub(crate) async fn init_component_connection(config: &FpushConfig) -> Result<Component> {
    let component = Component::new(
        config.component().component_hostname(),
        config.component().component_key(),
        config.component().server_hostname(),
        *config.component().server_port(),
    )
    .await?;

    Ok(component)
}

#[inline(always)]
pub(crate) async fn message_loop_main_thread(
    mut conn: tokio_xmpp::Component,
    push_modules: FpushPushArc,
) {
    // #[cfg(feature = "random_delay_before_push")]
    //let mut rng = rand::thread_rng();

    let (out_sender, mut out_recv) = mpsc::channel::<Iq>(3000);
    loop {
        tokio::select! {
            xmpp_msg = out_recv.recv() => {
                if let Some(msg) = xmpp_msg {
                    if let Err(e) = conn.send(msg.into()).await {
                        error!("Could not reply iq: {}", e);
                    }
                } else {
                    error!("Connection closed");
                    return;
                }
            }
            xmpp_poll = conn.next() => {
                match xmpp_poll {
                    Some(stanza) => {
                        info!("Received XMPP stanza"); 
                        dispatch_xmpp_msg_to_thread(&out_sender, push_modules.clone(), stanza);
                    },
                    None => {
                        error!("The stream was closed, opening new connection");
                        return;
                    }
                }
            },
            else => {
                error!("Main loop error: Closing");
                return;
            }
        };
    }
    // TODO: batch out iq's together send every xxxms out
}

#[inline(always)]
fn dispatch_xmpp_msg_to_thread(
    conn: &mpsc::Sender<Iq>,
    push_modules: FpushPushArc,
    stanza: Element,
) {
    let conn_to_master = conn.clone();
    tokio::spawn(async move {
        handle_iq(&conn_to_master, push_modules, stanza).await;
    });
}

#[inline(always)]
async fn handle_iq(conn: &mpsc::Sender<Iq>, push_modules: FpushPushArc, stanza: Element) {
    // parse message
    let stanza_clone = stanza.clone();

    match Iq::try_from(stanza) {
        Err(e) => {
            warn!("Could not parse stanza: {}", e);
            warn!("\nIQ XML:\n{}\n\n\n", String::from(&stanza_clone));            
        }
        Ok(iq) => {
            let (to, from, iq_payload) = match (iq.to, iq.from, iq.payload) {
                (Some(to), Some(from), xmpp_parsers::iq::IqType::Set(iq_payload)) => {
                    (to, from, iq_payload)
                }
                (Some(to), Some(from), xmpp_parsers::iq::IqType::Get(iq_payload)) => {
                    if iq_payload.name() == "ping" {
                        info!("Received ping from {}", from);
                        send_ack_iq(conn, &iq.id, from, to).await;
                    } else {
                        send_error_iq(conn, &iq.id, from, to).await;
                    }
                    return;
                }
                (Some(to), Some(from), _) => {
                    info!("Received unhandled iq from {}", from);
                    send_error_iq(conn, &iq.id, from, to).await;
                    return;
                }
                (_, None, _) => {
                    warn!("Received iq without from");
                    return;
                }
                (_, _, _) => {
                    return;
                }
            };
            let (module_id, notif_json) = match parse_token_and_module_id(iq_payload) {
                Ok((module_id, notif_json)) => (module_id, notif_json),
                Err(e) => {
                    warn!(
                        "Could not retrieve token or module_id: {} source: {}",
                        e, from
                    );
                    send_wait_iq_reason_old_prosody(conn, &iq.id, from, to).await;
                    return;
                }
            };
            warn!("\n TC3 Message: {} \n", String::from(&stanza_clone));
            
            let push_result = push_modules.push(&module_id, notif_json.clone()).await;
            handle_push_result(conn, &module_id, &notif_json, &push_result, from, to, iq.id).await
        }
    }
}

async fn handle_push_result(
    conn: &mpsc::Sender<Iq>,
    module_id: &str,
    token: &str,
    push_result: &PushRequestResult<()>,
    from: Jid,
    to: Jid,
    iq_id: String,
) {
    match push_result {
        Ok(()) => {
            info!(
                "{}: Successfully sent push notification for token {} from {}",
                module_id, token, from
            );
            send_ack_iq(conn, &iq_id, from, to).await
        },
        Err(PushRequestError::TokenRatelimited) => {
            warn!(
                "{}: Push request rate-limited for token {} from {}",
                module_id, token, from
            );
            // Some admins did not understood the wait_iq -> we now send an ack
            send_ack_iq(conn, &iq_id, from, to).await
        }
        Err(PushRequestError::TokenBlocked) => {
            warn!(
                "{}: Received push request from blocked token {} from {}",
                module_id, token, from
            );
            send_error_policy_iq(conn, &iq_id, from, to).await;
        }
        Err(PushRequestError::Internal) => {
            warn!(
                "{}: Incountered internal push error for token {} from {}",
                module_id, token, from
            );
            send_error_iq(conn, &iq_id, from, to).await;
        }
        Err(PushRequestError::UnknownPushModule) => {
            warn!(
                "{}: Unknown push module requested for token {} from {}",
                module_id, token, from
            );
            send_error_iq(conn, &iq_id, from, to).await;
        }
    }
}

#[inline(always)]
fn parse_token_and_module_id(iq_payload: Element) -> Result<(String, String)> {
    match PubSub::try_from(iq_payload.clone()) {
        Ok(pubsub) => {
            match Notificacion::from_pubsub(&pubsub) {
                Ok(notif) => {
                    let module_id = notif.get_moduleid()
                    .cloned()
                    .unwrap_or_else(|| "default".to_string());
                    Ok((module_id, notif.to_string()))
                },
                Err(e) => {
                    error!("Failed to create Notificacion from PubSub: {:?}", e);
                    Err(Error::InvalidNotificationFormat)
                }
            }
        },
        Err(e) => {
            error!("Failed to parse PubSub from payload: {:?}", e);
            error!("Payload content: {}", String::from(&iq_payload));
            Err(Error::PubSubInvalidFormat)
        }
    }
}
