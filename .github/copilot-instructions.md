# Process
- Issues are work orders written by the Product Manager. Do exactly what the issue says. If something is ambiguous, conflicts with the repository, or needs a decision, stop and ask in the pull request instead of choosing.
- The base branch is `dev`. Never target, push to, or merge into `main`. Open a draft pull request and do not merge it. Remove any "[WIP]" from the title when finished.
- One issue, one pull request. Keep commits small and labeled. List any change you made that the issue did not request.
- Do not touch `main`, release workflows, lockfiles, or anything the issue does not name, unless the issue says otherwise.

# Authority and scope
- The Rust core is the only rules authority. No game rules in the browser client, the gateway, or the native peer receiver.
- Data-only extension: profiles and scenarios choose from a finite, validated list of supported mechanics. No scripts, expressions, or executable behavior.
- Do not invent gameplay rules, budgets, or balance numbers. Attack damage is exactly one, defined by the core. Anything needing a rules decision is a question for the PM.
- Preserve existing public contracts: action grammar, function signatures, snapshot shape and values, legacy aliases. Add; do not change.
- Do not change networking code, protocol, timeouts, or retry behavior unless the issue says so. Networking work is currently parked.

# Evidence
- Every pull request description must state: what changed, which acceptance items are met, exact test counts, and what was not verified.
- Report every failure honestly, including ones that disappear on retry. Never retry a failure to make it pass, never raise a timeout, and never edit a test to make it pass unless the issue authorizes it and explains why.
- The test `network_simulation_auth_sync_disconnect_and_recovery` (default mode) is known to fail occasionally (about 7 percent, risk R02, cause unknown). If it fails, report it. Do not retry it away and do not change it. The direct-only recovery variant must pass.
- If a change affects what a player sees in the browser, say plainly that a human browser check is needed and whether you were able to run one. Never claim a check you did not perform.
- Separate what you verified by running it from what you inferred by reading code.

# Public repository
- This repository is public. Never put secrets, tokens, connection tickets, keys, personal paths, or private notes in code, comments, issues, pull requests, or screenshots. Do not commit evidence screenshots unless asked.
- Do not send personal data to external services.

# Style
- Plain-language summaries. Explain decisions briefly. Flag risks and anything that would make later work harder.
