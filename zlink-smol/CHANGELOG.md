# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.4.0 - 2026-02-22

### Added
- ✨ Support FDs in Service impls.
- ✨ Add notified::State::stream_once.
- ✨ Split stream types for `notified`. #86
- ✨ notified::Stream now handles notified::State drop.
- ✨ Add `service` attribute macro. #76

### Breaking
- 💥 Move `Sock` from `handle` to the trait in Service. #207

### Changed
- 🏗️ Provide traits for `notified` API.
- ♻️ Split `notified` module into a hierarchy.
- 🏗️ Use pin-project-lite to consolidate notified.

### Documentation
- 📝 Document `service` feature.
- 📝 Use & recommend `service` macro.
- 💡 Add missing `.` in sample code comments.

### Other
- 🚩 Remove std requirement from introspection.
- 🚩 `notified` now gated behind `server` feature.
- 🚩 `service` feature now requires `introspection` feature.

### Removed
- 🔥 Drop notified::Once.

## 0.3.0 - 2026-01-12

### Breaking
- 💥 chain::ReplyStream's items now owned. #185

### Changed
- 🏗️ Move reply types from Chain to send method.
- ♻️ Rename service handle lifetime to 'service.

### Documentation
- 📝 List forgotten `smol` feature.
- 📝 Drop now incorrect `no_std` claims.

### Other
- ✏️ Fix a typo.
- ✏️  Add missing `.` at the end of sentences.

### Performance
- ⚡️ Allow service to return borrowed data from call.

### Removed
- 🔥 `proxy` only generate chain methods for owned types ret.
