# Fish completions

The app installers place completions in `PREFIX/share/fish/vendor_completions.d`.
To install or refresh just the completions, without rebuilding or restarting apps:

```sh
bash scripts/install-completions.sh "$HOME/.local"
```

Optional app names select a subset. The completion files cover all seven CLI
apps, the two control commands, and the managed native command wrappers. Fish
autoloads them; no shell startup changes are needed. Custom completions under
`~/.config/fish/completions` retain priority.

Completions use static command and option definitions. They never inspect mail,
backup inventories, credentials, or remote machines. Greetjik alone lists its
bundled artwork names with its read-only `--all --list` command.

When changing a CLI, update its `.fish` file and run:

```sh
python3 -B -m unittest discover -s tests -p test_fish_completions.py -v
```

The tests install into temporary prefixes and use inert application stubs to
verify nested commands, wrapper delegation, option values and paths with spaces.
