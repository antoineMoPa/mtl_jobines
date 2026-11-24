# Agent instructions

- Keep map construction in `.road` files and application behavior in Rust. Use the Rust editor.
  MCP/socket API for external automation, including clients written in Python outside this
  repository.
- Avoid normalization functions. Use explicit mapping functions or enums when values need
  conversion.
- Do not ever add real brand or people names in the source code or artworks.
- Do not introduce new programming or scripting languages to the repo like python - stick to rust.
  Definitely feel free to use these in temp folders!


# Workflow
- Do all your edits for a turn + compile and format. Avoid small successive edits and compiles,
  these are slow.

# Cleanup

Run cargo `xtask --staged` once done with a task to detect files to clean.

# Converting vehicle models

Clean it up like we did for other medium poly models:

- cut model in half and use symmetry to reconstruct clean body.
- remove wheels, use our clean wheels, make a low poly interior (10-15 primitives max), separate
  window meshes carefully
- define vehicle parts and lights use claydash mcp or any tool you'd like work in the moontask
  folder or a temporary folder.
- separate general body vertices to their own group  then include the glb + car in game

# Compile at the end of your changes

This is a large rust projects, multiple agents might be working.

Change all of your files for your feature to batch compile rather than
making small successive edits and compiles.

Cargo errors have the benefit of showing all erros.