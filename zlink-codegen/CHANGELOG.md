# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.6.0 - 2026-07-11

### Added
- ✨ Add --rustfmt to zlink-codegen.
- ✨ Introduce generate_files() function.
- ✨ Introduce an error.rs.

### Changed
- 🔧 Replace hardcoded /tmp socket paths.
- 🏗️ ReplyError derive no longer requires specific field order.
- 🏗️ Drop support for `no_alloc`. #144

### Dependencies
- ⬆️ Update chrono to v0.4.42 (#107).

### Documentation
- 📝 Update docs as per the recent changes.
- 📝 Drop explicit lifetimes in Server example.
- 📝 Advertise the new Matrix channel.
- 📝 Redesign the top part, now that we have a nice logo.
- 📝 Add logo to the docs.

### Fixed
- 🐛 Fix the example varlink interface. #145
- 🐛 Add `rename` attributes to method parameters. #129
- 🐛 add `zlink` instead of `serde` attribute for ReplyError.

### Other
- 🚨 Fix clippy warnings from latest stable Rust.
- ♻ Refactor main.rs to use generate_files().
- 📄 Add license file to individual crates. #131

### Testing
- ✅ Update test-integration to use generate_files().
- 🧪 Add tests for camelCase params/fields.

## 0.5.0 - 2026-05-03

### Fixed
- 🐛 Make sure methods return objects in tests.

## 0.4.2 - 2026-04-26

### Documentation
- 📝 Configure docs.rs to build for all supported targets.

## 0.4.1 - 2026-03-28

### Other
- Updated the following local packages: zlink.

## 0.4.0 - 2026-02-22

### Added
- ✨ add support for ANY type.
- ✨ Support FDs in Service impls.

### Breaking
- 💥 Move `Sock` from `handle` to the trait in Service. #207

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
