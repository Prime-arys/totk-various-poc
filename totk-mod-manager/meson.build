# The homebrew that manages the mods: its Rust core plus a borealis
# application. The emulator flavour is the same program with the two
# instructions Ryujinx' JIT refuses patched out.

manager_nros = custom_target(
  'totk-mod-manager',
  output: ['totk-mod-manager.nro', 'totk-mod-manager-ryujinx.nro'],
  command: [bash, utils / 'build-manager.sh', '--out', '@OUTDIR@'],
  console: true,
  build_always_stale: true,
  build_by_default: true,
)
