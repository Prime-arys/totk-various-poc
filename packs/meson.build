# The two packs: the same four files, laid out where a console and where
# Ryujinx expect them. No mods — those are built apart.
#
# They are the end of the line, so they are written straight into release/:
# everything they are made of has been built in output/ by the targets above.

pack_switch = run_target(
  'pack-switch',
  command: [
    bash, utils / 'make-pack.sh',
    '--kind', 'switch',
    '--title-id', title_id,
    '--out', release / 'pack-switch',
    '--subsdk9', skyline_exefs[0],
    '--npdm', skyline_exefs[1],
    '--merger', merger_nro,
    '--manager', manager_nros[0],
    '--skyline-config', skyline_config,
    '--merger-config', merger_config,
    '--readme', files('readme-switch.md'),
    '--readme-fr', files('lisezmoi-switch.md'),
    zip,
  ],
)

pack_emulator = run_target(
  'pack-emulator',
  command: [
    bash, utils / 'make-pack.sh',
    '--kind', 'emulator',
    '--title-id', title_id,
    '--out', release / 'pack-emulator',
    '--subsdk9', skyline_exefs[0],
    '--npdm', skyline_exefs[1],
    '--merger', merger_nro,
    '--manager', manager_nros[1],
    '--skyline-config', skyline_config,
    '--merger-config', merger_config,
    '--readme', files('readme-emulator.md'),
    '--readme-fr', files('lisezmoi-emulateur.md'),
    zip,
  ],
)

alias_target('packs', pack_switch, pack_emulator)
