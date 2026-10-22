# Security

Found a security issue in Crest? Please report it privately via **Security → Report a
vulnerability** in this repository (https://github.com/rinci-labs/crest) instead of opening a public
issue, so it can be fixed before it becomes widely known. Include the Crest version, your Windows
version, and steps to reproduce.

In scope:

- the administrator step for incoming messages (`src/identity.rs`): certificate, signing, package;
- the installer and uninstaller (`src/install.rs`);
- data that leaves the computer (lyrics and weather, both optional) and incoming message content.
