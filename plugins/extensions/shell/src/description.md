Executes a command and returns its stdout, stderr, and exit code.

`command` is an argv array: argv[0] is the program (e.g. "git", "java", "bash"), the remaining elements are its arguments. To use shell features (pipes, globs, redirection, env-var expansion, chained commands), invoke a shell explicitly:

    ["bash", "-lc", "pacman -Q | grep firefox"]

`workdir` is required and must be an absolute path. Set the working directory via `workdir`; do NOT use `cd` in the command.

Each call spawns a fresh process, so nothing persists between calls — not the working directory, not exported variables, not shell functions or activated environments. Set `workdir` on every call, and keep genuinely dependent steps together in one `bash -lc`.

stdin is /dev/null. A command that waits for input never receives any: editors and pagers (`vim`, `less`, `top`), confirmation prompts, and `ssh` without key auth block until the timeout and return nothing useful. Use the non-interactive form instead — `apt-get -y`, `git --no-pager`, `ssh -o BatchMode=yes` — or read the file rather than opening it.

Current date/time:
- Local time: run exactly `["date", "+%Y-%m-%dT%H:%M:%S%z"]` (e.g. 2026-08-01T14:32:05-0700).
- UTC: run exactly `["date", "-u", "+%Y-%m-%dT%H:%M:%SZ"]` (e.g. 2026-08-01T21:32:05Z).
- Do not use bare `date` (locale-dependent output) or improvise other formats; these two work identically on Linux and macOS.

Output handling:
- Each stream is captured up to 50 KiB inline; ANSI escape sequences are stripped from the payload.
- When a stream exceeds 50 KiB, the full untruncated bytes are written to a file under /tmp/paloma/<exec_id>/ and its path is surfaced via the `full_output` attribute. Follow up with a separate shell call (tail, head, grep) on that path to inspect more.
- Do NOT pre-truncate output yourself (no head/tail/sed unless the user explicitly asks) — run the command directly and let truncation happen.

Commands time out after 300 seconds.

The result is an XML envelope shaped like:

    <shell_output command="..." workdir="..." exec_id="..." exit_code="..." duration_ms="..." [timed_out="true"]>
      <stdout total_bytes="..." [truncated="true"] [full_output="..."]><![CDATA[...]]></stdout>
      <stderr total_bytes="..." [truncated="true"] [full_output="..."]><![CDATA[...]]></stderr>
    </shell_output>

Attributes:
- exec_id: the tool-call id; also the spill directory name under /tmp/paloma/
- exit_code: numeric exit code, "timed_out", or "terminated_by_signal"
- duration_ms: wall-clock execution time in milliseconds
- timed_out: present only when the 300 s timeout fired
- total_bytes: full byte count of the stream including any spilled portion
- truncated: present when the inline payload was capped; the CDATA body ends with "..."
- full_output: absolute path to the complete output on disk when truncated

Approval:
- The launcher classifies the argv and asks the user to approve anything it does not already trust, so the argv must make the operation legible.
- A `bash -lc` chain is split into its individual commands and each is classified; the strictest verdict wins. A chain of already-trusted commands runs without prompting, but a chain containing anything novel prompts *and cannot be remembered* — so run an unfamiliar command on its own first to get it approved, then use it inside chains freely.
- Shell the parser cannot parse prompts on every run, with no way to remember it. Keep chains simple, or split them into separate calls.
- When invoking `timeout`, `env`, `nice`, or `nohup`, write *their* options canonically and spelled out (`--signal KILL`, not `--sig KILL`; `-v -f`, not `-vf`). The parser unwraps these only in canonical form: an unrecognized option yields a prompt that cannot be remembered, and a malformed one (missing or invalid flag value) fails the call outright. The inner command's own options, and the options of unwrapped commands, can be written normally.
- Some commands are refused before any prompt, because this path has no TTY to drive them: anything containing `sudo`, plus `su`, `passwd`, `ssh-add`, and `gpg --gen-key`/`--full-generate-key`. `rm` is forbidden — see Deletion below. Do not attempt them; tell the user to run it in a terminal.
- Recursive shapes always prompt and are never remembered — `chmod -R`, `chown -R`, `find -delete`, and `find -exec`/`-execdir`/`-ok`/`-okdir`. Expect a prompt every time, and never restructure a command to dodge one.
- A denial is final. On "command was denied by the user", "could not be validated", or "permission request was cancelled", nothing ran: report it and stop. Do not retry, reword, or reach the same outcome another way.

Deletion:
- `rm` is forbidden — every invocation, recursive or not, is refused before any prompt. Never route a deletion through an interpreter or a helper to get around that — no `["python3", "-c", ...]`, no generated script, no `find -delete`, no `xargs`, no emptying a file with `>` or `dd`. Those hide the operation from the argv the user is shown and from the safety parser.
- Move the target to the platform's trash instead, where the user can restore it:
  - macos: `["mv", "<path>", "<home>/.Trash/"]`.
  - linux: `["gio", "trash", "<path>"]` uses the desktop trash under `~/.local/share/Trash`. `gio` ships with glib2 and is present on most desktops, but confirm it exists before relying on it and say so if it does not.
- When the user explicitly wants a permanent delete, or no trash is available, tell the user to run the deletion themselves in a terminal — this path cannot perform one.

Privilege escalation:
- A bare `sudo` is refused outright — there is no TTY for it to prompt on. Use the platform's graphical authentication agent instead, chosen from the host OS. Both forms below always prompt for approval and are never remembered, so elevate only where it is genuinely required.
- linux: `["pkexec", "apt", "install", "ripgrep"]`. Polkit shows a password dialog. It resets the environment, so use absolute binary paths and pass options explicitly rather than relying on inherited env.
- macos: `["osascript", "-e", "do shell script \"<command>\" with administrator privileges"]`, escaping embedded double quotes as `\"`. Most user-level workflows (Homebrew under /opt/homebrew, user LaunchAgents) need no elevation.
- Read without elevation first to confirm something is broken, then elevate only for the write that fixes it. Say in `description` that a password prompt is coming. Never put a password in the argv, echo it, or store it.
