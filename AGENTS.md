# RimMod Agent Guide

## Project purpose

RimMod is a desktop RimWorld mod manager written in Rust with egui and eframe. The project has two equally important goals:

1. Build a useful, maintainable mod manager.
2. Help the project owner learn Rust by understanding the code they write.

Do not optimize only for delivering features. Optimize for code that a Rust beginner can read, explain, and extend.

## Teaching-first workflow

- Work in small, coherent steps and introduce one major concept at a time.
- Explain unfamiliar Rust syntax and the reason for important design choices.
- Prefer explicit, readable code over clever shorthand or advanced abstractions.
- Do not provide a large replacement file when the request is only for an explanation or diagnosis.
- When implementation is requested, implement it and then explain the important parts and ownership flow.
- Do not introduce advanced generics, procedural macros, trait objects, complex lifetimes, or async code until the feature genuinely needs them.
- Do not silence borrow-checker problems with unnecessary cloning. Explain ownership choices and clone only when copying is intentional.
- Compiler warnings and errors are learning opportunities. Explain their root cause instead of merely suppressing them.

## Architecture direction

Keep the structure simple while the application is small. Split code only when a file or responsibility has become meaningfully distinct.

The intended direction is:

- `main.rs`: start and configure the eframe application.
- `app.rs`: root application state, application logic, and top-level egui composition.
- Model modules: RimWorld mod metadata and load-order types without egui dependencies.
- Service modules: filesystem scanning, XML parsing, and load-order persistence.
- Screen or UI modules: add these when the interface is large enough to benefit from smaller rendering and interaction functions.

eframe owns one root `App`. Persistent application data belongs in that app or in smaller state structures owned by it. egui is immediate mode: `App::ui` describes the current interface on every repaint and handles widget responses such as clicks. Split a growing UI into smaller methods or modules that accept `&mut egui::Ui` and the minimum state they need. Keep filesystem and domain logic out of UI rendering code.

## Rust and dependency conventions

- Use Rust edition 2024 as configured in `Cargo.toml`.
- Use APIs compatible with the dependency versions recorded in `Cargo.toml`; egui and eframe change between releases, and older tutorials may use obsolete APIs.
- Before recommending a custom widget, abstraction, or workaround, check the documentation and, when useful, the installed source for the exact dependency version in `Cargo.toml`. First determine whether the library already provides a simpler built-in API.
- Prefer the library's supported built-in API over manual rendering or custom infrastructure when it satisfies the requirement. Explain why the API works so the project owner learns the relevant library concept.
- Treat version-sensitive API advice as something to verify rather than answer from memory. Clearly identify any remaining uncertainty instead of presenting an unverified design as the necessary approach.
- Prefer owned application data such as `String`, `PathBuf`, and `Vec<T>`. Introduce borrowed data and explicit lifetimes only when they provide a clear benefit.
- Represent optional values with `Option` and recoverable failures with `Result`.
- Avoid `unwrap` and `expect` on normal user-controlled paths such as files, directories, and XML input.
- Add dependencies with Cargo and explain why each new dependency is needed.
- Do not edit `Cargo.lock` manually.

## Verification

After code changes, run the checks appropriate to the change:

```text
cargo fmt --check
cargo check
cargo test
```

Use `cargo clippy` when the code is sufficiently complete for its suggestions to be useful. Do not apply lint suggestions blindly; keep the teaching goal in mind.

## RimWorld data safety

- Treat game and mod directories as read-only while scanning.
- Do not modify a user's RimWorld configuration or load order unless the requested feature explicitly requires it.
- Before implementing writes, define validation, error handling, and a recoverable backup strategy.
