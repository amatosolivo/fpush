use serde::{Deserialize, Serialize};

use std::str::FromStr;
use futures::StreamExt;

use crate::error::Error;

use xmpp_parsers::data_forms::DataForm;
use xmpp_parsers::pubsub::PubSub;


#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notificacion {
    message_count: u32,
    last_message_sender: String,
    last_message_body: String,
    additional_data: Option<String>,
    token: String,
    propousedid: Option<String>,
    media: Option<String>,    
    ntype: Option<String>,
    moduleid: Option<String>,
}

impl Notificacion {
    pub fn new(
        message_count: u32,
        last_message_sender: String,
        last_message_body: String,
        additional_data: Option<String>,
        token: String,
        propousedid: Option<String>,
        media: Option<String>,    
        ntype: Option<String>,    
        moduleid: Option<String>,
    ) -> Self {
        Notificacion {
            message_count,
            last_message_sender,
            last_message_body,
            additional_data,
            token,
            propousedid,
            media,
            ntype,
            moduleid,
        }
    }

    pub fn from_pubsub(pubsub: &PubSub) -> Result<Self, Error> {
        match pubsub {
            PubSub::Publish { publish, publish_options } => {
                let token = publish.node.0.clone();

                
                if let Some(item) = publish.items.first() {
                    if let Some(notification_elem) = item.payload.as_ref() {
                        if let Some(data_form) = notification_elem.get_child("x", "jabber:x:data") {
                            let data_form = DataForm::try_from(data_form.clone())
                            .map_err(|_| Error::InvalidNotificationFormat)?;
                            
                            let mut message_count = 0;
                            let mut last_message_sender = String::new();
                            let mut last_message_body = String::new();                            
                            
                            for field in data_form.fields {
                                match field.var.as_str() {
                                    "message-count" => {
                                        message_count = field.values.first().and_then(|v| v.parse().ok()).unwrap_or(0);
                                    },
                                    "last-message-sender" => {
                                        last_message_sender = field.values.first().cloned().unwrap_or_default();
                                    },
                                    "last-message-body" => {
                                        last_message_body = field.values.first().cloned().unwrap_or_default();
                                    },
                                    _ => {}
                                }
                            }
                            
                            let additional_data = notification_elem
                                .get_child("additional", "http://example.com/custom")//TODO: verificar que namespace va aqui 
                                .map(|elem| elem.text());
                            
                            let extract_field = |field_name: &str| -> Option<String> {
                                publish_options
                                    .as_ref()
                                    .and_then(|options| options.form.as_ref())
                                    .and_then(|form| {
                                        form.fields.iter()
                                            .find(|field| field.var == field_name)
                                            .and_then(|field| field.values.first().cloned())
                                    })
                                    .filter(|value| !value.is_empty())
                            };
                    
                            let module_id = extract_field("pushModule");
                            let notification_type = extract_field("notification-type");
                            let propose_id = extract_field("propose-id");
                            let propose_media = extract_field("propose-media");
                            
                            if notification_type.is_none() || module_id.is_none() {
                                return Err(Error::InvalidNotificationFormat);
                            }

                            if notification_type.as_deref() == Some("call") {
                                if propose_id.is_none() || propose_media.is_none() {
                                    return Err(Error::InvalidNotificationFormat);
                                }
                            }

                            return Ok(Notificacion::new(
                                message_count,
                                last_message_sender,
                                last_message_body,
                                additional_data,
                                token,
                                propose_id,
                                propose_media,
                                notification_type,
                                module_id,
                            ));
                        }
                    }
                }
                Err(Error::InvalidNotificationFormat)
            },
            _ => Err(Error::InvalidPubSubType),
        }
    }

    pub fn get_moduleid(&self) -> Option<&String> {
        self.moduleid.as_ref()
    }

    pub fn to_string(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

impl FromStr for Notificacion {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s).map_err(|_| Error::InvalidNotificationString)
    }
}
