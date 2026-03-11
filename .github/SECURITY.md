# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| latest  | Yes       |

## Reporting a Vulnerability

If you discover a security vulnerability, please **do not** open a public
issue. Instead, report it privately via one of these methods:

1. **GitHub Security Advisories** (preferred):
   <https://github.com/Fancy-Mumble/FancyMumbleNext/security/advisories/new>

2. **Email**: Open a private advisory on the repository (see link above).

We will acknowledge receipt within **48 hours** and aim to provide a fix or
mitigation within **7 days** for critical issues.

## Scope

This policy covers the Fancy Mumble desktop application and its protocol
library (`mumble-protocol`). It does **not** cover the upstream Mumble
server software.

## What Qualifies

- Remote code execution
- TLS/certificate validation bypasses
- XSS or injection in the chat/profile rendering pipeline
- Authentication or authorisation bypasses
- Information disclosure (e.g. credential leaks, key material exposure)

## What Does Not Qualify

- Denial of service against a user's own machine
- Issues in third-party dependencies (report those upstream, but let us
  know so we can update)
- Social engineering

## Disclosure

We follow coordinated disclosure. Once a fix is released we will credit
the reporter (unless they prefer anonymity) in the release notes.
