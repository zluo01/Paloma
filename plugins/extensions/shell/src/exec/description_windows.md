Executes a command and returns its stdout, stderr, and exit code.

`command` is an argv array: argv[0] is the program (e.g. "git", "cargo", "powershell"), the remaining elements are its arguments. To use shell features (pipes, globs, redirection, env-var expansion, chained commands), invoke a shell explicitly:

    ["powershell", "-NoProfile", "-NonInteractive", "-Command", "Get-ChildItem *.log | Select-String error"]
    ["powershell", "-NoProfile", "-NonInteractive", "-Command", "git add .; git commit -m 'fix'"]

`powershell` always invokes Windows PowerShell 5.1: `&&` and `||` do NOT exist there and are a parse error. Chain unconditionally with `;`, conditionally with `if ($?) { ... }`. PowerShell 7 (`pwsh`) is a separate program that may not be installed — do not assume it exists. Do not invoke `cmd` — it is unsupported; anything it can do, PowerShell can. Plain commands need no shell:

    ["cargo", "build", "--release"]
    ["git", "status"]

`workdir` is required and must be an absolute path (e.g. "C:\Users\me\project"). Set the working directory via `workdir`; do NOT use `cd` in the command.

Each call spawns a fresh process, so nothing persists between calls — not the working directory, not environment variables, not PowerShell functions or loaded modules. Set `workdir` on every call, and keep genuinely dependent steps together in one shell invocation.

stdin is NUL. A command that waits for input never receives any: confirmation prompts, `pause`, `choice`, editors and pagers (`notepad`, `more`), and `ssh` without key auth block until the timeout and return nothing useful. `timeout.exe` dies instantly with "ERROR: Input redirection is not supported" — sleep with `["powershell", "-NoProfile", "-Command", "Start-Sleep 5"]` instead. Prefer non-interactive forms — `-NonInteractive`, `-Confirm:$false`, `git --no-pager`, `ssh -o BatchMode=yes` — and read a file rather than opening it.

Current date/time:
- Local time: run exactly ["powershell", "-NoProfile", "-Command", "Get-Date -Format 'yyyy-MM-ddTHH:mm:sszzz'"] (e.g. 2026-08-03T14:32:05-07:00).
- UTC: run exactly ["powershell", "-NoProfile", "-Command", "(Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')"] (e.g. 2026-08-03T21:32:05Z).
- Do not use `date` or `time` (cmd builtins that prompt to set a new value) or improvise other formats.

Output handling:
- Each stream is captured up to 50 KiB inline; ANSI escape sequences are stripped from the payload.
- When a stream exceeds 50 KiB, the full untruncated bytes are written to a file under %TEMP%\paloma\<exec_id>\ and its absolute path is surfaced via the `full_output` attribute. Follow up with a separate shell call (Get-Content -TotalCount/-Tail, Select-String) on that path to inspect more.
- Do NOT pre-truncate output yourself (no Select-Object -First unless the user explicitly asks) — run the command directly and let truncation happen.

Commands time out after 300 seconds.

The result is an XML envelope shaped like:

    <exec_output command="..." workdir="..." exec_id="..." exit_code="..." duration_ms="..." [timed_out="true"]>
      <stdout total_bytes="..." [truncated="true"] [full_output="..."]><![CDATA[...]]></stdout>
      <stderr total_bytes="..." [truncated="true"] [full_output="..."]><![CDATA[...]]></stderr>
    </exec_output>

Attributes:
- exec_id: the tool-call id; also the spill directory name under %TEMP%\paloma\
- exit_code: numeric exit code, or "timed_out"; a command killed by cancellation reports 1
- duration_ms: wall-clock execution time in milliseconds
- timed_out: present only when the 300 s timeout fired
- total_bytes: full byte count of the stream including any spilled portion
- truncated: present when the inline payload was capped; the CDATA body ends with "..."
- full_output: absolute path to the complete output on disk when truncated

Approval:
- The launcher classifies the argv and asks the user to approve anything it does not already trust, so the argv must make the operation legible.
- A `powershell -Command` string cannot be split for classification; it prompts on every run, with no way to remember it. Keep shell strings simple, or split the steps into separate calls. Plain argv calls (["git", ...], ["cargo", ...]) can be approved once and remembered — prefer them, and reach for a shell only when its features are genuinely needed.
- A denial is final. On "command was denied by the user", "could not be validated", or "permission request was cancelled", nothing ran: report it and stop. Do not retry, reword, or reach the same outcome another way.

Deletion:
- Deletions belong in the `ext__Shell__DeleteFiles` tool, which moves targets to the Recycle Bin, where the user can restore them. Prefer it over any delete through this tool.
- Permanent deletes (`Remove-Item`, or its aliases `del`/`rm`/`rd`) are unrecoverable: use them only when the user explicitly asked for one. Keep the target legible — pass each path as its own argv element (["powershell", "-NoProfile", "-Command", "Remove-Item", "-LiteralPath", "C:\path\to\file"]) — and never bury a deletion inside a longer command string or a generated script.
- Recursive shapes (`Remove-Item -Recurse`) are the riskiest form; never restructure a command to make one look routine.

Privilege escalation:
- There is no supported elevation path: do not attempt `sudo` (usually absent; Windows' optional Sudo and tools like gsudo trigger a UAC prompt and run in a separate window whose output cannot be captured), `runas` (prompts for a password on a console this tool does not have), or `Start-Process -Verb RunAs` (cannot capture output). When something genuinely needs administrator rights, say so and suggest how the user can run it themselves.
