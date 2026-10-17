# towerfall-port

A clean-room reimplementation of [TowerFall Ascension](https://store.steampowered.com/app/251470/TowerFall_Ascension/)
in Rust, for Linux and Windows (macOS later): up to 12 archers, local on one
screen with controllers, online through [Warpzone](https://github.com/jarvis-jhk/warpzone),
or both mixed.

**Status: feature-complete pre-release.** Versus (Last Man Standing,
Headhunters, Team Deathmatch) for 2-12 archers with every tower, arrow,
treasure and variant, Quest, Trials and Dark World, online play with
rollback, options, controller remapping and records all work; Linux and
Windows (cross-built, checked under Wine) release archives build from this
tree. Still to come: a check on a real desktop and on real Windows, and macOS. Details in [docs/STATUS.md](docs/STATUS.md).

You need your own copy of TowerFall Ascension. towerfall-port contains no game
data: it reads the art, sounds, music and levels from your installation.
Not affiliated with Matt Makes Games / Maddy Makes Games.

Clean room: the behaviour was first described in our own words from the
original (specs kept private, they describe its internals), and the Rust code
was written from those descriptions.

Players: [packaging/README-player.md](packaging/README-player.md) (also the
README inside the release archives: finding your game, controls, online).

## Building

```sh
scripts/bootstrap-tools.sh   # toolchains into ./tools
source env.sh
cargo build --release
scripts/package.sh           # release archives in dist/ (Linux tar.gz, Windows zip)
```

In a git worktree, `source env.sh` from the main checkout (it sets
`TFP_ROOT`, where the build finds the project-local SDL2 in `tools/lib`).

## Checking

```sh
cargo test --workspace
scripts/dev/flow-test.sh          # the whole app, headless: every menu path, a match at 4/8/12 archers
scripts/dev/coop-proof.sh         # Quest / Trials presentation sheets
scripts/dev/darkworld-proof.sh    # Dark World sheets
scripts/dev/determinism-check.sh  # Linux vs Windows (Wine) sim checksums
target/release/towerfall-port --headless --soak 220   # random bot matches
```

All of these need your TowerFall install (found by itself, or `TF_GAME_DIR`).

## License

Code: MIT or Apache-2.0, at your option. TowerFall and its assets are © Matt
Makes Games Inc. and are not part of this project.
