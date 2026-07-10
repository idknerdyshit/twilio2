# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## Unreleased

## [0.4.0] - 2026-07-10

### Changed

- **Breaking:** Messaging and Pricing custom base URLs are now product roots
  without `/v1`, `/v2`, or `/v3`; request paths include the API version.
- **Breaking:** Messaging and Pricing account resources now use Twilio-style
  product/version namespaces such as `account.messaging().v1().services()` and
  `account.pricing().v2().voice()`. Direct v1 compatibility aliases were
  removed.
- **Breaking:** Remove the obsolete Pricing v1 Voice Number resource; use the
  documented Pricing v2 Voice Number resource instead.
- **Breaking:** Replace implicit caller-provided async transport constructors
  with `from_config_with_http_builder`, which preserves HTTPS-only and
  no-redirect policies after customization.

### Added

- Add remaining published Programmable Messaging endpoint families for Link
  Shortening, v1 service helpers, A2P SMS OTP retry, v2 channel senders, v2/v3
  typing indicators, Accounts Messaging GeoPermissions, and Pricing v1/v2 gaps.
- Add JSON request body support for typed operations that require it.

### Fixed

- Disable redirects and require HTTPS for clients constructed by the crate;
  document the unchecked injected-client escape hatch.
- Preserve RCS typing events, reject WhatsApp sender creation without a profile
  name, and add missing blocking wire-contract coverage.

## [0.3.1] - 2026-07-08

### Changed

- Refactor Twilio client message and pagination handling. (cab3c1d)
- Refactor Twilio client pagination and message handling. (03093c0)
- Ignore nested DS_Store files. (f4e089c)

## [0.3.0] - 2026-07-05

### Changed

- **Breaking:** `TwilioCreds` now owns redacted, zero-on-drop credential
  buffers. Construct credentials with `TwilioCreds::new(...)` and pass
  `&creds` to `client.account(...)` / `blocking_client.account(...)`.
  (f283ab3)
- Refactor Twilio client request validation and tests. (01161b7)

### Added

- Add a public `Secret<T>` wrapper for redacted, zero-on-drop sensitive values.
  (f283ab3)
- Add blocking sync Twilio API. (0aaf518)
- Add public contract coverage for new endpoints. (f1596a5)
- Add deactivations, short codes, and toll-free verifications. (a69435a)

## [0.2.0] - 2026-07-03

### Changed

- Replace flat `TwilioClient` resource methods with account/resource builders.
- Replace single base URL construction with `TwilioConfig` containing REST and
  Messaging v1 base URLs.
- Move Messages calls to `client.account(creds).messages()` and
  `client.account(creds).message(sid)` resource handles.
- Replace positional message create/list methods with borrowed request structs.
- Expand `TwilioMessage` and page response models to cover documented fields.

### Added

- Add Messaging Services create/fetch/list/page/update/delete support.
- Add Service PhoneNumbers, ShortCodes, AlphaSenders, ChannelSenders, and
  DestinationAlphaSenders create/fetch/list/page/delete support.
- Add Messaging v1 page metadata handling with `V1PageMeta`.
- Add strict Messaging v1 `next_page_url` validation for origin, base path,
  resource path, query keys, duplicate keys, and stable filters.
- Add Message update/redact/cancel and delete support.
- Add Message Media metadata, download, list, pagination, and delete support.
- Add Message Feedback creation support.
- Add light request validation and `TwilioError::InvalidRequest`.

## [0.1.1] - 2026-06-28

### Added

- Add sanitized Twilio request tracing (2da156a)
- Initial twilio2 crate (741b725)
