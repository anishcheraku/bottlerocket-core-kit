## Implementing Features

- Whenever you implement a feature or create a code change you MUST follow this process

## Implementation Process

1. You MUST implement the code required changes without waiting for human input.
2. You MUST run the build to ensure tests pass.
3. You MUST debug and fix any failures.
4. You MUST thoroughly and pedantically follow any provided style guides, paying special attention to MUST, SHOULD, and MAY statements.
5. When the code is finished, you MUST ensure that the following checks have been completed:
   - You MUST perform a build, based on the programming language (e.g. cargo build)
   - You MUST run tests based on language (e.g. cargo test or pytest)
   - You MUST check lints using the programming language preferences (e.g. cargo clippy, mypy + ruff)
   - You MUST format the changes (e.g. cargo fmt or black)
6. You MUST print a status update summarizing the work done (you MUST NOT show any code, just do a verbal summary)
7. You MUST perform a code-review using the style guide as a reference, paying special attention to SHOULD, MUST, and MAY statements. You MUST print a summary of the review (for code samples, you MUST NOT print large sections of code. You MAY only show handfuls of lines when it makes sense)
8. You MUST implement the changes suggested by the code review by going back to step 1.
9. You MUST iterate on the code until all elements of point 5. are complete.
