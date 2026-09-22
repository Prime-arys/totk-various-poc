# totk-various-poc

*[English version](README.md)*

Ce dépôt contient une **expérimentation** entièrement **vibe-codée** pour le modding de *The Legend
of Zelda: Tears of the Kingdom* (**v1.2.1**). Il a été fait pour plusieurs raisons :

1. Changer de mods facilement sur une vraie console, sans avoir besoin d'une copie de 16 Go du jeu
   sur la carte SD.
2. Permettre à plusieurs mods exefs de coexister, quand les patchs IPS et les cheats ne suffisent
   plus.
3. Construire le mod avec lequel je voulais jouer : **EnemyHp**, qui affiche les PV des ennemis en
   chiffres (comme dans BotW) et les leur fait regagner avec le temps.

Pour y arriver, le dépôt contient un **Skyline** adapté à TotK, un **homebrew** qui gère les mods
et lance le jeu, un **plugin fusionneur** qui fusionne les mods et sert les fichiers au jeu, et
quelques mods et plugins d'exemple.

Tout est écrit pour la version **1.2.1** du jeu, et reste une expérimentation :
la compatibilité complète avec les mods existants et avec toutes les fonctions de TKMM n'est pas garantie.

## Ce que contient le dépôt


| Dossier                                              | Ce que c'est                                                                                                                                                                                                                                         |
| ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| [`skyline-totk/`](skyline-totk/)                     | Skyline adapté à TotK 1.2.1 : l'`exefs` (`subsdk9` + `main.npdm`) qui charge les plugins, avec son journal sur la carte SD, sa mémoire mappée et ses crochets. C++ / devkitPro.                                                                  |
| [`totk-mod-merger-plugin/`](totk-mod-merger-plugin/) | Le fusionneur : un plugin Skyline qui lit les mods de`sd:/totk/mods`, les fusionne (portage de TKMM) et sert les fichiers au jeu. Il charge aussi le `plugin.nro` que fournit un mod. Rust.                                                          |
| [`totk-mod-manager/`](totk-mod-manager/)             | Le homebrew (borealis) qui gère mods, profils, options et conflits, lance la fusion et le jeu. C++ pour l'interface, Rust (`core/`) pour la logique partagée.                                                                                      |
| [`sharedlibs/`](sharedlibs/)                         | Les crates communes :`totk-formats` (SARC, BYML, RSTB, zstd), `totk-merge` (découverte des mods et fusion), `totk-mod-merger-api` (l'API `tkm_*` pour les autres plugins). Elles se compilent avec ou sans `std`, pour la console comme pour le PC. |
| [`plugins/`](plugins/)                               | Deux plugins d'exemple :`enemy-hp` (le code du mod EnemyHp : PV des ennemis, régénération) et `online-example` (impose au fusionneur un pack téléchargé depuis un PC).                                                                         |
| [`packs/`](packs/)                                   | La description des deux packs et les notices qu'ils emportent.                                                                                                                                                                                       |
| [`utils/`](utils/)                                   | Les scripts que Meson appelle dans le conteneur,`tkmm-oracle` (le packageur de TKMM) et le sous-module `TkSharp`.                                                                                                                                    |
| [`docs/`](docs/)                                     | La construction, et les mods.                                                                                                                                                                                                                        |

Les emprunts, les portages et les références sont listés dans **[CREDITS.fr.md](CREDITS.fr.md)** —
Skyline, borealis, TKMM/TkSharp et les autres.

Le code tiers arrive par **deux sous-modules git**, que `build.sh` récupère tout seul s'ils
manquent :


| Sous-module                                       | Où                                 | Version épinglée      | Pour quoi                                                                                            |
| --------------------------------------------------- | ------------------------------------- | ------------------------- | ------------------------------------------------------------------------------------------------------ |
| [borealis](https://github.com/xfangfang/borealis) | `totk-mod-manager/library/borealis` | `5f08b286` (2026-04-25) | l'interface du homebrew (son CMake l'attend là)                                                     |
| [TkSharp](https://github.com/TKMM-Team/TkSharp)   | `utils/TkSharp`                     | `be46b9ad` (2026-09-15) | le code de TKMM, que`utils/tkmm-oracle` exécute pour empaqueter les `.tkcl` et comparer les fusions |

Chaque sous-module est figé sur le commit avec lequel tout a été construit et vérifié : un clone
reprend exactement ces versions-là, sans suivre la branche amont.

## Construire

**Rien à installer à part Docker.** Toutes les chaînes d'outils (devkitPro, Rust pour Skyline,
Meson, .NET) sont dans une image ; le dépôt est monté dedans et c'est là que tout se construit —
à l'identique sur Windows, Linux et macOS.

```bash
git clone <ce dépôt> && cd totk-various-poc
./build.sh                  # les deux packs, dans release/
```

```powershell
.\build.ps1                 # la même chose depuis PowerShell
```

### Ce que sait faire `build.sh`

```bash
./build.sh                          # pack-switch et pack-emulator  → release/
./build.sh pack-switch              # un seul des deux
./build.sh plugins                  # enemy-hp.nro, online-example.nro → release/plugins/
./build.sh --romfs <dossier> mod    # le mod EnemyHp → release/mods/EnemyHp/
./build.sh test                     # les tests des crates qui tournent sur PC
./build.sh doctor                   # ce que contient l'image
./build.sh shell                    # un shell dans le conteneur
./build.sh clean [--all]            # vide output/ (et release/)
./build.sh --rebuild-image          # reconstruit l'image
```

Les mêmes options existent en PowerShell (`-Romfs`, `-Zip`, `-RebuildImage`, `-All`…). Tout est
détaillé dans **[docs/build.fr.md](docs/build.fr.md)** ; construire des mods, dans
**[docs/mods.fr.md](docs/mods.fr.md)**.

## Installation et utilisation

> À quoi ressemble la carte SD. La construction écrit un pack dans `release/`, dont le contenu se
> copie sur la carte.

```text
sd:/
├───atmosphere
│   └───contents
│       └───0100F2C0115B6000
│           ├───exefs                                  # l'exefs de skyline-totk
│           │       main.npdm
│           │       subsdk9
│           │
│           └───skyline
│               └───plugins
│                       totk-mod-merger-plugin.nro     # le plugin fusionneur
│
├───skyline
│   └───totk
│           config.ini                                 # les réglages de skyline-totk
│
├───switch
│       totk-mod-manager.nro                           # le homebrew qui gère les mods et lance le jeu
│
└───totk
    │   config.ini                                     # les réglages du fusionneur, partagés avec le manager
    │
    └───mods                                           # les mods, chacun dans son dossier
        ├───EnemyHp
        │       EnemyHp.tkcl
        │       plugin.nro
        │       mod.ini
        │       enemy-hp.ini
        └───OtherExample
                romfs/
                thumbnail.jpg
                mod.ini
```

### Utilisation

Pour se servir du manager, il faut ouvrir son lanceur de homebrew **par le jeu** : lancer TotK en
maintenant **R** sur la manette. Le manager peut alors lire les fichiers du jeu, fusionner les
mods et lancer la partie.

Voir **[le README du manager](totk-mod-manager/README.fr.md)** pour les options et l'utilisation du
homebrew, et **[le README du fusionneur](totk-mod-merger-plugin/README.fr.md)** pour celles du
plugin fusionneur.
Voir **[la notice du pack console](packs/lisezmoi-switch.md)** pour son installation et son usage,
et **[celle du pack émulateur](packs/lisezmoi-emulateur.md)** pour l'autre.

## Note

Voir **[CREDITS.fr.md](CREDITS.fr.md)** pour tout ce qui est emprunté, porté ou pris comme
référence, et pour les licences du code tiers.

Aucun fichier du jeu ne se trouve dans ce dépôt.
