project(
  'totk-various-poc',
  version: '0.1.0',
  meson_version: '>= 1.1.0',
)

# Meson compiles nothing here: each part has a toolchain of its own (devkitPro
# for the C++ side, the Skyline Rust toolchain for the plugins, .NET for the
# mod packager). What meson knows is the list of targets, their order, and how
# a pack is laid out; the scripts in utils/ do the building.
#
# It runs inside the repository's container, where all of that is installed —
# ./build.sh sets it up and calls it. Nothing here looks for a toolchain or a
# shell: the image has exactly one of each.

bash = find_program('bash')
utils = meson.project_source_root() / 'utils'

# The two directories the whole repository writes to. This build tree is
# itself inside output/ (output/build), so everything transient ends up
# there; release/ only ever receives finished things.
release = meson.project_source_root() / 'release'

title_id = get_option('title_id')
zip = get_option('pack_zip') ? ['--zip'] : []

subdir('skyline-totk')            # skyline_exefs, skyline_config
subdir('totk-mod-merger-plugin')  # merger_nro, merger_config
subdir('totk-mod-manager')        # manager_nros
subdir('plugins')                 # enemy_hp_nro, online_example_nro, plugins, mod
subdir('packs')                   # pack-switch, pack-emulator, packs

# What the image has, and what it should have: `./build.sh doctor`.
run_target('doctor', command: [bash, utils / 'doctor.sh'])

# The crates that run on a PC as well as on the console are tested there.
test(
  'crates',
  bash,
  args: [utils / 'run-tests.sh'],
  timeout: 1200,
  workdir: meson.project_source_root(),
)

summary(
  {
    'title id': title_id,
    'packs also zipped': get_option('pack_zip'),
    'TotK romfs (for mods)': get_option('totk_romfs') != '' ? get_option('totk_romfs') : 'not set (--romfs)',
    'everything transient': meson.project_build_root(),
    'finished things': release,
  },
  section: 'Configuration',
)
summary(
  {
    'the two packs': './build.sh',
    'one of them': './build.sh pack-switch | pack-emulator',
    'the example plugins': './build.sh plugins',
    'the EnemyHp mod': './build.sh --romfs <romfs> mod',
    'the toolchain of the image': './build.sh doctor',
    'the tests of the crates': './build.sh test',
    'throwing it all away': './build.sh clean [--all]',
  },
  section: 'Targets',
)
