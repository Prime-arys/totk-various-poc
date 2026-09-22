# Plugins are not part of a pack: one is dropped in skyline/plugins by hand,
# the other is shipped inside a mod. They are built on demand, and the finished
# .nro files are copied into release/plugins/.

enemy_hp_nro = custom_target(
  'enemy-hp.nro',
  output: 'enemy-hp.nro',
  command: [bash, utils / 'build-nro.sh', 'enemy-hp', '--out', '@OUTPUT@'],
  console: true,
  build_always_stale: true,
  build_by_default: false,
)

online_example_nro = custom_target(
  'online-example.nro',
  output: 'online-example.nro',
  command: [bash, utils / 'build-nro.sh', 'online-example', '--out', '@OUTPUT@'],
  console: true,
  build_always_stale: true,
  build_by_default: false,
)

run_target(
  'plugins',
  command: [
    bash, utils / 'release-copy.sh',
    '--out', release / 'plugins',
    enemy_hp_nro,
    online_example_nro,
  ],
)

# The whole EnemyHp mod: its data read out of the game's own files, packaged
# as a .tkcl, plus the plugin above. Needs an extracted romfs (--romfs); the
# script says so when it is missing.
run_target(
  'mod',
  command: [
    bash,
    utils / 'build-enemy-hp-mod.sh',
    '--romfs', get_option('totk_romfs'),
    '--out', release / 'mods' / 'EnemyHp',
  ],
)
