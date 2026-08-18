Use this tool whenever a file or directory needs to be deleted — removing something the user asked to delete, cleaning up files you created, pruning generated artifacts. All deletion goes through this tool; shell commands are not a deletion path.

Nothing is destroyed: targets are moved to the Recycle Bin, where the user can review and restore them; Ctrl+Z in Explorer undoes the most recent deletion. A target open in another program can fail with a sharing violation — relay the error and suggest closing that program before retrying.

`paths` is a list of absolute paths. Each entry is processed independently: one failure does not stop the others. Symbolic links are trashed as links; their target is untouched.

Elevation is not supported. When a target needs it, relay the error and suggest how the user can delete it themselves.

Permanent deletion is out of scope. When the user explicitly wants one, suggest how they can run it themselves.

The result is `<delete_output/>` when every path was trashed; otherwise it carries one child per path that failed:

    <delete_output>
      <failed target="..."><![CDATA[reason]]></failed>
    </delete_output>

A failure of the whole call is reported as a single `<error>` child instead:

    <delete_output>
      <error><![CDATA[reason]]></error>
    </delete_output>
