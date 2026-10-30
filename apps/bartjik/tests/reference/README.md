# Migration comparison oracle

The former Python implementation is retained only for development-time contract
comparison. Runtime source locations in the oracle point at the current artwork
and presets. Nothing in this directory is packaged or invoked by an installed
command. The native executable tests and frozen Rust contracts validate the port;
these reference tests alone do not establish native runtime correctness.
