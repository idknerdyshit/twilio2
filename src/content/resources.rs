#![cfg_attr(feature = "sync", allow(clippy::needless_pass_by_value))]

use http::Method;
use url::Url;

#[cfg(feature = "sync")]
use crate::blocking_client::BlockingTwilioAccount;
#[cfg(feature = "async")]
use crate::client::TwilioAccount;
#[cfg(feature = "sync")]
use crate::common::BlockingTwilioPaginator;
use crate::common::{
    ApiFamily, ContentPageResource, RequestSpec, TwilioError, decode_json_response,
    validate_content_next_page_continuation,
};
#[cfg(feature = "async")]
use crate::common::{PageFuture, TwilioPaginator};

use super::models::{
    ContentSearchRequest, CreateContentRequest, DeleteContentRequest, ListContentRequest,
    SubmitWhatsAppApprovalRequest, TwilioContent, TwilioContentAndApprovals,
    TwilioContentAndApprovalsPage, TwilioContentApprovalStatus, TwilioContentPage,
    TwilioLegacyContent, TwilioLegacyContentPage, TwilioWhatsAppApprovalSubmission,
    UpdateContentRequest, WireContent, WireContentAndApprovalsPage, WireContentPage,
    WireLegacyContentPage,
};

fn validate_content_sid(value: &str) -> Result<(), TwilioError> {
    if value.len() == 34
        && value.starts_with("HX")
        && value[2..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        Ok(())
    } else {
        Err(TwilioError::InvalidRequest(
            "ContentSid must be an HX SID".to_owned(),
        ))
    }
}

fn query_sensitive_values(pairs: &[(String, String)]) -> Vec<&str> {
    pairs
        .iter()
        .filter(|(key, _)| key != "PageSize")
        .map(|(_, value)| value.as_str())
        .collect()
}

macro_rules! resources {
    (
        $account:ty,
        $root:ident,
        $v1:ident,
        $v2:ident,
        $contents:ident,
        $template:ident,
        $approvals:ident,
        $content_approvals:ident,
        $legacy:ident,
        $v2_contents:ident,
        $v2_content_approvals:ident
    ) => {
        #[derive(Clone, Copy)]
        pub struct $root<'a> {
            account: $account,
        }
        impl<'a> $root<'a> {
            pub(crate) fn new(account: $account) -> Self {
                Self { account }
            }
            #[must_use]
            pub fn v1(self) -> $v1<'a> {
                $v1 {
                    account: self.account,
                }
            }
            #[must_use]
            pub fn v2(self) -> $v2<'a> {
                $v2 {
                    account: self.account,
                }
            }
        }

        #[derive(Clone, Copy)]
        pub struct $v1<'a> {
            account: $account,
        }
        impl<'a> $v1<'a> {
            #[must_use]
            pub fn contents(self) -> $contents<'a> {
                $contents {
                    account: self.account,
                }
            }
            #[must_use]
            pub fn content(self, sid: &'a str) -> $template<'a> {
                $template {
                    account: self.account,
                    sid,
                }
            }
            #[must_use]
            pub fn content_and_approvals(self) -> $content_approvals<'a> {
                $content_approvals {
                    account: self.account,
                }
            }
            #[must_use]
            pub fn legacy_contents(self) -> $legacy<'a> {
                $legacy {
                    account: self.account,
                }
            }
        }

        #[derive(Clone, Copy)]
        pub struct $v2<'a> {
            account: $account,
        }
        impl<'a> $v2<'a> {
            #[must_use]
            pub fn contents(self) -> $v2_contents<'a> {
                $v2_contents {
                    account: self.account,
                }
            }
            #[must_use]
            pub fn content_and_approvals(self) -> $v2_content_approvals<'a> {
                $v2_content_approvals {
                    account: self.account,
                }
            }
        }

        #[derive(Clone, Copy)]
        pub struct $contents<'a> {
            account: $account,
        }
        #[derive(Clone, Copy)]
        pub struct $template<'a> {
            account: $account,
            sid: &'a str,
        }
        #[derive(Clone, Copy)]
        pub struct $approvals<'a> {
            account: $account,
            sid: &'a str,
        }
        #[derive(Clone, Copy)]
        pub struct $content_approvals<'a> {
            account: $account,
        }
        #[derive(Clone, Copy)]
        pub struct $legacy<'a> {
            account: $account,
        }
        #[derive(Clone, Copy)]
        pub struct $v2_contents<'a> {
            account: $account,
        }
        #[derive(Clone, Copy)]
        pub struct $v2_content_approvals<'a> {
            account: $account,
        }
    };
}

#[cfg(feature = "async")]
resources!(
    TwilioAccount<'a>,
    ContentResource,
    ContentV1Resource,
    ContentV2Resource,
    ContentsResource,
    ContentTemplateResource,
    ContentApprovalRequestsResource,
    ContentAndApprovalsResource,
    LegacyContentsResource,
    ContentV2ContentsResource,
    ContentV2AndApprovalsResource
);

#[cfg(feature = "sync")]
resources!(
    BlockingTwilioAccount<'a>,
    BlockingContentResource,
    BlockingContentV1Resource,
    BlockingContentV2Resource,
    BlockingContentsResource,
    BlockingContentTemplateResource,
    BlockingContentApprovalRequestsResource,
    BlockingContentAndApprovalsResource,
    BlockingLegacyContentsResource,
    BlockingContentV2ContentsResource,
    BlockingContentV2AndApprovalsResource
);

fn split_content_page(page: TwilioContentPage) -> (Vec<TwilioContent>, Option<String>) {
    (page.contents, page.meta.next_page_url)
}
fn split_content_approvals_page(
    page: TwilioContentAndApprovalsPage,
) -> (Vec<TwilioContentAndApprovals>, Option<String>) {
    (page.contents, page.meta.next_page_url)
}
fn split_legacy_page(page: TwilioLegacyContentPage) -> (Vec<TwilioLegacyContent>, Option<String>) {
    (page.contents, page.meta.next_page_url)
}

#[cfg(feature = "async")]
macro_rules! async_list_resource {
    ($resource:ident, $request:ty, $page:ty, $wire:ty, $item:ty, $split:ident,
     $family:expr, $page_resource:expr, $version:literal, $path:literal, $operation:literal) => {
        impl<'a> $resource<'a> {
            /// Fetch one validated page.
            ///
            /// # Errors
            /// Returns [`TwilioError`] for invalid filters, transport, API, decode, or pagination failures.
            pub async fn list(self, request: $request) -> Result<$page, TwilioError> {
                request.validate()?;
                let pairs = request.pairs();
                let values = query_sensitive_values(&pairs);
                let mut current = self.account.client.content_endpoint($version, &[$path])?;
                for (key, value) in &pairs {
                    current.query_pairs_mut().append_pair(key, value);
                }
                let spec = RequestSpec::new($family, Method::GET, [$path])
                    .operation($operation)
                    .query_pairs(pairs.clone());
                let raw = self.account.send_spec_raw(spec, &values).await?;
                self.read_page(&raw.output, &values, &current)
            }

            /// Follow a validated Twilio continuation URL.
            ///
            /// # Errors
            /// Returns [`TwilioError`] when metadata or the request fails.
            pub async fn list_page_url(self, next_page_url: &str) -> Result<$page, TwilioError> {
                let url = self
                    .account
                    .client
                    .content_page_url(next_page_url, $page_resource)?;
                let values = [next_page_url];
                let spec = RequestSpec::from_url(
                    $family,
                    Method::GET,
                    url.clone(),
                    concat!($operation, ".page"),
                );
                let raw = self.account.send_spec_raw(spec, &values).await?;
                self.read_page(&raw.output, &values, &url)
            }

            #[must_use]
            pub fn list_all(self) -> TwilioPaginator<'a, $page, $item> {
                self.list_all_with(<$request>::new())
            }

            #[must_use]
            pub fn list_all_with(self, request: $request) -> TwilioPaginator<'a, $page, $item> {
                let request = request.with_default_page_size();
                TwilioPaginator::new(
                    move |next| -> PageFuture<'a, $page> {
                        let request = request.clone();
                        Box::pin(async move {
                            match next {
                                Some(url) => self.list_page_url(&url).await,
                                None => self.list(request).await,
                            }
                        })
                    },
                    $split,
                )
            }

            fn read_page(
                self,
                raw: &crate::RawResponse,
                values: &[&str],
                current: &Url,
            ) -> Result<$page, TwilioError> {
                let page = decode_json_response::<$wire>(raw, values)?.into_page();
                if page
                    .meta
                    .key
                    .as_deref()
                    .is_some_and(|key| key != "contents")
                {
                    return Err(TwilioError::InvalidResponseMetadata(
                        "pagination metadata key is not contents".to_owned(),
                    ));
                }
                if let Some(next) = page.meta.next_page_url.as_deref() {
                    let next_url = self.account.client.content_page_url(next, $page_resource)?;
                    validate_content_next_page_continuation(current, &next_url, $page_resource)?;
                }
                Ok(page)
            }
        }
    };
}

#[cfg(feature = "async")]
async_list_resource!(
    ContentsResource,
    ListContentRequest,
    TwilioContentPage,
    WireContentPage,
    TwilioContent,
    split_content_page,
    ApiFamily::ContentV1,
    ContentPageResource::V1Content,
    "v1",
    "Content",
    "content.v1.contents.list"
);
#[cfg(feature = "async")]
async_list_resource!(
    ContentAndApprovalsResource,
    ListContentRequest,
    TwilioContentAndApprovalsPage,
    WireContentAndApprovalsPage,
    TwilioContentAndApprovals,
    split_content_approvals_page,
    ApiFamily::ContentV1,
    ContentPageResource::V1ContentAndApprovals,
    "v1",
    "ContentAndApprovals",
    "content.v1.content_and_approvals.list"
);
#[cfg(feature = "async")]
async_list_resource!(
    LegacyContentsResource,
    ListContentRequest,
    TwilioLegacyContentPage,
    WireLegacyContentPage,
    TwilioLegacyContent,
    split_legacy_page,
    ApiFamily::ContentV1,
    ContentPageResource::V1LegacyContent,
    "v1",
    "LegacyContent",
    "content.v1.legacy_contents.list"
);
#[cfg(feature = "async")]
async_list_resource!(
    ContentV2ContentsResource,
    ContentSearchRequest,
    TwilioContentPage,
    WireContentPage,
    TwilioContent,
    split_content_page,
    ApiFamily::ContentV2,
    ContentPageResource::V2Content,
    "v2",
    "Content",
    "content.v2.contents.list"
);
#[cfg(feature = "async")]
async_list_resource!(
    ContentV2AndApprovalsResource,
    ContentSearchRequest,
    TwilioContentAndApprovalsPage,
    WireContentAndApprovalsPage,
    TwilioContentAndApprovals,
    split_content_approvals_page,
    ApiFamily::ContentV2,
    ContentPageResource::V2ContentAndApprovals,
    "v2",
    "ContentAndApprovals",
    "content.v2.content_and_approvals.list"
);

#[cfg(feature = "sync")]
macro_rules! blocking_list_resource {
    ($resource:ident, $request:ty, $page:ty, $wire:ty, $item:ty, $split:ident,
     $family:expr, $page_resource:expr, $version:literal, $path:literal, $operation:literal) => {
        impl<'a> $resource<'a> {
            /// Fetch one validated page.
            ///
            /// # Errors
            /// Returns [`TwilioError`] for invalid filters, transport, API, decode, or pagination failures.
            pub fn list(self, request: $request) -> Result<$page, TwilioError> {
                request.validate()?;
                let pairs = request.pairs();
                let values = query_sensitive_values(&pairs);
                let mut current = self.account.client.content_endpoint($version, &[$path])?;
                for (key, value) in &pairs {
                    current.query_pairs_mut().append_pair(key, value);
                }
                let spec = RequestSpec::new($family, Method::GET, [$path])
                    .operation($operation)
                    .query_pairs(pairs.clone());
                let raw = self.account.send_spec_raw(spec, &values)?;
                self.read_page(&raw.output, &values, &current)
            }

            /// Follow a validated Twilio continuation URL.
            ///
            /// # Errors
            /// Returns [`TwilioError`] when metadata or the request fails.
            pub fn list_page_url(self, next_page_url: &str) -> Result<$page, TwilioError> {
                let url = self
                    .account
                    .client
                    .content_page_url(next_page_url, $page_resource)?;
                let values = [next_page_url];
                let spec = RequestSpec::from_url(
                    $family,
                    Method::GET,
                    url.clone(),
                    concat!($operation, ".page"),
                );
                let raw = self.account.send_spec_raw(spec, &values)?;
                self.read_page(&raw.output, &values, &url)
            }

            #[must_use]
            pub fn list_all(self) -> BlockingTwilioPaginator<'a, $page, $item> {
                self.list_all_with(<$request>::new())
            }

            #[must_use]
            pub fn list_all_with(
                self,
                request: $request,
            ) -> BlockingTwilioPaginator<'a, $page, $item> {
                let request = request.with_default_page_size();
                BlockingTwilioPaginator::new(
                    move |next| match next {
                        Some(url) => self.list_page_url(&url),
                        None => self.list(request.clone()),
                    },
                    $split,
                )
            }

            fn read_page(
                self,
                raw: &crate::RawResponse,
                values: &[&str],
                current: &Url,
            ) -> Result<$page, TwilioError> {
                let page = decode_json_response::<$wire>(raw, values)?.into_page();
                if page
                    .meta
                    .key
                    .as_deref()
                    .is_some_and(|key| key != "contents")
                {
                    return Err(TwilioError::InvalidResponseMetadata(
                        "pagination metadata key is not contents".to_owned(),
                    ));
                }
                if let Some(next) = page.meta.next_page_url.as_deref() {
                    let next_url = self.account.client.content_page_url(next, $page_resource)?;
                    validate_content_next_page_continuation(current, &next_url, $page_resource)?;
                }
                Ok(page)
            }
        }
    };
}

#[cfg(feature = "sync")]
blocking_list_resource!(
    BlockingContentsResource,
    ListContentRequest,
    TwilioContentPage,
    WireContentPage,
    TwilioContent,
    split_content_page,
    ApiFamily::ContentV1,
    ContentPageResource::V1Content,
    "v1",
    "Content",
    "content.v1.contents.list"
);
#[cfg(feature = "sync")]
blocking_list_resource!(
    BlockingContentAndApprovalsResource,
    ListContentRequest,
    TwilioContentAndApprovalsPage,
    WireContentAndApprovalsPage,
    TwilioContentAndApprovals,
    split_content_approvals_page,
    ApiFamily::ContentV1,
    ContentPageResource::V1ContentAndApprovals,
    "v1",
    "ContentAndApprovals",
    "content.v1.content_and_approvals.list"
);
#[cfg(feature = "sync")]
blocking_list_resource!(
    BlockingLegacyContentsResource,
    ListContentRequest,
    TwilioLegacyContentPage,
    WireLegacyContentPage,
    TwilioLegacyContent,
    split_legacy_page,
    ApiFamily::ContentV1,
    ContentPageResource::V1LegacyContent,
    "v1",
    "LegacyContent",
    "content.v1.legacy_contents.list"
);
#[cfg(feature = "sync")]
blocking_list_resource!(
    BlockingContentV2ContentsResource,
    ContentSearchRequest,
    TwilioContentPage,
    WireContentPage,
    TwilioContent,
    split_content_page,
    ApiFamily::ContentV2,
    ContentPageResource::V2Content,
    "v2",
    "Content",
    "content.v2.contents.list"
);
#[cfg(feature = "sync")]
blocking_list_resource!(
    BlockingContentV2AndApprovalsResource,
    ContentSearchRequest,
    TwilioContentAndApprovalsPage,
    WireContentAndApprovalsPage,
    TwilioContentAndApprovals,
    split_content_approvals_page,
    ApiFamily::ContentV2,
    ContentPageResource::V2ContentAndApprovals,
    "v2",
    "ContentAndApprovals",
    "content.v2.content_and_approvals.list"
);

#[cfg(feature = "async")]
impl ContentsResource<'_> {
    /// Create a Content template.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, API, or decode failures.
    pub async fn create(self, request: CreateContentRequest) -> Result<TwilioContent, TwilioError> {
        request.validate()?;
        let spec = RequestSpec::new(ApiFamily::ContentV1, Method::POST, ["Content"])
            .operation("content.v1.contents.create")
            .json_body(&request)?;
        let wire: WireContent = self.account.send_spec_json(spec, &[]).await?;
        Ok(wire.into_content())
    }
}

#[cfg(feature = "sync")]
impl BlockingContentsResource<'_> {
    /// Create a Content template.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, API, or decode failures.
    pub fn create(self, request: CreateContentRequest) -> Result<TwilioContent, TwilioError> {
        request.validate()?;
        let spec = RequestSpec::new(ApiFamily::ContentV1, Method::POST, ["Content"])
            .operation("content.v1.contents.create")
            .json_body(&request)?;
        let wire: WireContent = self.account.send_spec_json(spec, &[])?;
        Ok(wire.into_content())
    }
}

macro_rules! template_common {
    ($template:ident, $approvals:ident) => {
        impl<'a> $template<'a> {
            #[must_use]
            pub fn approval_requests(self) -> $approvals<'a> {
                $approvals {
                    account: self.account,
                    sid: self.sid,
                }
            }
        }
    };
}
#[cfg(feature = "async")]
template_common!(ContentTemplateResource, ContentApprovalRequestsResource);
#[cfg(feature = "sync")]
template_common!(
    BlockingContentTemplateResource,
    BlockingContentApprovalRequestsResource
);

#[cfg(feature = "async")]
impl ContentTemplateResource<'_> {
    /// Fetch the template.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, API, or decode failures.
    pub async fn fetch(self) -> Result<TwilioContent, TwilioError> {
        validate_content_sid(self.sid)?;
        let spec = RequestSpec::new(ApiFamily::ContentV1, Method::GET, ["Content", self.sid])
            .operation("content.v1.content.fetch");
        let wire: WireContent = self.account.send_spec_json(spec, &[self.sid]).await?;
        Ok(wire.into_content())
    }
    /// Update the template.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, API, or decode failures.
    pub async fn update(self, request: UpdateContentRequest) -> Result<TwilioContent, TwilioError> {
        validate_content_sid(self.sid)?;
        request.validate()?;
        let spec = RequestSpec::new(ApiFamily::ContentV1, Method::PUT, ["Content", self.sid])
            .operation("content.v1.content.update")
            .json_body(&request)?;
        let wire: WireContent = self.account.send_spec_json(spec, &[self.sid]).await?;
        Ok(wire.into_content())
    }
    /// Delete the template.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, or API failures.
    pub async fn delete(self, request: DeleteContentRequest) -> Result<(), TwilioError> {
        validate_content_sid(self.sid)?;
        let mut spec =
            RequestSpec::new(ApiFamily::ContentV1, Method::DELETE, ["Content", self.sid])
                .operation("content.v1.content.delete");
        if let Some(value) = request.delete_in_waba {
            spec = spec.query("deleteInWaba", value.to_string());
        }
        self.account.send_spec_empty(spec, &[self.sid]).await
    }
}

#[cfg(feature = "sync")]
impl BlockingContentTemplateResource<'_> {
    /// Fetch the template.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, API, or decode failures.
    pub fn fetch(self) -> Result<TwilioContent, TwilioError> {
        validate_content_sid(self.sid)?;
        let spec = RequestSpec::new(ApiFamily::ContentV1, Method::GET, ["Content", self.sid])
            .operation("content.v1.content.fetch");
        let wire: WireContent = self.account.send_spec_json(spec, &[self.sid])?;
        Ok(wire.into_content())
    }
    /// Update the template.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, API, or decode failures.
    pub fn update(self, request: UpdateContentRequest) -> Result<TwilioContent, TwilioError> {
        validate_content_sid(self.sid)?;
        request.validate()?;
        let spec = RequestSpec::new(ApiFamily::ContentV1, Method::PUT, ["Content", self.sid])
            .operation("content.v1.content.update")
            .json_body(&request)?;
        let wire: WireContent = self.account.send_spec_json(spec, &[self.sid])?;
        Ok(wire.into_content())
    }
    /// Delete the template.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, or API failures.
    pub fn delete(self, request: DeleteContentRequest) -> Result<(), TwilioError> {
        validate_content_sid(self.sid)?;
        let mut spec =
            RequestSpec::new(ApiFamily::ContentV1, Method::DELETE, ["Content", self.sid])
                .operation("content.v1.content.delete");
        if let Some(value) = request.delete_in_waba {
            spec = spec.query("deleteInWaba", value.to_string());
        }
        self.account.send_spec_empty(spec, &[self.sid])
    }
}

#[cfg(feature = "async")]
impl ContentApprovalRequestsResource<'_> {
    /// Submit this template for `WhatsApp` approval.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, API, or decode failures.
    pub async fn submit_whatsapp(
        self,
        request: SubmitWhatsAppApprovalRequest,
    ) -> Result<TwilioWhatsAppApprovalSubmission, TwilioError> {
        validate_content_sid(self.sid)?;
        request.validate()?;
        let spec = RequestSpec::new(
            ApiFamily::ContentV1,
            Method::POST,
            ["Content", self.sid, "ApprovalRequests", "whatsapp"],
        )
        .operation("content.v1.approvals.submit_whatsapp")
        .json_body(&request)?;
        self.account.send_spec_json(spec, &[self.sid]).await
    }
    /// Fetch this template's approval status.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, API, or decode failures.
    pub async fn fetch(self) -> Result<TwilioContentApprovalStatus, TwilioError> {
        validate_content_sid(self.sid)?;
        let spec = RequestSpec::new(
            ApiFamily::ContentV1,
            Method::GET,
            ["Content", self.sid, "ApprovalRequests"],
        )
        .operation("content.v1.approvals.fetch");
        self.account.send_spec_json(spec, &[self.sid]).await
    }
}

#[cfg(feature = "sync")]
impl BlockingContentApprovalRequestsResource<'_> {
    /// Submit this template for `WhatsApp` approval.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, API, or decode failures.
    pub fn submit_whatsapp(
        self,
        request: SubmitWhatsAppApprovalRequest,
    ) -> Result<TwilioWhatsAppApprovalSubmission, TwilioError> {
        validate_content_sid(self.sid)?;
        request.validate()?;
        let spec = RequestSpec::new(
            ApiFamily::ContentV1,
            Method::POST,
            ["Content", self.sid, "ApprovalRequests", "whatsapp"],
        )
        .operation("content.v1.approvals.submit_whatsapp")
        .json_body(&request)?;
        self.account.send_spec_json(spec, &[self.sid])
    }
    /// Fetch this template's approval status.
    ///
    /// # Errors
    /// Returns [`TwilioError`] for validation, transport, API, or decode failures.
    pub fn fetch(self) -> Result<TwilioContentApprovalStatus, TwilioError> {
        validate_content_sid(self.sid)?;
        let spec = RequestSpec::new(
            ApiFamily::ContentV1,
            Method::GET,
            ["Content", self.sid, "ApprovalRequests"],
        )
        .operation("content.v1.approvals.fetch");
        self.account.send_spec_json(spec, &[self.sid])
    }
}
