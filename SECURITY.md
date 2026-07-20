# Security Policy

## Reporting a vulnerability

Please **do not open a public issue** for security problems.

Use GitHub's **"Report a vulnerability"** button (Security tab → Advisories) to
disclose privately, or email **maizemedia@protonmail.com**. You'll get an
acknowledgement as soon as practical; this is a personal project, so there's no
formal SLA and no bounty program.

## Scope notes

- MaizeView stores no accounts, tokens, or telemetry. Your library database and
  settings live only on your machine (`%APPDATA%/MaizeView`).
- The app shells out to ffmpeg/ffprobe on local files — issues around malformed
  media handling are in scope.
- Anything that could expose another user's library paths or data is a security
  issue here — please report it.
