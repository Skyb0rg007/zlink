# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.4.0 - 2026-02-22

### Added
- ✨ add support for ANY type.
- ✨ Support FDs in Service impls.
- ✨ Add Reply::map method.
- ✨ Add notified::State::stream_once.
- ✨ Add `service` attribute macro. #76

### Breaking
- 💥 Move `Sock` from `handle` to the trait in Service. #207

### Changed
- 🎨 Re-arrange attributes to satisfy rust-analyzer.
- 🏗️ Re-export pin-project-lite but keep it hidden.
- 🏗️ Provide traits for `notified` API.

### Other
- 🚩 Remove std requirement from introspection.
- 🚩 Feature-gate std-only introspect Type impls.
- 🚩 `service` feature now requires `introspection` feature.

### Removed
- 🔥 Drop notified::Once.

## 0.3.0 - 2026-01-12

### Added
- ✨ Allow creating chains from Iterator types. #168
- ✨ Internal MockSocket to handle pipelined messages.

### Breaking
- 💥 chain::ReplyStream's items now owned. #185

### Changed
- 🎨 varlink_service Owned types wrappers around borrowed siblings.
- 🏗️ Move reply types from Chain to send method.
- ♻️ Rename service handle lifetime to 'service.

### Documentation
- 📝 Don't hide ReplyStream from docs anymore.
- 📝 List forgotten `smol` feature.
- 📝 Drop now incorrect `no_std` claims.

### Fixed
- 🩹 Add some missing `cfg`s.
- 🐛 Add missing feature gate on a Deserialize impl.
- 🐛 Don't send reply for `oneway` methods.

### Other
- ✏️ fix docstring typos.
- 🦺 Box the Future type of the ReplyStream.
- ✏️ Fix a typo.
- 🚸 Add Chain::send_ignore_replies.
- ✏️  Add missing `.` at the end of sentences.

### Performance
- ⚡️ Reduce allocations.
- ⚡️ Allow service to return borrowed data from call.

### Removed
- 🔥 `proxy` only generate chain methods for owned types ret.
- 🔥 Remove a redundant referencing operation.

### Testing
- ✅ Rename 2 tests.
