# Working agreement for Claude Code sessions

Read and follow `.github/copilot-instructions.md` first. Its rules apply to every coding agent in this repository, not only Copilot. Where this file adds detail, it adds to those rules and never loosens them.

## How a session runs
- **The work order is a GitHub issue.** The user will name an issue number. Read it in full (`gh issue view <n>` if `gh` is signed in, otherwise fetch `https://api.github.com/repos/Dissolutio/HGSproto/issues/<n>`, which is public). Do exactly what it says. If it is ambiguous or needs a decision, stop and ask the user instead of choosing.
- **Branching.** Start from the latest `dev`. Create a branch named `claude/<issue-number>-<short-name>`. Never commit to `main` or `dev` directly.
- **One issue, one branch, one pull request.** Keep commits small. List any change the issue did not request.
- **Opening the pull request.** Push the branch and open a **draft** pull request into `dev` with `Fixes #<issue>` in the body. If you cannot open it, say so and give the user the branch name. Do not merge it.
- **Do not edit `.github/workflows/`, release files, lockfiles, or anything that reads secrets** unless the issue says so. Flag it if the work seems to need it.
- **Do not ask for, store, or print credentials.** If a sign-in is needed, tell the user to do it in their own terminal or browser.

## What the pull request description must contain
1. What changed, and which acceptance items are met.
2. Exact commands run and exact test counts, before and after where relevant.
3. What was verified by running, what was inferred by reading, and what was not verified.
4. Manual check steps for anything a human must see in a browser.
5. Any failure, reported honestly. Never retry a failure to make it pass.

## Browser behavior
You may only claim a browser check if you actually drove a browser and saw the result. Say which browser and what you saw. Otherwise mark it **NOT EXERCISED** and give the user the manual steps.

## Local build notes
- The first `npm --prefix client run dev` or `build` compiles the WASM package and can take a few minutes. It needs `wasm-pack`.
- CI does not build the Tauri desktop crate and does not launch a browser. Do not treat green CI as proof of either.
