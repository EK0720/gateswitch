# Security policy

Model Switcher handles API keys, so security reports are taken seriously.

## Reporting

Please **do not open a public issue** for a vulnerability. Use GitHub's private reporting instead: **Security → Report a vulnerability** on this repository.

Include what you found, how to reproduce it, and the version. **Do not include real API keys.**

## Scope

Examples of what counts:

- A key written to a log, error message, support report, preset or backup in plain text.
- A file written outside the documented config locations, or a write that cannot be undone.
- Code execution through a gateway response, preset file or model name.

## Supported versions

Only the latest release gets fixes.
