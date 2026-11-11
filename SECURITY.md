# Security Policy

`anyzip` is designed to process untrusted archives in memory without crashing the host process,
escaping path normalization, or exceeding configured resource limits.

## Reporting a Vulnerability

Please **do not** open a public issue for security vulnerabilities (such as memory unsafety in FFI,
uncaught panics/aborts, path traversal bypasses, or resource limit bypasses).

Report vulnerabilities privately via
[GitHub Private Vulnerability Reporting](https://github.com/jvitormelo/anyzip/security/advisories/new)
with a minimal reproducer archive (or generator script) and the `Limits` configuration used.
