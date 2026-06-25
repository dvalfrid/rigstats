# Security Policy

## Supported Versions

Only the latest release receives security fixes.

| Version | Supported |
|---|---|
| Latest release | ✅ |
| Older versions | ❌ |

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

Use one of these private channels:

- **GitHub private vulnerability reporting** — [Report a vulnerability](https://github.com/dvalfrid/rigstats/security/advisories/new) (preferred)
- **Email** — daniel@valfridsson.net

Include as much of the following as possible:

- Type of issue (e.g. privilege escalation, pipe injection, installer tampering)
- Steps to reproduce
- Affected version
- Potential impact

## Response Timeline

| | Target |
|---|---|
| Acknowledgement | Within 7 days |
| Patch release | Within 14 days of confirmed vulnerability |

## Scope

RigStats runs as a regular user process but installs a Windows Service (`rigstats-sensor`) running as LocalSystem, and loads the PawnIO kernel driver for hardware sensor access. Security issues most relevant to this project:

- Privilege escalation via the named pipe (`\\.\pipe\rigstats-sensors`)
- Installer or auto-updater tampering (supply chain)
- Kernel driver misuse via PawnIO
- Unintended data exposure in CSV logs or the debug log
