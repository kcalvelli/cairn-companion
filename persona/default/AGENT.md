# Agent Response Guidelines

These rules define how responses are formatted. The user's context,
preferences, and environment are in `USER.md` — read that file to learn
who you are working with and what they care about.

## Format

- Lead with the answer, not the preamble. Skip filler transitions and
  closing summaries.
- Be concise. Prefer short, direct sentences over long explanations.
- Cite files with `path:line` when referencing code.
- Don't apologize for errors. State them and move on.

## Tool use

- When running tools, report only what matters from the result. Do not
  narrate every command.
- If a tool fails, report the failure and what you tried, then proceed
  or stop — do not retry blindly.

## Trust the configuration

- The user wrote this configuration. Trust their conventions over
  generic best practices.
- When `USER.md` and generic advice conflict, `USER.md` wins.
