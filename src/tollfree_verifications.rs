use std::collections::BTreeMap;

use reqwest::{Method, Url};
use serde::Deserialize;
use time::OffsetDateTime;
use tracing::Instrument as _;

use crate::client::TwilioAccount;
use crate::common::{
    ApiFamily, DEFAULT_PAGE_SIZE, FormParam, PageFuture, RequestSpec, TwilioCreds, TwilioError,
    TwilioPaginator, V1PageMeta, V1PageResource, WireV1PageMeta, decode_json_response,
    has_non_empty, parse_iso8601, push_bool, push_sensitive, push_str, redacted_option,
    request_span, validate_page_size, validate_v1_meta_key, validate_v1_next_page_continuation,
};

/// Toll-free Verification use-case category request values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TollfreeUseCaseCategory<'a> {
    TwoFactorAuthentication,
    AccountNotifications,
    CustomerCare,
    CharityNonprofit,
    DeliveryNotifications,
    FraudAlertMessaging,
    Events,
    HigherEducation,
    K12,
    Marketing,
    Mixed,
    PollingAndVotingNonPolitical,
    PoliticalElectionCampaigns,
    PublicServiceAnnouncement,
    SecurityAlert,
    Raw(&'a str),
}

impl<'a> TollfreeUseCaseCategory<'a> {
    fn form_value(self) -> &'a str {
        match self {
            Self::TwoFactorAuthentication => "TWO_FACTOR_AUTHENTICATION",
            Self::AccountNotifications => "ACCOUNT_NOTIFICATIONS",
            Self::CustomerCare => "CUSTOMER_CARE",
            Self::CharityNonprofit => "CHARITY_NONPROFIT",
            Self::DeliveryNotifications => "DELIVERY_NOTIFICATIONS",
            Self::FraudAlertMessaging => "FRAUD_ALERT_MESSAGING",
            Self::Events => "EVENTS",
            Self::HigherEducation => "HIGHER_EDUCATION",
            Self::K12 => "K12",
            Self::Marketing => "MARKETING",
            Self::Mixed => "MIXED",
            Self::PollingAndVotingNonPolitical => "POLLING_AND_VOTING_NON_POLITICAL",
            Self::PoliticalElectionCampaigns => "POLITICAL_ELECTION_CAMPAIGNS",
            Self::PublicServiceAnnouncement => "PUBLIC_SERVICE_ANNOUNCEMENT",
            Self::SecurityAlert => "SECURITY_ALERT",
            Self::Raw(value) => value,
        }
    }
}

/// Toll-free Verification opt-in type request values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TollfreeOptInType<'a> {
    Verbal,
    WebForm,
    PaperForm,
    ViaText,
    MobileQrCode,
    Import,
    ImportPleaseReplace,
    Raw(&'a str),
}

impl<'a> TollfreeOptInType<'a> {
    fn form_value(self) -> &'a str {
        match self {
            Self::Verbal => "VERBAL",
            Self::WebForm => "WEB_FORM",
            Self::PaperForm => "PAPER_FORM",
            Self::ViaText => "VIA_TEXT",
            Self::MobileQrCode => "MOBILE_QR_CODE",
            Self::Import => "IMPORT",
            Self::ImportPleaseReplace => "IMPORT_PLEASE_REPLACE",
            Self::Raw(value) => value,
        }
    }
}

/// Toll-free Verification monthly message volume request values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TollfreeMessageVolume<'a> {
    Ten,
    Hundred,
    Thousand,
    TenThousand,
    HundredThousand,
    TwoHundredFiftyThousand,
    FiveHundredThousand,
    SevenHundredFiftyThousand,
    OneMillion,
    FiveMillion,
    TenMillionPlus,
    Raw(&'a str),
}

impl<'a> TollfreeMessageVolume<'a> {
    fn form_value(self) -> &'a str {
        match self {
            Self::Ten => "10",
            Self::Hundred => "100",
            Self::Thousand => "1,000",
            Self::TenThousand => "10,000",
            Self::HundredThousand => "100,000",
            Self::TwoHundredFiftyThousand => "250,000",
            Self::FiveHundredThousand => "500,000",
            Self::SevenHundredFiftyThousand => "750,000",
            Self::OneMillion => "1,000,000",
            Self::FiveMillion => "5,000,000",
            Self::TenMillionPlus => "10,000,000+",
            Self::Raw(value) => value,
        }
    }
}

/// Toll-free Verification list status request values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TollfreeVerificationStatus<'a> {
    PendingReview,
    InReview,
    TwilioApproved,
    TwilioRejected,
    Raw(&'a str),
}

impl<'a> TollfreeVerificationStatus<'a> {
    fn form_value(self) -> &'a str {
        match self {
            Self::PendingReview => "PENDING_REVIEW",
            Self::InReview => "IN_REVIEW",
            Self::TwilioApproved => "TWILIO_APPROVED",
            Self::TwilioRejected => "TWILIO_REJECTED",
            Self::Raw(value) => value,
        }
    }
}

/// Business registration authority request values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TollfreeBusinessRegistrationAuthority<'a> {
    Ein,
    Cbn,
    Crn,
    ProvincialNumber,
    Vat,
    Acn,
    Abn,
    Brn,
    Siren,
    Siret,
    Nzbn,
    UstIdNr,
    Cif,
    Nif,
    Cnpj,
    Uid,
    Neq,
    Other,
    Raw(&'a str),
}

impl<'a> TollfreeBusinessRegistrationAuthority<'a> {
    fn form_value(self) -> &'a str {
        match self {
            Self::Ein => "EIN",
            Self::Cbn => "CBN",
            Self::Crn => "CRN",
            Self::ProvincialNumber => "PROVINCIAL_NUMBER",
            Self::Vat => "VAT",
            Self::Acn => "ACN",
            Self::Abn => "ABN",
            Self::Brn => "BRN",
            Self::Siren => "SIREN",
            Self::Siret => "SIRET",
            Self::Nzbn => "NZBN",
            Self::UstIdNr => "USt-IdNr",
            Self::Cif => "CIF",
            Self::Nif => "NIF",
            Self::Cnpj => "CNPJ",
            Self::Uid => "UID",
            Self::Neq => "NEQ",
            Self::Other => "OTHER",
            Self::Raw(value) => value,
        }
    }
}

/// Business type request values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TollfreeBusinessType<'a> {
    PrivateProfit,
    PublicProfit,
    SoleProprietor,
    NonProfit,
    Government,
    Raw(&'a str),
}

impl<'a> TollfreeBusinessType<'a> {
    fn form_value(self) -> &'a str {
        match self {
            Self::PrivateProfit => "PRIVATE_PROFIT",
            Self::PublicProfit => "PUBLIC_PROFIT",
            Self::SoleProprietor => "SOLE_PROPRIETOR",
            Self::NonProfit => "NON_PROFIT",
            Self::Government => "GOVERNMENT",
            Self::Raw(value) => value,
        }
    }
}

/// Political vetting provider request values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TollfreeVettingProvider<'a> {
    CampaignVerify,
    Raw(&'a str),
}

impl<'a> TollfreeVettingProvider<'a> {
    fn form_value(self) -> &'a str {
        match self {
            Self::CampaignVerify => "CAMPAIGN_VERIFY",
            Self::Raw(value) => value,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct TollfreeVerificationFields<'a> {
    business_name: Option<&'a str>,
    business_website: Option<&'a str>,
    notification_email: Option<&'a str>,
    use_case_categories: &'a [TollfreeUseCaseCategory<'a>],
    use_case_summary: Option<&'a str>,
    production_message_sample: Option<&'a str>,
    opt_in_image_urls: &'a [&'a str],
    opt_in_type: Option<TollfreeOptInType<'a>>,
    message_volume: Option<TollfreeMessageVolume<'a>>,
    tollfree_phone_number_sid: Option<&'a str>,
    customer_profile_sid: Option<&'a str>,
    business_street_address: Option<&'a str>,
    business_street_address2: Option<&'a str>,
    business_city: Option<&'a str>,
    business_state_province_region: Option<&'a str>,
    business_postal_code: Option<&'a str>,
    business_country: Option<&'a str>,
    additional_information: Option<&'a str>,
    business_contact_first_name: Option<&'a str>,
    business_contact_last_name: Option<&'a str>,
    business_contact_email: Option<&'a str>,
    business_contact_phone: Option<&'a str>,
    external_reference_id: Option<&'a str>,
    edit_reason: Option<&'a str>,
    business_registration_number: Option<&'a str>,
    business_registration_authority: Option<TollfreeBusinessRegistrationAuthority<'a>>,
    business_registration_country: Option<&'a str>,
    business_type: Option<TollfreeBusinessType<'a>>,
    business_registration_phone_number: Option<&'a str>,
    doing_business_as: Option<&'a str>,
    opt_in_confirmation_message: Option<&'a str>,
    help_message_sample: Option<&'a str>,
    privacy_policy_url: Option<&'a str>,
    terms_and_conditions_url: Option<&'a str>,
    age_gated_content: Option<bool>,
    opt_in_keywords: &'a [&'a str],
    vetting_provider: Option<TollfreeVettingProvider<'a>>,
    vetting_id: Option<&'a str>,
}

impl<'a> TollfreeVerificationFields<'a> {
    #[allow(
        clippy::too_many_lines,
        reason = "TFV form fields mirror Twilio's documented request schema in wire order."
    )]
    fn form_params(self) -> Vec<FormParam> {
        let mut params = Vec::new();
        push_str(&mut params, "BusinessName", self.business_name);
        push_str(&mut params, "BusinessWebsite", self.business_website);
        push_str(&mut params, "NotificationEmail", self.notification_email);
        for value in self.use_case_categories {
            push_str(&mut params, "UseCaseCategories", Some(value.form_value()));
        }
        push_str(&mut params, "UseCaseSummary", self.use_case_summary);
        push_str(
            &mut params,
            "ProductionMessageSample",
            self.production_message_sample,
        );
        for value in self.opt_in_image_urls {
            push_str(&mut params, "OptInImageUrls", Some(value));
        }
        push_tfv_enum(&mut params, "OptInType", self.opt_in_type);
        push_tfv_enum(&mut params, "MessageVolume", self.message_volume);
        push_str(
            &mut params,
            "TollfreePhoneNumberSid",
            self.tollfree_phone_number_sid,
        );
        push_str(&mut params, "CustomerProfileSid", self.customer_profile_sid);
        push_str(
            &mut params,
            "BusinessStreetAddress",
            self.business_street_address,
        );
        push_str(
            &mut params,
            "BusinessStreetAddress2",
            self.business_street_address2,
        );
        push_str(&mut params, "BusinessCity", self.business_city);
        push_str(
            &mut params,
            "BusinessStateProvinceRegion",
            self.business_state_province_region,
        );
        push_str(&mut params, "BusinessPostalCode", self.business_postal_code);
        push_str(&mut params, "BusinessCountry", self.business_country);
        push_str(
            &mut params,
            "AdditionalInformation",
            self.additional_information,
        );
        push_str(
            &mut params,
            "BusinessContactFirstName",
            self.business_contact_first_name,
        );
        push_str(
            &mut params,
            "BusinessContactLastName",
            self.business_contact_last_name,
        );
        push_str(
            &mut params,
            "BusinessContactEmail",
            self.business_contact_email,
        );
        push_str(
            &mut params,
            "BusinessContactPhone",
            self.business_contact_phone,
        );
        push_str(
            &mut params,
            "ExternalReferenceId",
            self.external_reference_id,
        );
        push_str(&mut params, "EditReason", self.edit_reason);
        push_str(
            &mut params,
            "BusinessRegistrationNumber",
            self.business_registration_number,
        );
        push_tfv_enum(
            &mut params,
            "BusinessRegistrationAuthority",
            self.business_registration_authority,
        );
        push_str(
            &mut params,
            "BusinessRegistrationCountry",
            self.business_registration_country,
        );
        push_tfv_enum(&mut params, "BusinessType", self.business_type);
        push_str(
            &mut params,
            "BusinessRegistrationPhoneNumber",
            self.business_registration_phone_number,
        );
        push_str(&mut params, "DoingBusinessAs", self.doing_business_as);
        push_str(
            &mut params,
            "OptInConfirmationMessage",
            self.opt_in_confirmation_message,
        );
        push_str(&mut params, "HelpMessageSample", self.help_message_sample);
        push_str(&mut params, "PrivacyPolicyUrl", self.privacy_policy_url);
        push_str(
            &mut params,
            "TermsAndConditionsUrl",
            self.terms_and_conditions_url,
        );
        push_bool(&mut params, "AgeGatedContent", self.age_gated_content);
        for value in self.opt_in_keywords {
            push_str(&mut params, "OptInKeywords", Some(value));
        }
        push_tfv_enum(&mut params, "VettingProvider", self.vetting_provider);
        push_str(&mut params, "VettingId", self.vetting_id);
        params
    }

    fn validate_common(self) -> Result<(), TwilioError> {
        validate_enum_slice("UseCaseCategories", self.use_case_categories)?;
        validate_str_slice("OptInImageUrls", self.opt_in_image_urls)?;
        validate_str_slice("OptInKeywords", self.opt_in_keywords)?;
        validate_optional_enum("OptInType", self.opt_in_type)?;
        validate_optional_enum("MessageVolume", self.message_volume)?;
        validate_optional_enum(
            "BusinessRegistrationAuthority",
            self.business_registration_authority,
        )?;
        validate_optional_enum("BusinessType", self.business_type)?;
        validate_optional_enum("VettingProvider", self.vetting_provider)
    }

    fn sensitive_values(self, creds: TwilioCreds<'a>, sid: Option<&'a str>) -> Vec<&'a str> {
        let mut values = vec![creds.account_sid, creds.auth_token];
        push_sensitive(&mut values, sid);
        push_sensitive(&mut values, self.business_name);
        push_sensitive(&mut values, self.business_website);
        push_sensitive(&mut values, self.notification_email);
        push_sensitive(&mut values, self.use_case_summary);
        push_sensitive(&mut values, self.production_message_sample);
        values.extend(self.opt_in_image_urls.iter().copied());
        push_sensitive(&mut values, self.tollfree_phone_number_sid);
        push_sensitive(&mut values, self.customer_profile_sid);
        push_sensitive(&mut values, self.business_street_address);
        push_sensitive(&mut values, self.business_street_address2);
        push_sensitive(&mut values, self.business_city);
        push_sensitive(&mut values, self.business_state_province_region);
        push_sensitive(&mut values, self.business_postal_code);
        push_sensitive(&mut values, self.business_country);
        push_sensitive(&mut values, self.additional_information);
        push_sensitive(&mut values, self.business_contact_first_name);
        push_sensitive(&mut values, self.business_contact_last_name);
        push_sensitive(&mut values, self.business_contact_email);
        push_sensitive(&mut values, self.business_contact_phone);
        push_sensitive(&mut values, self.external_reference_id);
        push_sensitive(&mut values, self.edit_reason);
        push_sensitive(&mut values, self.business_registration_number);
        push_sensitive(&mut values, self.business_registration_country);
        push_sensitive(&mut values, self.business_registration_phone_number);
        push_sensitive(&mut values, self.doing_business_as);
        push_sensitive(&mut values, self.opt_in_confirmation_message);
        push_sensitive(&mut values, self.help_message_sample);
        push_sensitive(&mut values, self.privacy_policy_url);
        push_sensitive(&mut values, self.terms_and_conditions_url);
        values.extend(self.opt_in_keywords.iter().copied());
        push_sensitive(&mut values, self.vetting_id);
        values
    }
}

trait TollfreeFormValue<'a>: Copy {
    fn form_value(self) -> &'a str;
}

impl<'a> TollfreeFormValue<'a> for TollfreeUseCaseCategory<'a> {
    fn form_value(self) -> &'a str {
        self.form_value()
    }
}

impl<'a> TollfreeFormValue<'a> for TollfreeOptInType<'a> {
    fn form_value(self) -> &'a str {
        self.form_value()
    }
}

impl<'a> TollfreeFormValue<'a> for TollfreeMessageVolume<'a> {
    fn form_value(self) -> &'a str {
        self.form_value()
    }
}

impl<'a> TollfreeFormValue<'a> for TollfreeVerificationStatus<'a> {
    fn form_value(self) -> &'a str {
        self.form_value()
    }
}

impl<'a> TollfreeFormValue<'a> for TollfreeBusinessRegistrationAuthority<'a> {
    fn form_value(self) -> &'a str {
        self.form_value()
    }
}

impl<'a> TollfreeFormValue<'a> for TollfreeBusinessType<'a> {
    fn form_value(self) -> &'a str {
        self.form_value()
    }
}

impl<'a> TollfreeFormValue<'a> for TollfreeVettingProvider<'a> {
    fn form_value(self) -> &'a str {
        self.form_value()
    }
}

fn push_tfv_enum<'a, T: TollfreeFormValue<'a>>(
    params: &mut Vec<FormParam>,
    key: &'static str,
    value: Option<T>,
) {
    if let Some(value) = value {
        push_str(params, key, Some(value.form_value()));
    }
}

fn validate_optional_enum<'a, T: TollfreeFormValue<'a>>(
    name: &str,
    value: Option<T>,
) -> Result<(), TwilioError> {
    if value.is_some_and(|value| value.form_value().trim().is_empty()) {
        return Err(TwilioError::InvalidRequest(format!(
            "{name} must not be empty"
        )));
    }
    Ok(())
}

fn validate_enum_slice<'a, T: TollfreeFormValue<'a>>(
    name: &str,
    values: &[T],
) -> Result<(), TwilioError> {
    if values
        .iter()
        .any(|value| value.form_value().trim().is_empty())
    {
        return Err(TwilioError::InvalidRequest(format!(
            "{name} values must not be empty"
        )));
    }
    Ok(())
}

fn validate_str_slice(name: &str, values: &[&str]) -> Result<(), TwilioError> {
    if values.iter().any(|value| value.trim().is_empty()) {
        return Err(TwilioError::InvalidRequest(format!(
            "{name} values must not be empty"
        )));
    }
    Ok(())
}

fn validate_required(name: &str, value: Option<&str>) -> Result<(), TwilioError> {
    if !has_non_empty(value) {
        return Err(TwilioError::InvalidRequest(format!(
            "{name} must not be empty"
        )));
    }
    Ok(())
}

macro_rules! tollfree_common_field_setters {
    () => {
        #[must_use]
        pub fn business_name(mut self, value: &'a str) -> Self {
            self.fields.business_name = Some(value);
            self
        }

        #[must_use]
        pub fn business_website(mut self, value: &'a str) -> Self {
            self.fields.business_website = Some(value);
            self
        }

        #[must_use]
        pub fn notification_email(mut self, value: &'a str) -> Self {
            self.fields.notification_email = Some(value);
            self
        }

        #[must_use]
        pub fn use_case_categories(mut self, value: &'a [TollfreeUseCaseCategory<'a>]) -> Self {
            self.fields.use_case_categories = value;
            self
        }

        #[must_use]
        pub fn use_case_summary(mut self, value: &'a str) -> Self {
            self.fields.use_case_summary = Some(value);
            self
        }

        #[must_use]
        pub fn production_message_sample(mut self, value: &'a str) -> Self {
            self.fields.production_message_sample = Some(value);
            self
        }

        #[must_use]
        pub fn opt_in_image_urls(mut self, value: &'a [&'a str]) -> Self {
            self.fields.opt_in_image_urls = value;
            self
        }

        #[must_use]
        pub fn opt_in_type(mut self, value: TollfreeOptInType<'a>) -> Self {
            self.fields.opt_in_type = Some(value);
            self
        }

        #[must_use]
        pub fn message_volume(mut self, value: TollfreeMessageVolume<'a>) -> Self {
            self.fields.message_volume = Some(value);
            self
        }

        #[must_use]
        pub fn business_street_address(mut self, value: &'a str) -> Self {
            self.fields.business_street_address = Some(value);
            self
        }

        #[must_use]
        pub fn business_street_address2(mut self, value: &'a str) -> Self {
            self.fields.business_street_address2 = Some(value);
            self
        }

        #[must_use]
        pub fn business_city(mut self, value: &'a str) -> Self {
            self.fields.business_city = Some(value);
            self
        }

        #[must_use]
        pub fn business_state_province_region(mut self, value: &'a str) -> Self {
            self.fields.business_state_province_region = Some(value);
            self
        }

        #[must_use]
        pub fn business_postal_code(mut self, value: &'a str) -> Self {
            self.fields.business_postal_code = Some(value);
            self
        }

        #[must_use]
        pub fn business_country(mut self, value: &'a str) -> Self {
            self.fields.business_country = Some(value);
            self
        }

        #[must_use]
        pub fn additional_information(mut self, value: &'a str) -> Self {
            self.fields.additional_information = Some(value);
            self
        }

        #[must_use]
        pub fn business_contact_first_name(mut self, value: &'a str) -> Self {
            self.fields.business_contact_first_name = Some(value);
            self
        }

        #[must_use]
        pub fn business_contact_last_name(mut self, value: &'a str) -> Self {
            self.fields.business_contact_last_name = Some(value);
            self
        }

        #[must_use]
        pub fn business_contact_email(mut self, value: &'a str) -> Self {
            self.fields.business_contact_email = Some(value);
            self
        }

        #[must_use]
        pub fn business_contact_phone(mut self, value: &'a str) -> Self {
            self.fields.business_contact_phone = Some(value);
            self
        }

        #[must_use]
        pub fn business_registration_number(mut self, value: &'a str) -> Self {
            self.fields.business_registration_number = Some(value);
            self
        }

        #[must_use]
        pub fn business_registration_authority(
            mut self,
            value: TollfreeBusinessRegistrationAuthority<'a>,
        ) -> Self {
            self.fields.business_registration_authority = Some(value);
            self
        }

        #[must_use]
        pub fn business_registration_country(mut self, value: &'a str) -> Self {
            self.fields.business_registration_country = Some(value);
            self
        }

        #[must_use]
        pub fn business_type(mut self, value: TollfreeBusinessType<'a>) -> Self {
            self.fields.business_type = Some(value);
            self
        }

        #[must_use]
        pub fn business_registration_phone_number(mut self, value: &'a str) -> Self {
            self.fields.business_registration_phone_number = Some(value);
            self
        }

        #[must_use]
        pub fn doing_business_as(mut self, value: &'a str) -> Self {
            self.fields.doing_business_as = Some(value);
            self
        }

        #[must_use]
        pub fn opt_in_confirmation_message(mut self, value: &'a str) -> Self {
            self.fields.opt_in_confirmation_message = Some(value);
            self
        }

        #[must_use]
        pub fn help_message_sample(mut self, value: &'a str) -> Self {
            self.fields.help_message_sample = Some(value);
            self
        }

        #[must_use]
        pub fn privacy_policy_url(mut self, value: &'a str) -> Self {
            self.fields.privacy_policy_url = Some(value);
            self
        }

        #[must_use]
        pub fn terms_and_conditions_url(mut self, value: &'a str) -> Self {
            self.fields.terms_and_conditions_url = Some(value);
            self
        }

        #[must_use]
        pub fn age_gated_content(mut self, value: bool) -> Self {
            self.fields.age_gated_content = Some(value);
            self
        }

        #[must_use]
        pub fn opt_in_keywords(mut self, value: &'a [&'a str]) -> Self {
            self.fields.opt_in_keywords = value;
            self
        }

        #[must_use]
        pub fn vetting_provider(mut self, value: TollfreeVettingProvider<'a>) -> Self {
            self.fields.vetting_provider = Some(value);
            self
        }

        #[must_use]
        pub fn vetting_id(mut self, value: &'a str) -> Self {
            self.fields.vetting_id = Some(value);
            self
        }
    };
}

/// Request body for `POST /Tollfree/Verifications`.
#[derive(Clone, Copy, Default)]
pub struct CreateTollfreeVerificationRequest<'a> {
    fields: TollfreeVerificationFields<'a>,
}

impl<'a> CreateTollfreeVerificationRequest<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    tollfree_common_field_setters!();

    #[must_use]
    pub fn tollfree_phone_number_sid(mut self, value: &'a str) -> Self {
        self.fields.tollfree_phone_number_sid = Some(value);
        self
    }

    #[must_use]
    pub fn customer_profile_sid(mut self, value: &'a str) -> Self {
        self.fields.customer_profile_sid = Some(value);
        self
    }

    #[must_use]
    pub fn external_reference_id(mut self, value: &'a str) -> Self {
        self.fields.external_reference_id = Some(value);
        self
    }

    fn validate(self) -> Result<(), TwilioError> {
        validate_required("BusinessName", self.fields.business_name)?;
        validate_required("BusinessWebsite", self.fields.business_website)?;
        validate_required("NotificationEmail", self.fields.notification_email)?;
        if self.fields.use_case_categories.is_empty() {
            return Err(TwilioError::InvalidRequest(
                "UseCaseCategories must not be empty".to_owned(),
            ));
        }
        validate_required("UseCaseSummary", self.fields.use_case_summary)?;
        validate_required(
            "ProductionMessageSample",
            self.fields.production_message_sample,
        )?;
        if self.fields.opt_in_image_urls.is_empty() {
            return Err(TwilioError::InvalidRequest(
                "OptInImageUrls must not be empty".to_owned(),
            ));
        }
        validate_optional_enum("OptInType", self.fields.opt_in_type)?;
        if self.fields.opt_in_type.is_none() {
            return Err(TwilioError::InvalidRequest(
                "OptInType must not be empty".to_owned(),
            ));
        }
        validate_optional_enum("MessageVolume", self.fields.message_volume)?;
        if self.fields.message_volume.is_none() {
            return Err(TwilioError::InvalidRequest(
                "MessageVolume must not be empty".to_owned(),
            ));
        }
        validate_required(
            "TollfreePhoneNumberSid",
            self.fields.tollfree_phone_number_sid,
        )?;
        self.fields.validate_common()
    }

    fn form_params(self) -> Vec<FormParam> {
        self.fields.form_params()
    }

    fn sensitive_values(self, creds: TwilioCreds<'a>) -> Vec<&'a str> {
        self.fields.sensitive_values(creds, None)
    }
}

/// Request body for `POST /Tollfree/Verifications/{Sid}`.
#[derive(Clone, Copy, Default)]
pub struct UpdateTollfreeVerificationRequest<'a> {
    fields: TollfreeVerificationFields<'a>,
}

impl<'a> UpdateTollfreeVerificationRequest<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    tollfree_common_field_setters!();

    #[must_use]
    pub fn edit_reason(mut self, value: &'a str) -> Self {
        self.fields.edit_reason = Some(value);
        self
    }

    fn validate(self) -> Result<(), TwilioError> {
        if self.fields.form_params().is_empty() {
            return Err(TwilioError::InvalidRequest(
                "toll-free verification update requires at least one field".to_owned(),
            ));
        }
        self.fields.validate_common()
    }

    fn form_params(self) -> Vec<FormParam> {
        self.fields.form_params()
    }

    fn sensitive_values(self, creds: TwilioCreds<'a>, sid: &'a str) -> Vec<&'a str> {
        self.fields.sensitive_values(creds, Some(sid))
    }
}

/// Query parameters for `GET /Tollfree/Verifications`.
#[derive(Clone, Copy, Default)]
pub struct ListTollfreeVerificationsRequest<'a> {
    tollfree_phone_number_sid: Option<&'a str>,
    status: Option<TollfreeVerificationStatus<'a>>,
    external_reference_id: Option<&'a str>,
    include_sub_accounts: Option<bool>,
    trust_product_sids: &'a [&'a str],
    page_size: Option<u32>,
    page: Option<u32>,
    page_token: Option<&'a str>,
}

impl<'a> ListTollfreeVerificationsRequest<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn tollfree_phone_number_sid(mut self, value: &'a str) -> Self {
        self.tollfree_phone_number_sid = Some(value);
        self
    }

    #[must_use]
    pub fn status(mut self, value: TollfreeVerificationStatus<'a>) -> Self {
        self.status = Some(value);
        self
    }

    #[must_use]
    pub fn external_reference_id(mut self, value: &'a str) -> Self {
        self.external_reference_id = Some(value);
        self
    }

    #[must_use]
    pub fn include_sub_accounts(mut self, value: bool) -> Self {
        self.include_sub_accounts = Some(value);
        self
    }

    #[must_use]
    pub fn trust_product_sids(mut self, value: &'a [&'a str]) -> Self {
        self.trust_product_sids = value;
        self
    }

    #[must_use]
    pub fn page_size(mut self, value: u32) -> Self {
        self.page_size = Some(value);
        self
    }

    #[must_use]
    pub fn page(mut self, value: u32) -> Self {
        self.page = Some(value);
        self
    }

    #[must_use]
    pub fn page_token(mut self, value: &'a str) -> Self {
        self.page_token = Some(value);
        self
    }

    fn validate(self) -> Result<(), TwilioError> {
        validate_page_size(self.page_size)?;
        validate_optional_enum("Status", self.status)?;
        validate_str_slice("TrustProductSid", self.trust_product_sids)
    }

    fn apply_query(self, url: &mut Url) {
        let mut query = url.query_pairs_mut();
        if let Some(value) = self.tollfree_phone_number_sid {
            query.append_pair("TollfreePhoneNumberSid", value);
        }
        if let Some(value) = self.status {
            query.append_pair("Status", value.form_value());
        }
        if let Some(value) = self.external_reference_id {
            query.append_pair("ExternalReferenceId", value);
        }
        if let Some(value) = self.include_sub_accounts {
            query.append_pair("IncludeSubAccounts", &value.to_string());
        }
        for value in self.trust_product_sids {
            query.append_pair("TrustProductSid", value);
        }
        if let Some(value) = self.page_size {
            query.append_pair("PageSize", &value.to_string());
        }
        if let Some(value) = self.page {
            query.append_pair("Page", &value.to_string());
        }
        if let Some(value) = self.page_token {
            query.append_pair("PageToken", value);
        }
    }

    fn sensitive_values(self, creds: TwilioCreds<'a>) -> Vec<&'a str> {
        let mut values = vec![creds.account_sid, creds.auth_token];
        push_sensitive(&mut values, self.tollfree_phone_number_sid);
        push_sensitive(&mut values, self.external_reference_id);
        values.extend(self.trust_product_sids.iter().copied());
        push_sensitive(&mut values, self.page_token);
        values
    }
}

/// A Messaging v1 Toll-free Verification.
#[derive(Clone)]
pub struct TwilioTollfreeVerification {
    pub sid: Option<String>,
    pub account_sid: Option<String>,
    pub customer_profile_sid: Option<String>,
    pub trust_product_sid: Option<String>,
    pub regulated_item_sid: Option<String>,
    pub date_created: Option<OffsetDateTime>,
    pub date_updated: Option<OffsetDateTime>,
    pub business_name: Option<String>,
    pub business_street_address: Option<String>,
    pub business_street_address2: Option<String>,
    pub business_city: Option<String>,
    pub business_state_province_region: Option<String>,
    pub business_postal_code: Option<String>,
    pub business_country: Option<String>,
    pub business_website: Option<String>,
    pub business_contact_first_name: Option<String>,
    pub business_contact_last_name: Option<String>,
    pub business_contact_email: Option<String>,
    pub business_contact_phone: Option<String>,
    pub notification_email: Option<String>,
    pub use_case_categories: Option<Vec<String>>,
    pub use_case_summary: Option<String>,
    pub production_message_sample: Option<String>,
    pub opt_in_image_urls: Option<Vec<String>>,
    pub opt_in_type: Option<String>,
    pub message_volume: Option<String>,
    pub additional_information: Option<String>,
    pub tollfree_phone_number_sid: Option<String>,
    pub tollfree_phone_number: Option<String>,
    pub status: Option<String>,
    pub rejection_reason: Option<String>,
    pub error_code: Option<i64>,
    pub edit_expiration: Option<OffsetDateTime>,
    pub edit_allowed: Option<bool>,
    pub rejection_reasons: Option<serde_json::Value>,
    pub resource_links: Option<BTreeMap<String, String>>,
    pub url: Option<String>,
    pub external_reference_id: Option<String>,
    pub business_registration_number: Option<String>,
    pub business_registration_authority: Option<String>,
    pub business_registration_country: Option<String>,
    pub business_type: Option<String>,
    pub business_registration_phone_number: Option<String>,
    pub doing_business_as: Option<String>,
    pub age_gated_content: Option<bool>,
    pub help_message_sample: Option<String>,
    pub opt_in_confirmation_message: Option<String>,
    pub opt_in_keywords: Option<Vec<String>>,
    pub privacy_policy_url: Option<String>,
    pub terms_and_conditions_url: Option<String>,
    pub vetting_id: Option<String>,
    pub vetting_id_expiration: Option<OffsetDateTime>,
    pub vetting_provider: Option<String>,
}

#[allow(
    clippy::too_many_lines,
    reason = "TFV responses have many fields and Debug must redact them explicitly."
)]
impl std::fmt::Debug for TwilioTollfreeVerification {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwilioTollfreeVerification")
            .field("sid", &redacted_option(&self.sid))
            .field("account_sid", &redacted_option(&self.account_sid))
            .field(
                "customer_profile_sid",
                &redacted_option(&self.customer_profile_sid),
            )
            .field(
                "trust_product_sid",
                &redacted_option(&self.trust_product_sid),
            )
            .field(
                "regulated_item_sid",
                &redacted_option(&self.regulated_item_sid),
            )
            .field("date_created", &self.date_created)
            .field("date_updated", &self.date_updated)
            .field("business_name", &redacted_option(&self.business_name))
            .field(
                "business_street_address",
                &redacted_option(&self.business_street_address),
            )
            .field(
                "business_street_address2",
                &redacted_option(&self.business_street_address2),
            )
            .field("business_city", &redacted_option(&self.business_city))
            .field(
                "business_state_province_region",
                &redacted_option(&self.business_state_province_region),
            )
            .field(
                "business_postal_code",
                &redacted_option(&self.business_postal_code),
            )
            .field("business_country", &redacted_option(&self.business_country))
            .field("business_website", &redacted_option(&self.business_website))
            .field(
                "business_contact_first_name",
                &redacted_option(&self.business_contact_first_name),
            )
            .field(
                "business_contact_last_name",
                &redacted_option(&self.business_contact_last_name),
            )
            .field(
                "business_contact_email",
                &redacted_option(&self.business_contact_email),
            )
            .field(
                "business_contact_phone",
                &redacted_option(&self.business_contact_phone),
            )
            .field(
                "notification_email",
                &redacted_option(&self.notification_email),
            )
            .field(
                "use_case_categories",
                &self
                    .use_case_categories
                    .as_ref()
                    .map(|_| crate::common::REDACTED),
            )
            .field("use_case_summary", &redacted_option(&self.use_case_summary))
            .field(
                "production_message_sample",
                &redacted_option(&self.production_message_sample),
            )
            .field(
                "opt_in_image_urls",
                &self
                    .opt_in_image_urls
                    .as_ref()
                    .map(|_| crate::common::REDACTED),
            )
            .field("opt_in_type", &self.opt_in_type)
            .field("message_volume", &self.message_volume)
            .field(
                "additional_information",
                &redacted_option(&self.additional_information),
            )
            .field(
                "tollfree_phone_number_sid",
                &redacted_option(&self.tollfree_phone_number_sid),
            )
            .field(
                "tollfree_phone_number",
                &redacted_option(&self.tollfree_phone_number),
            )
            .field("status", &self.status)
            .field("rejection_reason", &redacted_option(&self.rejection_reason))
            .field("error_code", &self.error_code)
            .field("edit_expiration", &self.edit_expiration)
            .field("edit_allowed", &self.edit_allowed)
            .field(
                "rejection_reasons",
                &self
                    .rejection_reasons
                    .as_ref()
                    .map(|_| crate::common::REDACTED),
            )
            .field(
                "resource_links",
                &self
                    .resource_links
                    .as_ref()
                    .map(|_| crate::common::REDACTED),
            )
            .field("url", &redacted_option(&self.url))
            .field(
                "external_reference_id",
                &redacted_option(&self.external_reference_id),
            )
            .field(
                "business_registration_number",
                &redacted_option(&self.business_registration_number),
            )
            .field(
                "business_registration_authority",
                &self.business_registration_authority,
            )
            .field(
                "business_registration_country",
                &redacted_option(&self.business_registration_country),
            )
            .field("business_type", &self.business_type)
            .field(
                "business_registration_phone_number",
                &redacted_option(&self.business_registration_phone_number),
            )
            .field(
                "doing_business_as",
                &redacted_option(&self.doing_business_as),
            )
            .field("age_gated_content", &self.age_gated_content)
            .field(
                "help_message_sample",
                &redacted_option(&self.help_message_sample),
            )
            .field(
                "opt_in_confirmation_message",
                &redacted_option(&self.opt_in_confirmation_message),
            )
            .field(
                "opt_in_keywords",
                &self
                    .opt_in_keywords
                    .as_ref()
                    .map(|_| crate::common::REDACTED),
            )
            .field(
                "privacy_policy_url",
                &redacted_option(&self.privacy_policy_url),
            )
            .field(
                "terms_and_conditions_url",
                &redacted_option(&self.terms_and_conditions_url),
            )
            .field("vetting_id", &redacted_option(&self.vetting_id))
            .field("vetting_id_expiration", &self.vetting_id_expiration)
            .field("vetting_provider", &self.vetting_provider)
            .finish()
    }
}

/// One page of Toll-free Verifications.
#[derive(Clone, Debug)]
pub struct TwilioTollfreeVerificationPage {
    pub tollfree_verifications: Vec<TwilioTollfreeVerification>,
    pub meta: V1PageMeta,
}

#[derive(Deserialize)]
struct WireTollfreeVerification {
    sid: Option<String>,
    account_sid: Option<String>,
    customer_profile_sid: Option<String>,
    trust_product_sid: Option<String>,
    regulated_item_sid: Option<String>,
    date_created: Option<String>,
    date_updated: Option<String>,
    business_name: Option<String>,
    business_street_address: Option<String>,
    business_street_address2: Option<String>,
    business_city: Option<String>,
    business_state_province_region: Option<String>,
    business_postal_code: Option<String>,
    business_country: Option<String>,
    business_website: Option<String>,
    business_contact_first_name: Option<String>,
    business_contact_last_name: Option<String>,
    business_contact_email: Option<String>,
    business_contact_phone: Option<String>,
    notification_email: Option<String>,
    use_case_categories: Option<Vec<String>>,
    use_case_summary: Option<String>,
    production_message_sample: Option<String>,
    opt_in_image_urls: Option<Vec<String>>,
    opt_in_type: Option<String>,
    message_volume: Option<String>,
    additional_information: Option<String>,
    tollfree_phone_number_sid: Option<String>,
    tollfree_phone_number: Option<String>,
    status: Option<String>,
    rejection_reason: Option<String>,
    error_code: Option<i64>,
    edit_expiration: Option<String>,
    edit_allowed: Option<bool>,
    rejection_reasons: Option<serde_json::Value>,
    resource_links: Option<BTreeMap<String, String>>,
    url: Option<String>,
    external_reference_id: Option<String>,
    business_registration_number: Option<String>,
    business_registration_authority: Option<String>,
    business_registration_country: Option<String>,
    business_type: Option<String>,
    business_registration_phone_number: Option<String>,
    doing_business_as: Option<String>,
    age_gated_content: Option<bool>,
    help_message_sample: Option<String>,
    opt_in_confirmation_message: Option<String>,
    opt_in_keywords: Option<Vec<String>>,
    privacy_policy_url: Option<String>,
    terms_and_conditions_url: Option<String>,
    vetting_id: Option<String>,
    vetting_id_expiration: Option<String>,
    vetting_provider: Option<String>,
}

impl WireTollfreeVerification {
    fn into_verification(self) -> TwilioTollfreeVerification {
        TwilioTollfreeVerification {
            sid: self.sid,
            account_sid: self.account_sid,
            customer_profile_sid: self.customer_profile_sid,
            trust_product_sid: self.trust_product_sid,
            regulated_item_sid: self.regulated_item_sid,
            date_created: parse_iso8601(self.date_created),
            date_updated: parse_iso8601(self.date_updated),
            business_name: self.business_name,
            business_street_address: self.business_street_address,
            business_street_address2: self.business_street_address2,
            business_city: self.business_city,
            business_state_province_region: self.business_state_province_region,
            business_postal_code: self.business_postal_code,
            business_country: self.business_country,
            business_website: self.business_website,
            business_contact_first_name: self.business_contact_first_name,
            business_contact_last_name: self.business_contact_last_name,
            business_contact_email: self.business_contact_email,
            business_contact_phone: self.business_contact_phone,
            notification_email: self.notification_email,
            use_case_categories: self.use_case_categories,
            use_case_summary: self.use_case_summary,
            production_message_sample: self.production_message_sample,
            opt_in_image_urls: self.opt_in_image_urls,
            opt_in_type: self.opt_in_type,
            message_volume: self.message_volume,
            additional_information: self.additional_information,
            tollfree_phone_number_sid: self.tollfree_phone_number_sid,
            tollfree_phone_number: self.tollfree_phone_number,
            status: self.status,
            rejection_reason: self.rejection_reason,
            error_code: self.error_code,
            edit_expiration: parse_iso8601(self.edit_expiration),
            edit_allowed: self.edit_allowed,
            rejection_reasons: self.rejection_reasons,
            resource_links: self.resource_links,
            url: self.url,
            external_reference_id: self.external_reference_id,
            business_registration_number: self.business_registration_number,
            business_registration_authority: self.business_registration_authority,
            business_registration_country: self.business_registration_country,
            business_type: self.business_type,
            business_registration_phone_number: self.business_registration_phone_number,
            doing_business_as: self.doing_business_as,
            age_gated_content: self.age_gated_content,
            help_message_sample: self.help_message_sample,
            opt_in_confirmation_message: self.opt_in_confirmation_message,
            opt_in_keywords: self.opt_in_keywords,
            privacy_policy_url: self.privacy_policy_url,
            terms_and_conditions_url: self.terms_and_conditions_url,
            vetting_id: self.vetting_id,
            vetting_id_expiration: parse_iso8601(self.vetting_id_expiration),
            vetting_provider: self.vetting_provider,
        }
    }
}

#[derive(Deserialize)]
struct WireTollfreeVerificationPage {
    #[serde(default)]
    verifications: Vec<WireTollfreeVerification>,
    #[serde(default)]
    meta: WireV1PageMeta,
}

impl WireTollfreeVerificationPage {
    fn into_page(self) -> TwilioTollfreeVerificationPage {
        TwilioTollfreeVerificationPage {
            tollfree_verifications: self
                .verifications
                .into_iter()
                .map(WireTollfreeVerification::into_verification)
                .collect(),
            meta: self.meta.into_meta(),
        }
    }
}

/// Messaging v1 Toll-free Verifications collection.
#[derive(Clone, Copy)]
pub struct TollfreeVerificationsResource<'a> {
    account: TwilioAccount<'a>,
}

impl<'a> TollfreeVerificationsResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>) -> Self {
        Self { account }
    }

    /// `POST /Tollfree/Verifications`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub async fn create(
        self,
        request: CreateTollfreeVerificationRequest<'a>,
    ) -> Result<TwilioTollfreeVerification, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let spec = RequestSpec::new(
                ApiFamily::Messaging,
                Method::POST,
                ["Tollfree", "Verifications"],
            )
            .operation("tollfree_verifications.create")
            .form_params(request.form_params());
            let parsed: WireTollfreeVerification =
                self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(parsed.into_verification())
        }
        .instrument(request_span(
            &self.account.client.config.messaging_base_url,
            "tollfree_verifications.create",
            "POST",
        ))
        .await
    }

    /// `GET /Tollfree/Verifications`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, malformed JSON responses, or invalid pagination
    /// metadata.
    pub async fn list(
        self,
        request: ListTollfreeVerificationsRequest<'a>,
    ) -> Result<TwilioTollfreeVerificationPage, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds);
            let mut url = self.collection_url()?;
            request.apply_query(&mut url);
            let spec = RequestSpec::from_url(
                ApiFamily::Messaging,
                Method::GET,
                url.clone(),
                "tollfree_verifications.list",
            );
            let raw = self.account.send_spec_raw(spec, &sensitive_values).await?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        }
        .instrument(request_span(
            &self.account.client.config.messaging_base_url,
            "tollfree_verifications.list",
            "GET",
        ))
        .await
    }

    /// Fetch a subsequent page by Twilio's `meta.next_page_url`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] when the URL is invalid, leaves the configured
    /// Messaging API base, changes stable filters, or the HTTP request/response
    /// fails.
    pub async fn list_page_url(
        self,
        next_page_url: &str,
    ) -> Result<TwilioTollfreeVerificationPage, TwilioError> {
        async move {
            let sensitive_values = vec![
                self.account.creds.account_sid,
                self.account.creds.auth_token,
                next_page_url,
            ];
            let resource = V1PageResource::TollfreeVerifications;
            let url = self.account.client.v1_page_url(next_page_url, resource)?;
            let spec = RequestSpec::from_url(
                ApiFamily::Messaging,
                Method::GET,
                url.clone(),
                "tollfree_verifications.list_page_url",
            );
            let raw = self.account.send_spec_raw(spec, &sensitive_values).await?;
            self.read_page(&raw.output, &sensitive_values, Some(&url))
        }
        .instrument(request_span(
            &self.account.client.config.messaging_base_url,
            "tollfree_verifications.list_page_url",
            "GET",
        ))
        .await
    }

    fn read_page(
        self,
        raw: &crate::RawResponse,
        sensitive_values: &[&str],
        current_url: Option<&Url>,
    ) -> Result<TwilioTollfreeVerificationPage, TwilioError> {
        let parsed: WireTollfreeVerificationPage = decode_json_response(raw, sensitive_values)?;
        let page = parsed.into_page();
        let resource = V1PageResource::TollfreeVerifications;
        validate_v1_meta_key(&page.meta, resource)?;
        validate_next_v1_url(
            self.account,
            page.meta.next_page_url.as_deref(),
            resource,
            current_url,
        )?;
        Ok(page)
    }

    /// Lazily list all Toll-free Verifications using a default page size of 50.
    #[must_use]
    pub fn list_all(
        self,
    ) -> TwilioPaginator<'a, TwilioTollfreeVerificationPage, TwilioTollfreeVerification> {
        self.list_all_with(ListTollfreeVerificationsRequest::new())
    }

    /// Lazily list all Toll-free Verifications using supplied first-page filters.
    #[must_use]
    pub fn list_all_with(
        self,
        mut request: ListTollfreeVerificationsRequest<'a>,
    ) -> TwilioPaginator<'a, TwilioTollfreeVerificationPage, TwilioTollfreeVerification> {
        if request.page_size.is_none() {
            request.page_size = Some(DEFAULT_PAGE_SIZE);
        }
        TwilioPaginator::new(
            move |cursor| {
                let resource = self;
                Box::pin(async move {
                    if let Some(cursor) = cursor {
                        resource.list_page_url(&cursor).await
                    } else {
                        resource.list(request).await
                    }
                }) as PageFuture<'a, TwilioTollfreeVerificationPage>
            },
            split_tollfree_verification_page,
        )
    }

    fn collection_url(self) -> Result<Url, TwilioError> {
        self.account
            .client
            .messaging_endpoint(&["Tollfree", "Verifications"])
    }
}

fn split_tollfree_verification_page(
    page: TwilioTollfreeVerificationPage,
) -> (Vec<TwilioTollfreeVerification>, Option<String>) {
    (page.tollfree_verifications, page.meta.next_page_url)
}

/// One Messaging v1 Toll-free Verification resource.
#[derive(Clone, Copy)]
pub struct TollfreeVerificationResource<'a> {
    account: TwilioAccount<'a>,
    sid: &'a str,
}

impl<'a> TollfreeVerificationResource<'a> {
    pub(crate) fn new(account: TwilioAccount<'a>, sid: &'a str) -> Self {
        Self { account, sid }
    }

    /// `GET /Tollfree/Verifications/{Sid}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures, non-2xx API responses,
    /// or malformed JSON responses.
    pub async fn fetch(self) -> Result<TwilioTollfreeVerification, TwilioError> {
        async move {
            let sensitive_values = self.sensitive_values();
            let spec = self.verification_spec(Method::GET, "tollfree_verification.fetch")?;
            let parsed: WireTollfreeVerification =
                self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(parsed.into_verification())
        }
        .instrument(request_span(
            &self.account.client.config.messaging_base_url,
            "tollfree_verification.fetch",
            "GET",
        ))
        .await
    }

    /// `POST /Tollfree/Verifications/{Sid}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for invalid requests, transport failures,
    /// non-2xx API responses, or malformed JSON responses.
    pub async fn update(
        self,
        request: UpdateTollfreeVerificationRequest<'a>,
    ) -> Result<TwilioTollfreeVerification, TwilioError> {
        async move {
            request.validate()?;
            let sensitive_values = request.sensitive_values(self.account.creds, self.sid);
            let spec = self
                .verification_spec(Method::POST, "tollfree_verification.update")?
                .form_params(request.form_params());
            let parsed: WireTollfreeVerification =
                self.account.send_spec_json(spec, &sensitive_values).await?;
            Ok(parsed.into_verification())
        }
        .instrument(request_span(
            &self.account.client.config.messaging_base_url,
            "tollfree_verification.update",
            "POST",
        ))
        .await
    }

    /// `DELETE /Tollfree/Verifications/{Sid}`.
    ///
    /// # Errors
    ///
    /// Returns [`TwilioError`] for transport failures or non-2xx API responses.
    pub async fn delete(self) -> Result<(), TwilioError> {
        async move {
            let sensitive_values = self.sensitive_values();
            let spec = self.verification_spec(Method::DELETE, "tollfree_verification.delete")?;
            self.account.send_spec_empty(spec, &sensitive_values).await
        }
        .instrument(request_span(
            &self.account.client.config.messaging_base_url,
            "tollfree_verification.delete",
            "DELETE",
        ))
        .await
    }

    fn verification_url(self) -> Result<Url, TwilioError> {
        self.account
            .client
            .messaging_endpoint(&["Tollfree", "Verifications", self.sid])
    }

    fn verification_spec(
        self,
        method: Method,
        operation: &'static str,
    ) -> Result<RequestSpec, TwilioError> {
        Ok(RequestSpec::from_url(
            ApiFamily::Messaging,
            method,
            self.verification_url()?,
            operation,
        ))
    }

    fn sensitive_values(self) -> Vec<&'a str> {
        vec![
            self.account.creds.account_sid,
            self.account.creds.auth_token,
            self.sid,
        ]
    }
}

fn validate_next_v1_url(
    account: TwilioAccount<'_>,
    next_page_url: Option<&str>,
    resource: V1PageResource<'_>,
    current_url: Option<&Url>,
) -> Result<(), TwilioError> {
    if let Some(next_page_url) = next_page_url {
        let next_url = account.client.v1_page_url(next_page_url, resource)?;
        if let Some(current_url) = current_url {
            validate_v1_next_page_continuation(current_url, &next_url, resource)?;
        }
    }
    Ok(())
}
