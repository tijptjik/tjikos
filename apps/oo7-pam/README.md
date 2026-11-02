# oo7 PAM for Hyprlock

An oo7 PAM module with an opt-in `unlock_on_auth` mode for screen lockers that
call `pam_authenticate` and `pam_end` without opening a PAM session. Installed
separately as `/usr/local/lib/security/pam_oo7_tjik.so`; Fedora's module and oo7
daemon are retained.

The option saves the password only in zeroizing memory attached to the PAM
transaction. Delivery happens synchronously during `pam_end(PAM_SUCCESS)` via
oo7's existing user-owned socket. Failed transactions, replaced PAM data, empty
tokens and biometric-only authentication do not deliver a password. The daemon
must already be running. Socket failure cannot grant or deny screen access;
the normal system authentication stack remains authoritative.

Build with `bun run build --filter=@tjikos/oo7-pam`, test with
`bun run --cwd apps/oo7-pam test`, and install with
`bash apps/oo7-pam/scripts/install.sh`. Dotjik owns the PAM and login policies.
Tests use synthetic tokens, a private PAM configuration and isolated sockets.

Source files and PAM test helpers are based on linux-credentials/oo7 commit
`e91b98a7235a4971c6cbb7f0533034afc8a8f618`, matching Fedora 45's oo7 package.
The MIT license and upstream authorship are retained in [LICENSE](LICENSE).
Local changes add the explicit auth-only completion hook and regression tests;
upstream session and password-change modes retain their behavior.
