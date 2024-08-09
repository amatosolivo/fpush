use std::time::SystemTime;
use std::collections::HashMap;

use a2::{
    Client, DefaultNotificationBuilder, NotificationBuilder, NotificationOptions, Priority,
    PushType,
};
use fpush_traits::push::{PushError, PushResult, PushTrait};

use async_trait::async_trait;
use log::{debug, error, info, warn};
use serde_json::Value;

use crate::AppleApnsConfig;
pub struct FpushApns {
    apns: a2::client::Client,
    topic: String,
    additional_data: Option<HashMap<String, Value>>,
}

impl FpushApns {
    fn open_cert(filename: &str) -> PushResult<std::fs::File> {
        if let Ok(file) = std::fs::File::open(filename) {
            Ok(file)
        } else {
            Err(PushError::CertLoading)
        }
    }

    pub fn init(apns_config: &AppleApnsConfig) -> PushResult<Self> {
        let mut certificate = FpushApns::open_cert(apns_config.cert_file_path())?;
        match Client::certificate(
            &mut certificate,
            apns_config.cert_password(),
            apns_config.endpoint(),
        ) {
            Ok(apns_conn) => {
                let wrapped_conn = Self {
                    apns: apns_conn,
                    topic: apns_config.topic().to_string(),
                    additional_data: apns_config.additional_data().clone(),
                };
                Ok(wrapped_conn)
            }
            Err(a2::error::Error::ReadError(_)) => Err(PushError::PushEndpointPersistent),
            Err(e) => {
                error!("Problem initializing apple config: {}", e);
                Err(PushError::PushEndpointTmp)
            },
        }
    }
}

#[async_trait]
impl PushTrait for FpushApns {
    #[inline(always)]
    async fn send(&self, notif_json: String) -> PushResult<()> {

        let notif: Value = match serde_json::from_str(&notif_json) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Error deserializing notification JSON: {}", e);
                return  Err(PushError::Unknown(2));
            }
        };
        
        let token = notif["token"].as_str().ok_or(PushError::Unknown(1))?;
        let message_count = notif["message_count"].as_u64().unwrap_or(1);
        let last_message_sender = notif["last_message_sender"].as_str().unwrap_or("Someone");
        let last_message_body = notif["last_message_body"].as_str().unwrap_or("New message");
    
        let title = format!("{} new message(s)", message_count);
        let body = format!("{}: {}", last_message_sender, last_message_body);
    
        let mut notification_builder = DefaultNotificationBuilder::new()
            .set_title(&title)
            .set_body(&body)
            .set_mutable_content()
            .set_sound("default");

        let mut payload = notification_builder.build(
            token,
            NotificationOptions {
                apns_priority: Some(Priority::High),
                apns_topic: Some(&self.topic),
                apns_expiration: Some(
                    SystemTime::now().elapsed().unwrap().as_secs() + 4 * 7 * 24 * 3600,
                ),
                apns_push_type: PushType::Alert,
                ..Default::default()
            },
        );
        match &self.additional_data {
            None => {}
            Some(additional_data) => {
                for (key, value) in additional_data {
                    payload.add_custom_data(key, value).unwrap();
                }
            }
        }
       info!(
            "Payload send to apple: {}",
            payload.clone().to_json_string().unwrap()
        );
        match self.apns.send(payload).await {
            Ok(response) => {
                info!("Got response {} from apple for token {}",
                    response.code, token
                );
                response_code_to_push_error(response.code)
            }
            Err(e) => {
                error!("Could not send apns message to apple: {}", e);
                if let a2::Error::ResponseError(response) = e {
                    return response_code_to_push_error(response.code);
                }
                Err(PushError::PushEndpointTmp)
            }
        }
    }
}

fn response_code_to_push_error(response_code: u16) -> PushResult<()> {
    match response_code {
        200 => {
            info!("Successfully sent APNS notification");
            Ok(())
        },
        400 => {
            warn!("Bad request error from APNS");
            Err(PushError::PushEndpointPersistent)
        },
        403 => {
            warn!("Authentication error with APNS");
            Err(PushError::PushEndpointPersistent)
        },
        405 => {
            warn!("Method not allowed error from APNS");
            Err(PushError::PushEndpointPersistent)
        },
        410 => {
            warn!("Token is no longer valid");
            Err(PushError::TokenBlocked)
        },
        429 => {
            warn!("Too many requests to APNS");
            Err(PushError::TokenRateLimited)
        },
        500 => {
            warn!("Internal server error from APNS");
            Err(PushError::PushEndpointTmp)
        },
        503 => {
            warn!("APNS service is unavailable");
            Err(PushError::PushEndpointTmp)
        },
        ecode => {
            error!("Received unhandled error code from apple apns: {}", ecode);
            Err(PushError::Unknown(ecode))
        }
    }
}
