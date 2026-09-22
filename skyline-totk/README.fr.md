# skyline-totk

*[English version](README.md)*

[Skyline](https://github.com/skyline-dev/skyline) adapté à **The Legend of Zelda: Tears of the Kingdom**
(`0100F2C0115B6000`).

Skyline est un environnement de chargement de code : il s'injecte dans le jeu sous forme de
`subsdk9`, installe des hooks, et charge des plugins (`.nro`) depuis la carte SD — typiquement des
plugins écrits en Rust avec [`cargo-skyline`](https://crates.io/crates/cargo-skyline) ou en C++.

## Ce qui change par rapport au Skyline d'origine

| Sujet | Skyline (SSBU) | skyline-totk |
|---|---|---|
| NPDM | `cross.npdm` figé pour Smash | généré depuis le `main.npdm` de TotK (`npdm/totk.json`), permissions élargies |
| Init | tout au chargement du module (logger, sockets, 6 Mo de pool réseau) | rien n'alloue avant que le romfs soit monté ; seuls les hooks sont posés au démarrage |
| Logger | kernel + TCP en dur | configurable (`kernel`, `sd`, `tcp`) via `config.ini`, SD par défaut |
| Threads | cœur 3 codé en dur | cœur par défaut du process (TotK n'autorise que 0-2 — l'ancien code échouait) |
| Build | `CROSSVER`, patches IPS Smash | build unique, sortie prête à copier sur SD |
| ABI plugin | — | ajoute `totk_get_version`, `totk_get_version_string`, `totk_get_rom_mount` |

## Prérequis

- **Console** : Atmosphère avec les *sigpatches* (le `main.npdm` personnalisé n'est accepté que si
  la vérification de signature ACID est désactivée dans le loader).
- **Build** : devkitPro (`devkitA64`, `libnx`, `npdmtool`) et Python 3 — tout cela est dans
  l'image du dépôt, qui construit ce dossier avec `./build.sh skyline-totk` à la racine
  (voir [docs/build.fr.md](../docs/build.fr.md)).

## Compilation

Depuis la racine du dépôt, dans le conteneur :

```bash
./build.sh skyline-totk
```

Seul, dans un shell qui a devkitPro (`./build.sh shell` à la racine), avec le `build.sh` de ce
dossier-ci :

```bash
cd skyline-totk
./build.sh package OUT_ROOT=/work/output/skyline
```

`build.sh` positionne `DEVKITPRO` et appelle `make`. Sans `OUT_ROOT`, tout est écrit à côté des
sources (`build/`, `out/`) ; avec, rien n'y est écrit. Résultat dans `$OUT_ROOT/out/` :

```
out/atmosphere/contents/0100F2C0115B6000/
├── exefs/
│   ├── main.npdm     <- NPDM du jeu + permissions Skyline
│   └── subsdk9       <- Skyline
└── skyline/plugins/   <- déposez vos plugins .nro ici
```

Les plugins sont cherchés dans `sd:/atmosphere/contents/0100F2C0115B6000/skyline/plugins/`, puis
(compatibilité) dans `romfs:/skyline/plugins/`. Préférez le premier : dès qu'un dossier `romfs`
existe pour le titre, Atmosphère construit un romfs LayeredFS par-dessus les ~300 000 fichiers de
TotK à chaque démarrage, ce qui est lent et peut empêcher le jeu de démarrer (mémoire de fs.mitm)
sur les firmwares récents. Le log signale les plugins encore chargés depuis le romfs.

Copiez le dossier `out/atmosphere` à la racine de la carte SD (il fusionne avec l'existant).

Autres cibles :

```bash
./build.sh clean
./build.sh install SD=/d            # copie directe sur une SD montée
./build.sh dump-npdm GAME_NPDM=/chemin/vers/exefs/main.npdm   # régénère npdm/totk.json
```

## Configuration (optionnelle)

Fichier `sd:/skyline/totk/config.ini` :

```ini
# Sorties de log : none | kernel | sd | tcp (combinables : "sd,tcp"), ou "all"
log = sd
log_path = sd:/skyline/totk/skyline.log
tcp_port = 6969
# Charger les plugins
plugins = 1
# Dossier des plugins sur la SD (le romfs est consulté ensuite)
plugins_dir = sd:/atmosphere/contents/0100F2C0115B6000/skyline/plugins
```

Sans fichier, les valeurs ci-dessus s'appliquent telles quelles. Le log SD est réécrit à chaque
démarrage. Avec `tcp`, Skyline prend la main sur la pile réseau du jeu (`nn::socket`) : à n'activer
que pour du debug, avec `cargo skyline listen <ip>` en face.

## Comment ça démarre

1. Le loader d'Atmosphère charge `subsdk9` en plus des NSO du jeu, avec le `main.npdm` fourni.
2. `rtld` résout nos imports `nn::*` contre les modules du jeu (`main`, `sdk`) puis appelle notre
   `DT_INIT` (`__custom_init` → `skyline_init`).
3. À cet instant l'allocateur du jeu n'est pas garanti prêt : on se limite à récupérer notre handle
   de process, initialiser le moteur de hooks (qui passe par les svc JIT, pas par le tas) et poser
   deux hooks — `nn::fs::MountRom` et `nn::ro::Initialize`.
4. Quand le jeu monte son romfs, le hook lance un thread de travail qui monte la SD, lit la config,
   démarre le logger, détecte la version du jeu, puis charge les plugins via `nn::ro`.
   Le thread appelant attend la fin, donc les plugins sont prêts avant que le jeu ne lise un fichier.
5. Une fois tous les `main` exécutés, les callbacks enregistrés avec
   `skyline_totk_on_plugins_loaded` sont appelés — toujours avant que le jeu ne reprenne.

## Écrire un plugin

N'importe quel plugin Skyline standard fonctionne :

```bash
cargo skyline new mon-plugin
# puis déposer le .nro dans atmosphere/contents/0100F2C0115B6000/skyline/plugins/
```

En plus de l'ABI Skyline habituelle (`A64HookFunction`, `A64InlineHook`, `sky_memcpy`,
`getRegionAddress`, `get_program_id`, `skyline_tcp_send_raw`), ce fork exporte :

```c
uint32_t    totk_get_version();         // 10201 pour 1.2.1
const char* totk_get_version_string();  // "1.2.1"
const char* totk_get_rom_mount();       // "content:/" sur TotK

// Mémoire de travail prise au noyau (svcMapPhysicalMemory), pas au tas du jeu
void* totk_map_memory(uint64_t size);
bool  totk_unmap_memory(void* address, uint64_t size);

// Appelle callback(user) quand tous les plugins ont exécuté leur main (tout de suite si c'est
// déjà le cas, et renvoie alors false). Permet à un plugin d'attendre que les autres aient pu
// l'utiliser — c'est ainsi que totk-mod-merger laisse un autre plugin choisir les mods.
bool skyline_totk_on_plugins_loaded(void (*callback)(void*), void* user);

// Initialise la pile réseau (nn::socket) pour les plugins qui en ont besoin dès le démarrage,
// par exemple pour télécharger un pack de mods. Idempotent.
bool skyline_totk_init_sockets();
```

**Pourquoi `totk_map_memory`** : quand les plugins démarrent, l'allocateur de TotK n'a que
quelques Mo disponibles (mesuré sur 1.2.1 : des tampons de 1, 2 et 4 Mio passent, 8 Mio non).
Un plugin qui manipule de gros fichiers doit prendre sa mémoire ailleurs — ces deux fonctions
la mappent depuis le noyau, dans la région *alias* du process.

Deux symboles de compatibilité sont aussi exportés, `__nnmusl_ErrnoLocation` et `__pthread_join` :
`libc-nnsdk` (utilisé par les plugins Rust) les réclame, mais le SDK 15.3.1 de TotK ne les exporte
plus sous ces noms. Sans eux, un plugin Rust qui touche à `errno` ou aux threads ne se charge pas.

Voir `totk-mod-merger-plugin/` pour un exemple complet, et son `plugins/online-example` pour un
plugin qui utilise le réseau et l'API d'un autre plugin.

Pièges connus des plugins Rust sur ce SDK :

- ne jamais laisser tomber le `JoinHandle` d'un thread : cela appelle `pthread_detach`, qui fait
  planter nnSdk (joindre le thread, ou `std::mem::forget` le handle) ;
- pas de `std::sync::Mutex` dans un hook appelé par plusieurs threads du jeu : en cas de contention,
  nnSdk abandonne le processus (`ArbitrateLock` → handle invalide). Un spinlock convient ;
- les piles de threads sont prises au tas du jeu : 4 Mio peuvent être refusés, 2 Mio passent.

## État

Vérifié sur **TotK 1.2.1** (sous Ryujinx, qui applique les mods exefs comme la console) :
`subsdk9` et le `main.npdm` sont acceptés, les hooks s'installent, la carte SD est montée, la
version du jeu est détectée (`1.2.1`), le plugin est chargé via `nn::ro` et exécuté, puis le jeu
démarre normalement. Traces correspondantes dans `sd:/skyline/totk/skyline.log` :

```
[SdLogger] Logger initialized.
[skyline-totk] Tears of the Kingdom 1.2.1 (code 10201)
[skyline-totk] romfs mounted at 'content:/'
[PluginManager] Loaded 'sd:/atmosphere/contents/0100F2C0115B6000/skyline/plugins/totk-mod-merger-plugin.nro'
[PluginManager] Running plugins-loaded callback 0
```

Le test sur matériel réel reste à faire (Atmosphère + sigpatches).

## Vérifier une nouvelle version du jeu

Les symboles `nn::*` sont résolus dynamiquement : rien n'est codé en dur, donc une mise à jour du
jeu ne casse pas Skyline tant que les symboles existent. Pour vérifier après une mise à jour :

```bash
# 1. extraire l'exefs de la mise à jour
nstool -k prod.keys --tik <ticket> -x /0 exefs <program.nca>
# 2. régénérer le NPDM
./build.sh dump-npdm GAME_NPDM=exefs/main.npdm && ./build.sh
# 3. comparer les symboles importés par subsdk9 à ceux exportés par le jeu
python3 scripts/check_symbols.py exefs skyline-totk.elf
```

## Crédits

- [Skyline](https://github.com/skyline-dev/skyline) — base du projet
- [exlaunch](https://github.com/shadowninja108/exlaunch) — référence pour l'injection moderne
- [cucholix/TotK-graphic-plugin](https://github.com/cucholix/TotK-graphic-plugin) — première
  adaptation de Skyline à TotK
