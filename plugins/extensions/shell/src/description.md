Executes a command and returns its stdout, stderr, and exit code.

`command` is an argv array: argv[0] is the program (e.g. "git", "java", "bash"), the remaining elements are its arguments. To use shell features (pipes, globs, redirection, env-var expansion, chained commands), invoke a shell explicitly:

    ["bash", "-lc", "pacman -Q | grep firefox"]

`workdir` is required and must be an absolute path. Set the working directory via `workdir`; do NOT use `cd` in the command.

Output handling:
- Each stream is captured up to 50 KiB inline; ANSI escape sequences are stripped from the payload.
- When a stream exceeds 50 KiB, the full untruncated bytes are written to /tmp/scry/<exec_id>/<stdout|stderr> and the path is surfaced via the `full_output` attribute. Follow up with a separate shell call (tail, head, grep) on that path to inspect more.
- Do NOT pre-truncate output yourself (no head/tail/sed unless the user explicitly asks) — run the command directly and let truncation happen.

Commands time out after 300 seconds.

The result is an XML envelope shaped like:

    <shell_output command="..." workdir="..." exec_id="..." exit_code="..." duration_ms="..." [timed_out="true"]>
      <stdout total_bytes="..." [truncated="true"] [full_output="..."]><![CDATA[...]]></stdout>
      <stderr total_bytes="..." [truncated="true"] [full_output="..."]><![CDATA[...]]></stderr>
    </shell_output>

Attributes:
- exec_id: the tool-call id; also the spill directory name under /tmp/scry/
- exit_code: numeric exit code, "timed_out", or "terminated_by_signal"
- duration_ms: wall-clock execution time in milliseconds
- timed_out: present only when the 300 s timeout fired
- total_bytes: full byte count of the stream including any spilled portion
- truncated: present when the inline payload was capped; the CDATA body ends with "..."
- full_output: absolute path to the complete output on disk when truncated
