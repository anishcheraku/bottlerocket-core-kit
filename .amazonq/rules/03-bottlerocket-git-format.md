## Bottlerocket Git Commit Convention

* Commits MUST NOT follow the conventional commit format
* Commits MUST follow this format:
```
<component>: <description>

<body>
```

Where:
* <component> MUST be the subsystem you modified (e.g., "prompt-plan", "implement", "code-review")
* <description> MUST use present imperative tense (e.g., "add", "fix", "update", not "adds", "fixed", "updated")
* <description> MUST be lowercase and MUST NOT end with a period
* The commit message SHOULD complete the sentence: "If applied, this commit will ________"
* If needed, you MAY add a body separated by a blank line, with lines no longer than 72 characters

Example:
```
prompt-plan: implement structured format parser

This change adds a parser for the structured prompt plan format that
extracts prompt information and status.
`
