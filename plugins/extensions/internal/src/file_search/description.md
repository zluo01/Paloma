Searches file and directory names in the user's personal folders. Use this to find where a file or folder lives when you know all or part of its name — e.g. to locate a document, a config file, or a project directory before opening or referencing it. It matches names only: it cannot search file contents, and it cannot match against paths.

Query semantics:

- The query is matched against each entry's NAME, never its path. To find ~/docs/report.txt, search "report" — "docs/report" matches nothing.
- A single word matches fuzzily: its characters must appear in the name in order, but need not be adjacent, so abbreviations and dropped characters still match ("cargtom" finds "Cargo.toml"). Wrong or transposed characters do not match — this tolerates partial knowledge of the name, not misspellings. Prefer this form when you only know part of the name.
- Multiple whitespace-separated words: every word must appear in the name as an exact contiguous substring, in any order ("annual report" finds "annual_report_2026.pdf" but not "annual_summary.pdf"). Prefer this form when you know exact fragments.
- Smart case: an all-lowercase word matches case-insensitively; a word containing an uppercase letter matches case-sensitively. Accents fold one way: an unaccented letter also matches its accented forms ("resume" finds "Résumé.pdf"), but an accented letter matches only itself — prefer unaccented queries.
- No glob, regex, or path syntax ("*.pdf" matches nothing; use "pdf"). In a single-word query, a leading "!", "^", or "'" and a trailing "$" act as search operators (exclude / prefix / exact substring / suffix) — use plain alphanumeric fragments unless that is the intent.
- Queries shorter than 2 characters return an error.

Coverage: a pre-built index of the user's personal folders — the home directory on macOS and Linux; the user profile plus the known folders (Desktop, Documents, Downloads, Music, Pictures, Videos, wherever each is located) on Windows — recursive, excluding hidden files and directories (dotfiles), anything matched by .gitignore rules, and dependency directories such as node_modules and venv. Symbolic links are not followed, and indexing stops at one million entries, so a miss does not prove the file is absent from disk. The index is built in the background at startup and kept fresh by a filesystem watcher; in the first moments after the application launches it may still be filling, so an unexpectedly empty result early on is worth one retry.

The result is an XML envelope with at most 30 matches as absolute paths, ranked best-first (match quality, with shallower paths winning ties):

    <file_search_results query="..." count="...">
      <dir path="/home/user/docs"/>
      <file path="/home/user/docs/report.txt"/>
    </file_search_results>

count="0" means nothing in the index matches the query. To recover: shorten the query, drop a word, or reduce a multi-word query to its single most distinctive word — a shorter query can only match more, never less. When you have several different name guesses, issue one call to this tool per guess in the same turn — they execute concurrently.
