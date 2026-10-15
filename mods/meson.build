# Mods that are a package and nothing else: what TKMM calls a mod, code
# included. Like the plugins, they are not part of a pack: they are built on
# demand, and the finished .tkcl goes into release/mods/.

# The SE edition of EnemyHp: the same data as the first edition
# (plugins/enemy-hp), with its code as an exefs module built with exlaunch
# and its settings as options of the package. Needs an extracted romfs
# (--romfs); the script says so when it is missing.
mod_se = run_target(
  'mod-se',
  command: [
    bash,
    utils / 'build-mod.sh',
    'EnemyHpSe',
    '--script', meson.current_source_dir() / 'enemy-hp-se' / 'build-mod.sh',
    '--romfs', get_option('totk_romfs'),
    '--out', release / 'mods' / 'EnemyHpSe',
  ],
)

# Both editions. Targets that write to the terminal run one at a time, so the
# packager they share is built once, by the first.
alias_target('mods', mod, mod_se)
