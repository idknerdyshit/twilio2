# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.2.0] - 2026-07-02

### Changed

- Replace positional message create/list methods with borrowed request structs.
- Expand `TwilioMessage` and page response models to cover documented fields.

### Added

- Add Message update/redact/cancel and delete support.
- Add Message Media metadata, download, list, pagination, and delete support.
- Add Message Feedback creation support.
- Add light request validation and `TwilioError::InvalidRequest`.

## [0.1.1] - 2026-06-28

### Added

- Add sanitized Twilio request tracing (2da156a)
- Initial twilio2 crate (741b725)
