# The merger: a Skyline plugin that merges the mods of sd:/totk/mods and
# serves them to the game.

merger_nro = custom_target(
  'totk-mod-merger-plugin.nro',
  output: 'totk-mod-merger-plugin.nro',
  command: [bash, utils / 'build-nro.sh', 'totk-mod-merger-plugin', '--out', '@OUTPUT@'],
  console: true,
  build_always_stale: true,
  build_by_default: true,
)

# Shipped as sd:/totk/config.ini.
merger_config = files('config.example.ini')
