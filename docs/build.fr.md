# Construire

*[English version](build.md)*

Tout se construit dans un conteneur. La machine n'a besoin que de **Docker** (Docker Desktop sur
Windows et macOS, le moteur docker sur Linux) et de **git** pour le clone ; rien d'autre n'est
installé, et rien d'autre n'est utilisé — les chaînes d'outils de la machine, s'il y en a, ne
servent pas.

```bash
git clone <ce dépôt> && cd totk-various-poc
./build.sh                  # ou, depuis PowerShell : .\build.ps1
```

La première exécution fait trois choses : récupérer les sous-modules, construire l'image
(quelques minutes, une seule fois), puis construire les deux packs.

## Les deux dossiers

| Dossier | Ce qu'il contient |
|---|---|
| `output/` | tout ce qui est temporaire. `output/build` est le dossier Meson, `output/cargo` la sortie de cargo, `output/skyline` les objets et le `.nso` de skyline-totk, `output/manager` cmake + borealis + le cœur Rust, `output/dotnet` les sorties .NET de l'empaqueteur et de TkSharp, `output/tmp` les fichiers de passage. |
| `release/` | ce qui est fini : `release/pack-switch`, `release/pack-emulator`, `release/plugins/*.nro`, `release/mods/<mod>/`. |

Aucun autre dossier du dépôt n'est écrit. C'est ce qui rend le nettoyage évident :

```bash
./build.sh clean            # vide output/
./build.sh clean --all      # vide output/ et release/
```

Rien n'est effacé d'autre : les sources, les sous-modules et l'image Docker restent. Pour
l'image : `docker image rm totk-various-poc`.

## Les cibles

```bash
./build.sh                          # packs : les deux, dans release/
./build.sh pack-switch              # release/pack-switch/
./build.sh pack-emulator            # release/pack-emulator/
./build.sh plugins                  # release/plugins/{enemy-hp,online-example}.nro
./build.sh --romfs <dossier> mod    # release/mods/EnemyHp/
./build.sh test                     # les tests des crates, sur le PC
./build.sh doctor                   # ce que contient l'image
./build.sh shell                    # un shell dans le conteneur
```

Chaque nom est une cible Meson : ce qui suit `build.sh` lui est passé tel quel, donc
`./build.sh skyline-totk` ou `./build.sh totk-mod-manager` construisent un morceau seul. La liste
complète s'affiche à la fin de `meson setup`, dans le résumé.

Les options :

| Option (bash) | Option (PowerShell) | Effet |
|---|---|---|
| `--romfs <dossier>` | `-Romfs <dossier>` | un romfs de TotK extrait, monté en lecture seule, nécessaire pour construire un mod |
| `--zip` | `-Zip` | écrit aussi un `.zip` à côté de chaque pack |
| `--title-id <id>` | `-TitleId <id>` | un autre identifiant de jeu que celui de TotK |
| `--rebuild-image` | `-RebuildImage` | reconstruit l'image (après une modification du `Dockerfile`) |
| `--no-dotnet` | `-NoDotnet` | avec le précédent : image sans le SDK .NET, ~1 Gio de moins, mais plus de `.tkcl` |
| `-- <commande>` | `-Run '<commande>'` | exécute autre chose dans le conteneur |

Les options passées à Meson (`--romfs`, `--zip`, `--title-id`) sont mémorisées dans
`output/build` : une fois données, elles restent jusqu'à ce qu'on les change ou qu'on nettoie.

## Ce que fait chaque cible

| Cible | Ce qui tourne | Où ça travaille |
|---|---|---|
| `skyline-totk` | le `Makefile` de skyline-totk, devkitA64, `npdmtool`, `elf2nso` | `output/skyline` |
| `totk-mod-merger-plugin.nro` | cargo avec la toolchain `skyline-v3`, puis `linkle` | `output/cargo` |
| `totk-mod-manager` | cargo (cœur `no_std`), puis cmake + make (borealis, deko3d) | `output/manager` |
| `enemy-hp.nro`, `online-example.nro` | comme le fusionneur | `output/cargo` |
| `mod` | les exemples de `totk-merge` lisent le romfs, `tkmm-oracle` empaquette le `.tkcl` | `output/dotnet`, `output/tmp` |
| `pack-switch`, `pack-emulator` | assemblage des fichiers déjà construits | écrit dans `release/` |

Meson ne compile rien lui-même : il connaît les cibles, leur ordre et la disposition des packs,
et appelle les scripts de [`utils/`](../utils). Ceux-ci supposent le conteneur (devkitPro dans
`/opt/devkitpro`, Rust dans `/opt/rust`) — c'est pour cela qu'ils n'ont plus rien pour chercher
un shell, convertir un chemin ou trouver un dossier temporaire inscriptible.

## Les packs

```text
pack-switch/SD/                                   pack-emulator/Ryujinx/
├── atmosphere/contents/<jeu>/                    ├── mods/contents/<jeu>/skyline-totk/
│   ├── exefs/{subsdk9,main.npdm}                 │   └── exefs/{subsdk9,main.npdm}
│   └── skyline/plugins/                          └── sdcard/
│       └── totk-mod-merger-plugin.nro                ├── atmosphere/…/skyline/plugins/…nro
├── skyline/totk/config.ini                           ├── skyline/totk/config.ini
├── switch/totk-mod-manager.nro                       ├── switch/totk-mod-manager-ryujinx.nro
└── totk/{config.ini, mods/}                          └── totk/{config.ini, mods/}
```

Chaque pack est reconstruit à partir de zéro et contient une notice (`README.md`, et
`LISEZMOI.md` en français) et un `contenu.txt` qui liste les fichiers avec leur taille et leur
empreinte MD5 — pratique pour vérifier ce qui a été installé quand un essai tourne mal.

Le pack émulateur emporte la variante Ryujinx du homebrew : le même programme, avec les deux
instructions ARMv9 que son JIT refuse remplacées (voir `totk-mod-manager/build.sh`).

**Aucun mod n'est dans un pack** : ils se construisent séparément ([mods.fr.md](mods.fr.md)) et se
déposent dans `totk/mods/`.

## L'image

| Outil | Pourquoi |
|---|---|
| devkitPro (`devkitpro/devkita64`) + `switch-dev`, `uam` | `skyline-totk` et le homebrew |
| `switch-glm`, `switch-curl`, `switch-libarchive` | les bibliothèques du homebrew |
| cmake, ninja, make, meson, python3, git | la construction elle-même |
| rustup + la nightly du cœur du manager | `totk-mod-manager/core` |
| `cargo-skyline` (chaîne `skyline-v3`) et `linkle` | les plugins Skyline |
| SDK .NET 10 (facultatif) | `utils/tkmm-oracle`, l'empaqueteur `.tkcl` |

Le [`Dockerfile`](../Dockerfile) ne copie rien : le dépôt est monté à `/work` au lancement. Il ne
change donc que lorsqu'une chaîne d'outils change, et `./build.sh --rebuild-image` suffit à le
répercuter. `./build.sh doctor` dit ce que l'image contient, ligne par ligne.

La chaîne Skyline est reconstruite par `cargo skyline update-std` au moment de la construction de
l'image : c'est une nightly de Rust plus la bibliothèque standard de `skyline-rs`. Cette
étape-là interroge github, qui limite le nombre de requêtes par adresse ; le `Dockerfile`
réessaie chaque téléchargement avec une attente croissante, et s'il échoue quand même, réessayer
plus tard suffit.

Sous Linux et macOS, `build.sh` passe `--user $(id -u):$(id -g)` pour que les fichiers produits
appartiennent à celui qui a lancé la commande. Sous Windows, Docker Desktop s'en charge.

## Dépannage

| Symptôme | Cause habituelle |
|---|---|
| `docker not found` | installer Docker Desktop (Windows, macOS) ou le moteur docker (Linux) |
| `No space left on device` alors que `df` semble large | c'est le disque de la machine virtuelle Docker : `docker builder prune` et `docker image prune` le libèrent |
| `retrying (n): cargo skyline update-std` pendant la construction de l'image, puis échec | github limite les requêtes de l'adresse ; recommencer plus tard |
| CMake ne trouve pas borealis | les sous-modules sont vides : `git submodule update --init` (`build.sh` le fait tout seul quand il voit le dossier vide) |
| `--romfs must point at an extracted TotK romfs` | `--romfs` manquant, ou le dossier indiqué n'existe pas |
| `error: the build left no ...tkmm-oracle.dll` | image construite avec `--no-dotnet` : `./build.sh --rebuild-image` |
| un `meson setup` qui ignore une option | `meson setup` sur un dossier déjà configuré sort sans erreur en ignorant les options ; `build.sh` le sait et passe par `meson configure` — mais si tu lances Meson à la main, c'est le piège à connaître |
| `cc1plus: ... Cannot allocate memory` en compilant borealis | trop de compilateurs en parallèle pour la mémoire de la machine virtuelle Docker. Le script en lance un par 1,5 Gio de mémoire ; pour forcer : `./build.sh -- bash -lc 'JOBS=4 meson compile -C output/build totk-mod-manager'`, ou donner plus de mémoire à Docker Desktop |
| la commande ne rend jamais la main alors que tout est construit | un outil a laissé un démon derrière lui, qui a hérité du tuyau par lequel ninja lit sa commande : ninja attend alors une fin de fichier qui ne vient pas. `dotnet build` faisait exactement ça (quatre nœuds MSBuild et le serveur Roslyn) ; `utils/build-enemy-hp-mod.sh` les désactive et écrit la sortie du build dans un fichier plutôt que dans un tuyau |
| le homebrew plante au lancement sous Ryujinx | version console utilisée : prendre `totk-mod-manager-ryujinx.nro` |

## Sans `build.sh`

Le lanceur n'est qu'un raccourci ; ces deux commandes font la même chose :

```bash
docker build -t totk-various-poc .
docker run --rm -v "$PWD:/work" -w /work totk-various-poc \
    bash -lc 'meson setup output/build && meson compile -C output/build packs'
```

Et dans un shell du conteneur (`./build.sh shell`), les scripts de `utils/` marchent seuls :

```bash
utils/doctor.sh
utils/build-skyline.sh --out /tmp/exefs
utils/build-nro.sh totk-mod-merger-plugin --out /tmp/merger.nro
utils/build-manager.sh --out /tmp/manager
utils/build-enemy-hp-mod.sh --romfs /romfs --out /tmp/EnemyHp
```
