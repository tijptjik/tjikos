Frozen pre-migration engines from Dotjik, captured 2026-10-29. These files are
independent compatibility oracles, not runtime code. Never run their entrypoints:
they contain former host defaults. Tests may extract pure functions with AST or
import guarded modules while replacing all filesystem/process/network inputs.
