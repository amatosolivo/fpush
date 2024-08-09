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
}

impl Notificacion {
    pub fn new(
        message_count: u32,
        last_message_sender: String,
        last_message_body: String,
        additional_data: Option<String>,
        token: String,
    ) -> Self {
        Notificacion {
            message_count,
            last_message_sender,
            last_message_body,
            additional_data,
            token,
        }
    }

    pub fn from_pubsub(pubsub: &PubSub) -> Result<Self, Error> {
        match pubsub {
            PubSub::Publish { publish, publish_options: _ } => {
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
                            
                            return Ok(Notificacion::new(
                                message_count,
                                last_message_sender,
                                last_message_body,
                                additional_data,
                                token,
                            ));
                        }
                    }
                }
                Err(Error::InvalidNotificationFormat)
            },
            _ => Err(Error::InvalidPubSubType),
        }
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
