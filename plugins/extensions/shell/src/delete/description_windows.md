Use this tool whenever a file or directory needs to be deleted — removing something the user asked to delete, cleaning up files you created, pruning generated artifacts. Prefer it over any deletion through a shell command.

Nothing is destroyed: targets are moved to the Recycle Bin, where the user can review and restore them.

`paths` is a list of absolute paths. Each entry is processed independently: one failure does not stop the others.

Elevation is not supported. When a target needs it, relay the error and suggest how the user can delete it themselves.

Permanent deletion is out of scope. Reach for it through the `ext__Shell__Exec` tool only when the user explicitly asked for one, following that tool's Deletion rules, or tell the user to run it themselves.

The result is `<delete_output/>` when every path was trashed; otherwise it carries one child per path that failed:

    <delete_output>
      <failed target="..."><![CDATA[reason]]></failed>
    </delete_output>

A failure of the whole call is reported as a single `<error>` child instead; nothing was trashed:

    <delete_output>
      <error><![CDATA[reason]]></error>
    </delete_output>
