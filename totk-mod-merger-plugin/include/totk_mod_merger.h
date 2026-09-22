/*
 * totk_mod_merger.h - API of totk-mod-merger-plugin (API version 1)
 *
 * Lets another Skyline plugin decide which mods are merged and served to
 * Tears of the Kingdom, e.g. an online mode that downloads the mod pack every
 * player must run.
 *
 * How it works
 * ------------
 * The merger runs once, after every plugin's `main` has returned (skyline-totk
 * calls it through `skyline_totk_on_plugins_loaded`), while the game is still
 * blocked on its romfs mount. A plugin that wants to choose the mods calls
 * `tkm_take_control` from its own `main`; the merge then waits until that
 * plugin calls `tkm_commit` or `tkm_release_control`, or its timeout expires.
 * The work in between (downloading, unpacking...) can happen on a thread of
 * the plugin's own.
 *
 * While a plugin has control:
 *   - the mods on the SD card (sd:/totk/mods) are NOT merged, unless the
 *     plugin calls tkm_set_local_mods_enabled(token, true);
 *   - only the mods it adds are merged, in the order they were added (later
 *     ones win conflicts), above the SD card's mods if those are enabled;
 *   - the merge goes to its own folder ("<merged_dir>-<owner>" by default),
 *     so the SD card's merge stays cached for when the plugin is not used.
 * If the timeout expires, whatever was added so far is merged.
 *
 * Mods can be `.tkcl` packages (TKMM), folders holding `romfs`/`exefs`, or
 * bare romfs folders.
 *
 * Plugins mods ship
 * -----------------
 * A mod folder can hold a `plugin.nro` of its own (or several, in a `plugins`
 * folder). The merger loads those once the merge is served, in merge order,
 * and runs their `main` — which is how code mods coexist, since a console has
 * room for only one exefs and Skyline is already in it.
 *
 * Such a plugin is an ordinary Skyline plugin. From its `main` it can call
 * tkm_current_mod_dir() to find the folder it came from (its own settings,
 * its own assets). It is too late for it to take control of the mod list: a
 * plugin that chooses the mods belongs in skyline/plugins, which is loaded
 * before the merge.
 *
 * Finding the functions
 * ---------------------
 * Plugins are loaded in no particular order, so resolve the functions at run
 * time rather than linking against them:
 *
 *     uintptr_t address = 0;
 *     if (nn::ro::LookupSymbol(&address, "tkm_take_control") == 0 && address) {
 *         auto take_control = (uint64_t (*)(const char*, uint32_t))address;
 *         ...
 *     }
 *
 * Every function is safe to call from any thread. Strings are UTF-8 and
 * NUL-terminated; they are copied, so they need not outlive the call.
 */

#ifndef TOTK_MOD_MERGER_H
#define TOTK_MOD_MERGER_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define TKM_API_VERSION 1

/* Merge states, as returned by tkm_get_state and passed to callbacks. */
typedef enum {
    TKM_STATE_WAITING = 0, /* mods can still be chosen */
    TKM_STATE_MERGING = 1,
    TKM_STATE_DONE = 2,    /* merged files are being served */
    TKM_STATE_FAILED = 3,  /* nothing is served (error, or the merger is disabled) */
} tkm_state;

/* Version of this API implemented by the loaded merger. Compare with
 * TKM_API_VERSION before using anything else. */
uint32_t tkm_api_version(void);

/* While a mod's plugin runs its `main`: the folder it came from, e.g.
 * "sd:/totk/mods/EnemyHp". NULL at any other time, and for plugins loaded
 * from skyline/plugins. Copy it if you need it later. */
const char* tkm_current_mod_dir(void);

/* The name of that mod, as the mod list shows it. NULL outside a mod
 * plugin's `main`. */
const char* tkm_current_mod_name(void);

/* Takes control of the mod list. `owner` names the caller in logs and in the
 * default merge folder. The merge waits at most `timeout_ms` (capped by the
 * user's `control_timeout_ms` setting) for tkm_commit.
 * Returns a non-zero token for the calls below, or 0 when another plugin
 * already has control or the merge has started. */
uint64_t tkm_take_control(const char* owner, uint32_t timeout_ms);

/* Gives control back: the SD card's mods are merged as if nothing happened. */
bool tkm_release_control(uint64_t token);

/* Also merge the SD card's mods (below the ones added here). Off by default. */
bool tkm_set_local_mods_enabled(uint64_t token, bool enabled);

/* Removes every mod added so far. */
bool tkm_clear_mods(uint64_t token);

/* Adds a mod: a `.tkcl` file, a folder with `romfs`, or a romfs folder, e.g.
 * "sd:/totk/online/packs/pvp.tkcl". `name` may be NULL.
 * Returns the mod's index (for tkm_select_option), or -1. */
int32_t tkm_add_mod(uint64_t token, const char* path, const char* name);

/* Selects an option of a package added with tkm_add_mod. Call once per
 * selected option; groups that are never mentioned keep their defaults. */
bool tkm_select_option(uint64_t token, int32_t mod_index, const char* group, const char* option);

/* Folder the merged files (and the cache that makes the next boot instant)
 * are written to. Must not be shared with another mod set. */
bool tkm_set_merged_dir(uint64_t token, const char* dir);

/* The mod list is final: the merge may start. */
bool tkm_commit(uint64_t token);

/* Current tkm_state. */
int32_t tkm_get_state(void);

/* Number of game files served from the merge (valid once DONE). */
uint32_t tkm_get_served_files(void);

/* Calls `callback(state, served_files, user)` once the merge is over, or right
 * away if it already is. The callback runs on the merger's thread, before the
 * game resumes: keep it short. */
typedef void (*tkm_merged_callback)(int32_t state, uint32_t served_files, void* user);
bool tkm_on_merged(tkm_merged_callback callback, void* user);

#ifdef __cplusplus
}
#endif

#endif /* TOTK_MOD_MERGER_H */
