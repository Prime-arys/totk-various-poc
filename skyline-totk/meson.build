# Skyline adapted to Tears of the Kingdom: the exefs pair every pack starts
# with. Built by its own Makefile, which compiles into output/skyline.

skyline_exefs = custom_target(
  'skyline-totk',
  output: ['subsdk9', 'main.npdm'],
  command: [bash, utils / 'build-skyline.sh', '--out', '@OUTDIR@', '--title-id', title_id],
  console: true,
  build_always_stale: true,
  build_by_default: true,
)

# Shipped as sd:/skyline/totk/config.ini.
skyline_config = files('config.example.ini')
