Use this tool whenever a file or directory needs to be deleted — removing something the user asked to delete, cleaning up files you created, pruning generated artifacts. All deletion goes through this tool; shell commands are not a deletion path.

Nothing is destroyed: targets are moved to the user's trash, where they can be reviewed and restored.
- linux: entries are trashed with `gio trash` following the freedesktop trash spec; desktop file managers list them under Trash. A target on another filesystem goes to that volume's own trash directory when one is available; system-internal mounts (`/tmp`, `/run`, …) and volumes without a usable trash directory are refused, and the entry fails with the raw error. When the call fails with `gio was not found on the host`, ask the user to install glib2 or to delete the paths themselves.
- macos: entries are trashed through the system file manager, silently and without any permission prompt; the user restores them by dragging out of the Trash (Finder's "Put Back" may be unavailable for them). A target on another volume goes to that volume's own trash when it supports one; otherwise the entry fails with the system's error. Privacy-protected folders (Desktop, Documents, Downloads, network volumes) can fail per entry when the app lacks Files and Folders access — relay the error and point the user at System Settings → Privacy & Security → Files and Folders.

`paths` is a list of absolute paths. Each entry is processed independently: one failure does not stop the others. Symbolic links are trashed as links; their target is untouched.

Elevation is not supported. When an entry fails with a permission error, relay the error and suggest how the user can delete the target themselves.

Permanent deletion is out of scope. When the user explicitly wants one, suggest how they can run it themselves.

The result is `<delete_output/>` when every path was trashed; otherwise it carries one child per path that failed:

    <delete_output>
      <failed target="..."><![CDATA[reason]]></failed>
    </delete_output>

A failure of the whole call is reported as a single `<error>` child instead:

    <delete_output>
      <error><![CDATA[reason]]></error>
    </delete_output>
