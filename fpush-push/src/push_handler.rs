use crate::error::{PushRequestError, PushRequestResult};

use crate::push_module::PushModuleEnum;
use fpush_traits::push::PushError;

use log::{info, warn};

#[inline(always)]
pub async fn handle_push_request(
    push_module: &PushModuleEnum,
    token: String,
) -> PushRequestResult<()> {
    if push_module.blocklist().is_blocked(&token) {
        return Err(PushRequestError::TokenBlocked);
    }
    if push_module
        .ratelimit()
        .lookup_ratelimit(token.to_string())
        .await
    {
        match push_module.send(token.to_string()).await {
            Ok(()) => {
                info!(
                    "{}: Send push message OK",
                    push_module.identifier()
                );
                Ok(())
            }
            Err(PushError::TokenBlocked) => {
                info!(
                    "{}: Received push request from blocked",
                    push_module.identifier()
                );
                push_module.blocklist().block_invalid_token(token);
                Err(PushRequestError::TokenBlocked)
            }
            Err(PushError::TokenRateLimited) => {
                push_module.ratelimit().hard_ratelimit(token.to_string());
                Err(PushRequestError::TokenRatelimited)
            }
            Err(PushError::PushEndpointTmp) => Err(PushRequestError::Internal),
            Err(PushError::PushEndpointPersistent) => Err(PushRequestError::Internal),
            Err(e) => {
                warn!(
                    "{}: Blocking token {}",
                    push_module.identifier(),
                    e
                );
                push_module
                    .blocklist()
                    .block_after_unhandled_push_error(token);
                Err(PushRequestError::Internal)
            }
        }
    } else {
        info!(
            "{}: Ignoring push request for token {} due to ratelimit",
            push_module.identifier(),
            token,
        );
        Err(PushRequestError::TokenRatelimited)
    }
}
