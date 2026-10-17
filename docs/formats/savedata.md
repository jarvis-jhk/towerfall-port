# Save file (`tf_saveData`) — read-only

`tfp_assets::savedata::SaveData::load(path)` parses the original's save into
a typed model; `SaveData::find(inst)` picks the newest of the install-folder
and per-user copies (`$XDG_DATA_HOME/TowerFall`, macOS Application Support),
either spelling. We never write this file; it is meant for a later "import
settings/stats from your TowerFall save".

Format: .NET `XmlSerializer` XML, root `<SaveData>` (layout and meaning:
`docs/spec/ui.md` §12.5). Reader notes:

* Empty objects/array slots are `xsi:nil="true"` elements → `None` (empty
  variant presets, unused keyboard slots).
* Arrays are repeated children named after the element type
  (`<unsignedLong>`, `<string>`, `<Point>`, `<QuestTowerStats>`…); jagged
  arrays (`Awards`, `Trials/Levels`) nest `<ArrayOf…>` wrappers.
* Times are .NET ticks (`ticks_to_seconds`); 0 = no record.
* The Steam-backed counters (`TimesLaunched`, `MatchesPlayed`, …) are plain
  integers in the file (−1 if the game never synced them) — read as `i64`.
* Model: `options` (16 fields), `stats` (kill/death counters per archer and
  cause, wins, 70×9 awards, 5 variant presets, 16 tower plays, counters),
  `quest[14]`, `dark_world[5]`, `trials[16][3]`, `unlocks` (towers, archer
  reveal/intro flags, yellow gems as (tower, level), NEW variants),
  `keyboard[4]` (XNA key names per binding).
