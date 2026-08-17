# delfaf

delete fast as F#@*.

**Be careful with this one.**

Pairs with [fafind](https://github.com/rywils/fafind).
It will delete any file that fafind printed.
Use caution or use exact matching mode with fafind.
Pass `-y` into delfaf to skip confirmation.

## install

```bash
cargo build --release
sudo cp target/release/delfaf /usr/local/bin/
```

## usage

Pipe:

```bash
faf node_modules / | delfaf
```

Redirect to a file, then delete:

```bash
faf node_modules / > hits.txt
delfaf hits.txt
```

Skip the prompt:

```bash
faf node_modules / | delfaf -y
```

After a search, with no pipe:

```bash
faf node_modules /
delfaf
```

That last form reads `~/.cache/fafind/last` (or `$XDG_CACHE_HOME/fafind/last`).
`faf` has to write that file.
Override the path with `FAFIND_LAST`.
If `HOME` and `XDG_CACHE_HOME` are unset, `FAFIND_LAST` is required.

Do not use `faf ... > delfaf`.
That overwrites a file named `delfaf` in the current directory.
Use a pipe.

## behavior

- Before deleting, prompts `Are you sure you want to delete (N) of files? (N/y)`. Default is No.
- The prompt is read from the terminal, so a pipe still asks.
- Files and directories are removed. Directory trees go with `remove_dir_all`.
- Symlinks are unlinked. The target is left alone.
- Missing paths are skipped.
- `/`, `.`, and any path with `..` are refused.
- Failed deletes print the path and the error.
- Deletes run in parallel.

## exit codes

```text
0 = all listed paths deleted (missing skipped)
1 = no paths, aborted, or some deletes failed
2 = no input (no pipe, no file, no last-hits cache, or no cache dir)
```
