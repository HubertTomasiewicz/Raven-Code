# Raven Code workspaces

A workspace file uses the `.rvenc` extension and stores a versioned JSON document:

```json
{
  "version": 1,
  "root": "."
}
```

`root` is resolved relative to the `.rvenc` file when possible. Raven Code also accepts absolute paths. To create one, open a folder and choose **File → Save Workspace As…**. Open it with **File → Open Workspace…**, the ordinary Open File command, or by passing the `.rvenc` path to `raven-code.exe`.

The file currently records the workspace root only. Open editor tabs and per-workspace settings are not serialized yet.

## Windows file icons

The supplied category icons are used inside Raven Code's Explorer and editor tabs. To register the matching `.ico` files for Windows Explorer and add Raven Code to **Open with** for common text and binary extensions, run `scripts/register-raven-file-icons.ps1` from the repository after building. It registers for the current Windows user and keeps Raven Code as the default application only for `.rvenc`.
