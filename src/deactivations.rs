#![cfg_attr(feature = "sync", allow(clippy::needless_pass_by_value))]

use http::Method;
use serde::Deserialize;
use time::{Date, Month};
#[cfg(feature = "async")]
use tracing::Instrument as _;

#[cfg(feature = "sync")]
use crate::blocking_client::BlockingTwilioAccount;
#[cfg(feature = "async")]
use crate::client::TwilioAccount;
use crate::common::{
    ApiFamily, RequestSpec, TwilioCreds, TwilioError, redacted_option, request_span,
};

/// Query parameters for `GET /Deactivations`.
#[derive(Clone, Copy)]
pub struct FetchDeactivationsRequest<'a> {
    date: &'a str,
}

impl<'a> FetchDeactivationsRequest<'a> {
    /// Create a Deactivations request for a `YYYY-MM-DD` date.
    #[must_use]
    pub fn new(date: &'a str) -> Self {
        Self { date }
    }

    fn validate(self) -> Result<(), TwilioError> {
        validate_yyyy_mm_dd("Date", self.date)
    }

    fn sensitive_values(self, creds: &'a TwilioCreds) -> Vec<&'a str> {
        vec![creds.account_sid(), creds.auth_token(), self.date]
    }
}

/// Signed redirect URL for a carrier deactivation report.
#[derive(Clone)]
pub struct TwilioDeactivation {
    pub redirect_to: Option<String>,
}

impl std::fmt::Debug for TwilioDeactivation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwilioDeactivation")
            .field("redirect_to", &redacted_option(&self.redirect_to))
            .finish()
    }
}

#[derive(Deserialize)]
struct WireDeactivation {
    redirect_to: Option<String>,
}

impl WireDeactivation {
    fn into_deactivation(self) -> TwilioDeactivation {
        TwilioDeactivation {
            redirect_to: self.redirect_to,
        }
    }
}

/// Messaging v1 Deactivations collection.
#[derive(Clone, Copy)]
#[cfg(feature = "async")]
pub struct DeactivationsResource<'a> {
    account: TwilioAccount<'a>,
}

#[cfg(feature = "async")]
impl<'a> DeactivationsResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `GET /Deactivations`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub async fn fetch(
        self,
        request: FetchDeactivationsRequest<'a>,
    ) -> Result<TwilioDeactivation, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let spec = RequestSpec::new(ApiFamily::Messaging, Method::GET, ["Deactivations"])
                .operation("deactivations.fetch")
                .query("Date", request.date)
                .accept_status(307);
            let parsed: WireDeactivation =
                self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(parsed.into_deactivation())
        }
        .instrument(request_span(
            &self.account.client.config.messaging_base_url,
            "deactivations.fetch",
            "GET",
        ))
        .await
    }
}

/// Blocking Messaging v1 Deactivations collection.
#[derive(Clone, Copy)]
#[cfg(feature = "sync")]
pub struct BlockingDeactivationsResource<'a> {
    account: BlockingTwilioAccount<'a>,
}

#[cfg(feature = "sync")]
impl<'a> BlockingDeactivationsResource<'a> {
    pub(crate) fn new(account: BlockingTwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `GET /Deactivations`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub fn fetch(
        self,
        request: FetchDeactivationsRequest<'a>,
    ) -> Result<TwilioDeactivation, TwilioError> {
        request_span(
            &self.account.client.config.messaging_base_url,
            "deactivations.fetch",
            "GET",
        )
        .in_scope(|| {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let spec = RequestSpec::new(ApiFamily::Messaging, Method::GET, ["Deactivations"])
                .operation("deactivations.fetch")
                .query("Date", request.date)
                .accept_status(307);
            let parsed: WireDeactivation = self.account.send_spec_json(spec, &sensitive_values)?;
            Ok(parsed.into_deactivation())
        })
    }
}

fn validate_yyyy_mm_dd(name: &str, value: &str) -> Result<(), TwilioError> {
    let Some((year, month_day)) = value.split_once('-') else {
        return invalid_date(name);
    };
    let Some((month, day)) = month_day.split_once('-') else {
        return invalid_date(name);
    };
    if value.len() != 10 || year.len() != 4 || month.len() != 2 || day.len() != 2 {
        return invalid_date(name);
    }
    let Ok(year) = year.parse::<i32>() else {
        return invalid_date(name);
    };
    let Ok(month) = month.parse::<u8>() else {
        return invalid_date(name);
    };
    let Ok(day) = day.parse::<u8>() else {
        return invalid_date(name);
    };
    let Ok(month) = Month::try_from(month) else {
        return invalid_date(name);
    };
    Date::from_calendar_date(year, month, day).map_err(|_| {
        TwilioError::InvalidRequest(format!("{name} must be a valid YYYY-MM-DD date"))
    })?;
    Ok(())
}

fn invalid_date<T>(name: &str) -> Result<T, TwilioError> {
    Err(TwilioError::InvalidRequest(format!(
        "{name} must be a valid YYYY-MM-DD date"
    )))
}
