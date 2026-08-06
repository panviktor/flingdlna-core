# Security policy

## Reporting a vulnerability

Please do not open a public issue for a suspected security vulnerability.
Contact the maintainers privately through the security contact published on the
repository's GitHub Security tab.

## Secrets and releases

The project must never contain API keys, Telegram bot tokens, Apple signing
certificates, provisioning profiles, `.env` files, databases, or logs. Run
`./scripts/audit-open-source.sh` from a clean checkout before every release.

If a credential is ever committed, revoke or rotate it before publishing any
repository that contains the commit, even if the commit is later removed.
