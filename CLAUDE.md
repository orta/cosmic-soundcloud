# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

This project uses [just](https://github.com/casey/just) as the command runner:

- `just` - Build release (default)
- `just run` - Build and run with `RUST_BACKTRACE=full`
- `just check` - Run clippy with pedantic warnings (`-W clippy::pedantic`)
- `just check-json` - Clippy with JSON output for IDE integration
- `just build-debug` - Debug build
- `just install` - Install to system (uses `rootdir` and `prefix` variables)
- `just clean` - Clean build artifacts

## Architecture

This is a COSMIC desktop application built with libcosmic (Iced-based GUI framework for Pop!_OS/COSMIC desktop).

### Core Structure

- **`src/main.rs`** - Entry point: initializes i18n, configures window settings, runs the cosmic app event loop
- **`src/app.rs`** - Main application model implementing `cosmic::Application` trait with:
  - `AppModel` struct holding application state (nav bar, config, context pages)
  - `Message` enum for all application events
  - `view()` renders UI based on current nav page
  - `update()` handles message dispatch
  - `subscription()` manages async background tasks (config watching, timers)
- **`src/config.rs`** - Persistent configuration using `cosmic_config` with versioned schema
- **`src/i18n.rs`** - Localization setup using Fluent; exports `fl!()` macro for translations

### Key Patterns

- Navigation uses `nav_bar::Model` with `Page` enum variants
- Context drawer pattern for side panels (e.g., About page)
- Config changes watched via subscription and auto-applied
- Menu actions map to messages via `MenuAction` enum implementing `menu::action::MenuAction`

### Localization

Translations in `i18n/{lang_code}/cosmic_soundcloud.ftl` using Fluent format. Use `fl!("message-id")` or `fl!("message-id", arg = value)` for parameterized messages.

### App ID

The application ID is `com.github.orta.cosmic-soundcloud` (defined in justfile and `APP_ID` constant in app.rs).
