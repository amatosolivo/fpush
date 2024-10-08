use std::{collections::HashMap, path::Path};

use fpush_traits::push::{PushError, PushResult, PushTrait};

use async_trait::async_trait;
use google_fcm1::{
    api::{Message, SendMessageRequest},
    oauth2, FirebaseCloudMessaging,
};
use log::{debug, error, info, warn};
use serde::Deserialize;
use serde_json::Value;

use crate::config::GoogleFcmConfig;
pub struct FpushFcm {
    fcm_conn:
        FirebaseCloudMessaging<hyper_rustls::HttpsConnector<hyper::client::connect::HttpConnector>>,
    fcm_parent: String,
}

impl FpushFcm {
    async fn load_oauth2_app_secret(fcm_config: &GoogleFcmConfig) -> oauth2::ServiceAccountKey {
        match oauth2::read_service_account_key(Path::new(fcm_config.fcm_secret_path())).await {
            Ok(s) => s,
            Err(e) => panic!(
                "Could not read fcm config file at {} reason: {}",
                fcm_config.fcm_secret_path(),
                e
            ),
        }
    }

    pub async fn init(fcm_config: &GoogleFcmConfig) -> PushResult<Self> {
        let fcm_secret = Self::load_oauth2_app_secret(fcm_config).await;

        // create login auth object
        let auth = match oauth2::ServiceAccountAuthenticator::builder(fcm_secret.clone())
            .build()
            .await
        {
            Ok(auth) => auth,
            Err(e) => {
                error!("Could not load fcm DeviceFlowAuthenticator: {}", e);
                return Err(PushError::CertLoading);
            }
        };

        let hyper_client = hyper::Client::builder().build(
            hyper_rustls::HttpsConnectorBuilder::new()
                .with_native_roots()
                .https_only()
                .enable_http2()
                .build(),
        );
        let fcm_conn = FirebaseCloudMessaging::new(hyper_client, auth);
        Ok(Self {
            fcm_conn,
            fcm_parent: format!("projects/{}", fcm_secret.project_id.unwrap()),
        })
    }
}

#[derive(Debug, Deserialize)]
struct FcmError {
    error_code: FcmErrorCode,
}

impl FcmError {
    pub fn error_code(&self) -> &FcmErrorCode {
        &self.error_code
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum FcmErrorCode {
    UnspecifiedError,
    InvalidArgument,
    Unregistered,
    SenderIdMismatch,
    QuotaExceeded,
    Unavailable,
    Internal,
    ThirdPartyAuthError,
}

#[async_trait]
impl PushTrait for FpushFcm {
    #[inline(always)]
    async fn send(&self, notif_json: String) -> PushResult<()> {
        let req = SendMessageRequest {
            message: Some(create_push_message(notif_json)),
            validate_only: None,
        };

        let fcm_result = self
            .fcm_conn
            .projects()
            .messages_send(req, &self.fcm_parent)
            .doit()
            .await;
        match fcm_result {
            Err(e) => {
                warn!("FCM returned {}", e);
                if let google_fcm1::client::Error::BadRequest(error_body) = e {
                    let parsed_error_body: FcmError = serde_json::from_value(error_body).unwrap();
                    match parsed_error_body.error_code() {
                        FcmErrorCode::Unregistered => Err(PushError::TokenBlocked),
                        FcmErrorCode::QuotaExceeded => Err(PushError::TokenRateLimited),
                        FcmErrorCode::Unavailable => Err(PushError::PushEndpointTmp),
                        FcmErrorCode::Internal => Err(PushError::PushEndpointTmp),
                        FcmErrorCode::SenderIdMismatch => Err(PushError::TokenBlocked),
                        _ => Err(PushError::Unknown(u16::MAX)),
                    }
                } else {
                    Err(PushError::PushEndpointTmp)
                }
            }
            Ok(_) => {
                info!("FCM: Successfully sent push notification ");
                Ok(())
            }
        }
    }
}

#[inline(always)]
fn create_push_message(notif_json: String) -> Message {
    
    let notif: Value = match serde_json::from_str(&notif_json) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error deserializing notification JSON: {}", e);
            return Message::default();
        }
    };

    let mut data = HashMap::new();
    let mut token = None;

    if let Some(obj) = notif.as_object() {
        for (key, value) in obj {
            if key == "token" {
                if let Some(t) = value.as_str() {
                    token = Some(t.to_string());
                }
            } else {
                let value_str = match value {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    _ => value.to_string(),
                };
                data.insert(key.clone(), value_str);
            }
        }
    }

    Message {
        data: Some(data),
        token: token,
        ..Default::default()
    }
}
