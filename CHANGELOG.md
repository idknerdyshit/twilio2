# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

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
