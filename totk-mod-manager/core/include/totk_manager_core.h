/*
 * totk-mod-manager core: the merger's code (mods, profiles, packages, merge)
 * as a C library. Implemented in Rust, see core/src/lib.rs.
 *
 * Returned text is UTF-8, JSON unless said otherwise, and must be released
 * with tkmc_free(). Functions returning "an error" return NULL on success.
 * Paths are spelled the plugin's way ("sd:/totk/mods/..."): tkmc_init() says
 * what sd:/ is on this platform.
 *
 * Not thread safe: call from one thread at a time.
 */

#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef void (*tkmc_log_fn)(const char *line, void *user);
/* stage: 0 reading mods (done/total mods), 3 comparing them (files several
 * mods change), 1 merging files, 2 writing. */
typedef void (*tkmc_progress_fn)(int stage, uint32_t done, uint32_t total, const char *item, void *user);
/* Gets {"conflicts": [{"kind": "file"|"values", "file", "count", "mods": [{"folder", "name"}],
 * "samples"}]} (mods winner first); returns whether to merge anyway. */
typedef bool (*tkmc_conflicts_fn)(const char *conflicts, void *user);

void tkmc_init(const char *sd_root, tkmc_log_fn log, void *user);
void tkmc_free(char *text);
void tkmc_free_bytes(uint8_t *data, size_t size);

/* {"config", "profiles", "profile": {"name", "exists", "mods"}, "mods", "problems"} */
char *tkmc_state(void);
/* Whether sd:/totk/merged holds the merge of the active profile as it is now.
 * Lists every folder mod's files: slow on large mods. */
bool tkmc_is_applied(void);
/* {"folder", "kind", "name", "version", "author", "description", "url", "option_groups",
 * "ini_options", "plugins": [file names], "load_plugins", "error"?} */
char *tkmc_mod_details(const char *folder);
/* Loads (or not) the Skyline plugins a mod ships; its files are merged either way. */
char *tkmc_mod_set_plugins(const char *folder, bool enabled);
/* The mod's image (thumbnail file or the one in its package). */
bool tkmc_mod_thumbnail(const char *folder, uint8_t **data, size_t *size);

bool tkmc_profile_name_valid(const char *name);
/* Writing a profile: begin, add mods winner first (options go to the last
 * mod added; an empty option records an empty selection), commit. */
void tkmc_profile_begin(const char *name);
void tkmc_profile_add_mod(const char *folder, bool enabled);
void tkmc_profile_add_option(const char *group, const char *option);
char *tkmc_profile_commit(void);
char *tkmc_profile_activate(const char *name);
char *tkmc_profile_delete(const char *name);
char *tkmc_profile_rename(const char *from, const char *to);
char *tkmc_config_set(const char *key, const char *value);
/* The manager's own preferences (sd:/totk/manager.ini): plain text, NULL when unset. */
char *tkmc_manager_get(const char *key);
char *tkmc_manager_set(const char *key, const char *value);
/* The conflicts the last merge found, as given to tkmc_conflicts_fn. */
char *tkmc_conflicts(void);
/* {"merges", "bytes"}: merges kept in sd:/totk/merged and the size of their files. Lists the store. */
char *tkmc_merge_cache_usage(void);

/* {"candidates": [{"kind", "path", "label", "has_code", "size"}]} */
char *tkmc_find_mods(const char *dir);
/* A FAT32-safe folder name (plain text). */
char *tkmc_folder_name(const char *name);
/* {"ok", "path"?, "error"?} */
char *tkmc_install(const char *source, const char *kind, const char *folder, const char *name,
                   const char *version, const char *author, const char *description, const char *url,
                   const char *thumbnail);
char *tkmc_uninstall(const char *folder);
/* {"moved": [...]} */
char *tkmc_migrate(void);
bool tkmc_remove_tree(const char *dir);

/* {"ok", "version", "nso", "error"?} */
char *tkmc_rom_info(const char *rom_prefix);
/* Blocks; conflicts may be NULL (merge anyway). {"ok", "reused", "from_cache", "cancelled", "conflicts", "mods",
 * "files", "served", "warnings", "patches", "seconds", "error"} */
char *tkmc_apply(const char *rom_prefix, tkmc_progress_fn progress, tkmc_conflicts_fn conflicts, void *user);

#ifdef __cplusplus
}
#endif
