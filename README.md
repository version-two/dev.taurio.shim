# taurio-shim

One tiny native Windows binary that pretends to be many. Copy or hardlink it to
`<name>.exe`, drop a `<name>.shim` next to it, and the shim execs the real
target with the configured args plus whatever the user passed.

This is the same idea [Scoop's shim](https://github.com/ScoopInstaller/Shim)
and [busybox](https://busybox.net/) use. Taurio uses it so things like `php`,
`composer`, `mysql`, `git` show up as real `.exe` files that `where.exe`,
Node's `child_process.spawn`, Python's `subprocess`, Git Bash, and Go's
`exec.Command` all find natively – which `.bat` shims can't do.

## How it dispatches

1. Reads its own filename via `current_exe()` and lowercases the stem (e.g. `php`).
2. If the stem is `php` or `composer`, runs Taurio's project-aware PHP runtime
   resolver internally – walks up looking for `.taurio.json`, then reads
   `%APPDATA%\Taurio\taurio.json` to pick the runtime.
3. Otherwise reads the sibling `<stem>.shim` file:
   ```ini
   path = "C:\Taurio\bin\mysql\bin\mysql.exe"
   args = "--defaults-file=C:\Taurio\etc\mysql\my.ini"
   ```
4. Execs `path` with `args` + the user's argv, propagates the exit code.

`#`-prefixed lines and blank lines are ignored. `args` may quote tokens with
spaces using double quotes.

## Install

Download `taurio-shim.exe` from the [latest release](https://github.com/version-two/dev.taurio.shim/releases/latest)
and copy it once per shim name:

```powershell
Copy-Item taurio-shim.exe C:\Taurio\shims\php.exe
Copy-Item taurio-shim.exe C:\Taurio\shims\composer.exe
Copy-Item taurio-shim.exe C:\Taurio\shims\mysql.exe
Set-Content C:\Taurio\shims\mysql.shim @'
path = "C:\Taurio\bin\mysql\bin\mysql.exe"
args = ""
'@
```

`php` and `composer` need no `.shim` file – they resolve via Taurio's config.

## Build

Requires Rust (stable). Output is one statically linked `~270 KB` binary.

```powershell
cargo build --release
# target\release\taurio-shim.exe
```

Release builds are produced automatically by GitHub Actions on `v*` tag push.

## License

MIT – see [LICENSE](LICENSE).
