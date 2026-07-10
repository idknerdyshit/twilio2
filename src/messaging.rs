#![cfg_attr(feature = "sync", allow(clippy::needless_pass_by_value))]

#[cfg(feature = "async")]
use crate::a2p::{A2PBrandRegistrationResource, A2PBrandRegistrationsResource};
#[cfg(feature = "sync")]
use crate::a2p::{BlockingA2PBrandRegistrationResource, BlockingA2PBrandRegistrationsResource};
#[cfg(feature = "sync")]
use crate::blocking_client::BlockingTwilioAccount;
#[cfg(feature = "sync")]
use crate::channel_senders::BlockingMessagingV2ChannelSendersResource;
#[cfg(feature = "async")]
use crate::channel_senders::MessagingV2ChannelSendersResource;
#[cfg(feature = "async")]
use crate::client::TwilioAccount;
#[cfg(feature = "sync")]
use crate::deactivations::BlockingDeactivationsResource;
#[cfg(feature = "async")]
use crate::deactivations::DeactivationsResource;
#[cfg(feature = "sync")]
use crate::link_shortening::{
    BlockingMessagingV1LinkShorteningResource, BlockingMessagingV2LinkShorteningResource,
};
#[cfg(feature = "async")]
use crate::link_shortening::{
    MessagingV1LinkShorteningResource, MessagingV2LinkShorteningResource,
};
#[cfg(feature = "sync")]
use crate::services::{BlockingServiceResource, BlockingServicesResource};
#[cfg(feature = "async")]
use crate::services::{ServiceResource, ServicesResource};
#[cfg(feature = "sync")]
use crate::tollfree_verifications::{
    BlockingTollfreeVerificationResource, BlockingTollfreeVerificationsResource,
};
#[cfg(feature = "async")]
use crate::tollfree_verifications::{TollfreeVerificationResource, TollfreeVerificationsResource};
#[cfg(feature = "sync")]
use crate::typing_indicators::{
    BlockingMessagingV2TypingIndicatorsResource, BlockingMessagingV3TypingIndicatorsResource,
};
#[cfg(feature = "async")]
use crate::typing_indicators::{
    MessagingV2TypingIndicatorsResource, MessagingV3TypingIndicatorsResource,
};

/// Twilio Messaging product root.
#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct MessagingResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> MessagingResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// Messaging v1 resources.
    #[must_use]
    pub fn v1(self) -> MessagingV1Resource<'a> {
        MessagingV1Resource {
            account: self.account,
        }
    }

    /// Messaging v2 resources.
    #[must_use]
    pub fn v2(self) -> MessagingV2Resource<'a> {
        MessagingV2Resource {
            account: self.account,
        }
    }

    /// Messaging v3 resources.
    #[must_use]
    pub fn v3(self) -> MessagingV3Resource<'a> {
        MessagingV3Resource {
            account: self.account,
        }
    }
}

/// Twilio Messaging v1 resources.
#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct MessagingV1Resource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> MessagingV1Resource<'a> {
    /// Messaging v1 Deactivations collection.
    #[must_use]
    pub fn deactivations(self) -> DeactivationsResource<'a> {
        DeactivationsResource::new(self.account)
    }

    /// Messaging Services collection.
    #[must_use]
    pub fn services(self) -> ServicesResource<'a> {
        ServicesResource::new(self.account)
    }

    /// One Messaging Service resource and its subresources.
    #[must_use]
    pub fn service(self, sid: &'a str) -> ServiceResource<'a> {
        ServiceResource::new(self.account, sid)
    }

    /// Messaging v1 Toll-free Verifications collection.
    #[must_use]
    pub fn tollfree_verifications(self) -> TollfreeVerificationsResource<'a> {
        TollfreeVerificationsResource::new(self.account)
    }

    /// One Messaging v1 Toll-free Verification resource.
    #[must_use]
    pub fn tollfree_verification(self, sid: &'a str) -> TollfreeVerificationResource<'a> {
        TollfreeVerificationResource::new(self.account, sid)
    }

    /// Messaging v1 A2P 10DLC Brand Registrations collection.
    #[must_use]
    pub fn a2p_brand_registrations(self) -> A2PBrandRegistrationsResource<'a> {
        A2PBrandRegistrationsResource::new(self.account)
    }

    /// One Messaging v1 A2P 10DLC Brand Registration resource.
    #[must_use]
    pub fn a2p_brand_registration(self, sid: &'a str) -> A2PBrandRegistrationResource<'a> {
        A2PBrandRegistrationResource::new(self.account, sid)
    }

    /// Messaging v1 Link Shortening resources.
    #[must_use]
    pub fn link_shortening(self) -> MessagingV1LinkShorteningResource<'a> {
        MessagingV1LinkShorteningResource::new(self.account)
    }
}

/// Twilio Messaging v2 resources.
#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct MessagingV2Resource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> MessagingV2Resource<'a> {
    /// Standalone Messaging v2 channel senders.
    #[must_use]
    pub fn channel_senders(self) -> MessagingV2ChannelSendersResource<'a> {
        MessagingV2ChannelSendersResource::new(self.account)
    }

    /// Messaging v2 typing indicators.
    #[must_use]
    pub fn typing_indicators(self) -> MessagingV2TypingIndicatorsResource<'a> {
        MessagingV2TypingIndicatorsResource::new(self.account)
    }

    /// Messaging v2 Link Shortening resources.
    #[must_use]
    pub fn link_shortening(self) -> MessagingV2LinkShorteningResource<'a> {
        MessagingV2LinkShorteningResource::new(self.account)
    }
}

/// Twilio Messaging v3 resources.
#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct MessagingV3Resource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> MessagingV3Resource<'a> {
    /// Messaging v3 typing indicators.
    #[must_use]
    pub fn typing_indicators(self) -> MessagingV3TypingIndicatorsResource<'a> {
        MessagingV3TypingIndicatorsResource::new(self.account)
    }
}

/// Blocking Twilio Messaging product root.
#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingMessagingResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingMessagingResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// Messaging v1 resources.
    #[must_use]
    pub fn v1(self) -> BlockingMessagingV1Resource<'a> {
        BlockingMessagingV1Resource {
            account: self.account,
        }
    }

    /// Messaging v2 resources.
    #[must_use]
    pub fn v2(self) -> BlockingMessagingV2Resource<'a> {
        BlockingMessagingV2Resource {
            account: self.account,
        }
    }

    /// Messaging v3 resources.
    #[must_use]
    pub fn v3(self) -> BlockingMessagingV3Resource<'a> {
        BlockingMessagingV3Resource {
            account: self.account,
        }
    }
}

/// Blocking Twilio Messaging v1 resources.
#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingMessagingV1Resource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingMessagingV1Resource<'a> {
    /// Messaging v1 Deactivations collection.
    #[must_use]
    pub fn deactivations(self) -> BlockingDeactivationsResource<'a> {
        BlockingDeactivationsResource::new(self.account)
    }

    /// Messaging Services collection.
    #[must_use]
    pub fn services(self) -> BlockingServicesResource<'a> {
        BlockingServicesResource::new(self.account)
    }

    /// One Messaging Service resource and its subresources.
    #[must_use]
    pub fn service(self, sid: &'a str) -> BlockingServiceResource<'a> {
        BlockingServiceResource::new(self.account, sid)
    }

    /// Messaging v1 Toll-free Verifications collection.
    #[must_use]
    pub fn tollfree_verifications(self) -> BlockingTollfreeVerificationsResource<'a> {
        BlockingTollfreeVerificationsResource::new(self.account)
    }

    /// One Messaging v1 Toll-free Verification resource.
    #[must_use]
    pub fn tollfree_verification(self, sid: &'a str) -> BlockingTollfreeVerificationResource<'a> {
        BlockingTollfreeVerificationResource::new(self.account, sid)
    }

    /// Messaging v1 A2P 10DLC Brand Registrations collection.
    #[must_use]
    pub fn a2p_brand_registrations(self) -> BlockingA2PBrandRegistrationsResource<'a> {
        BlockingA2PBrandRegistrationsResource::new(self.account)
    }

    /// One Messaging v1 A2P 10DLC Brand Registration resource.
    #[must_use]
    pub fn a2p_brand_registration(self, sid: &'a str) -> BlockingA2PBrandRegistrationResource<'a> {
        BlockingA2PBrandRegistrationResource::new(self.account, sid)
    }

    /// Messaging v1 Link Shortening resources.
    #[must_use]
    pub fn link_shortening(self) -> BlockingMessagingV1LinkShorteningResource<'a> {
        BlockingMessagingV1LinkShorteningResource::new(self.account)
    }
}

/// Blocking Twilio Messaging v2 resources.
#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingMessagingV2Resource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingMessagingV2Resource<'a> {
    /// Standalone Messaging v2 channel senders.
    #[must_use]
    pub fn channel_senders(self) -> BlockingMessagingV2ChannelSendersResource<'a> {
        BlockingMessagingV2ChannelSendersResource::new(self.account)
    }

    /// Messaging v2 typing indicators.
    #[must_use]
    pub fn typing_indicators(self) -> BlockingMessagingV2TypingIndicatorsResource<'a> {
        BlockingMessagingV2TypingIndicatorsResource::new(self.account)
    }

    /// Messaging v2 Link Shortening resources.
    #[must_use]
    pub fn link_shortening(self) -> BlockingMessagingV2LinkShorteningResource<'a> {
        BlockingMessagingV2LinkShorteningResource::new(self.account)
    }
}

/// Blocking Twilio Messaging v3 resources.
#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingMessagingV3Resource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingMessagingV3Resource<'a> {
    /// Messaging v3 typing indicators.
    #[must_use]
    pub fn typing_indicators(self) -> BlockingMessagingV3TypingIndicatorsResource<'a> {
        BlockingMessagingV3TypingIndicatorsResource::new(self.account)
    }
}
