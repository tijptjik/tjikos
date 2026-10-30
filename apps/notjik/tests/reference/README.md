Frozen Python baseline for migration comparison tests only. These modules are not
application entry points, are never built or installed, and are never selected
as runtime fallbacks. The supported implementation is ../../src (Rust).

Existing test_*.py files describe the old contracts; native_contracts.py exercises
the Rust executable, and cargo test covers the native state machines and frozen
Telegram display fixtures. benchmark_native.py compares both workers using
synthetic, private state.

The visual test drivers may use reference functions solely to construct synthetic
fixtures or issue private IPC requests. Their QML workers and action/recording
processes are the built Rust executable. The native executable tests separately
verify the Rust IPC implementation and its argument/file privacy boundary.
