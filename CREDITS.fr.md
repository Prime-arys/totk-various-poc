# Crédits et références

*[English version](CREDITS.md)*

Ce dépôt n'existe que grâce au travail d'autres personnes : un chargeur de code, une bibliothèque
d'interface, un fusionneur de mods, des chaînes d'outils et beaucoup de rétro-ingénierie publiée.
Cette page dit ce qui vient d'ailleurs, sous quelle licence, et ce qui a servi de référence.

Le reste — le fusionneur en Rust, le homebrew, les adaptations de Skyline, les plugins d'exemple —
est écrit ici.

## Code repris ou adapté

| Projet | Licence | Ce qu'il apporte |
|---|---|---|
| [Skyline](https://github.com/skyline-dev/skyline) (The Skyline Project) | MIT | Le chargeur : [`skyline-totk/`](skyline-totk/) en est une adaptation à TotK (NPDM du jeu, journal sur SD, initialisation repoussée, ABI `totk_*`). Les différences sont listées dans son [README](skyline-totk/README.fr.md). |
| [libeiffel](https://github.com/skyline-dev/libeiffel) (skyline-dev) | MIT | Utilitaires nnSdk, dans [`skyline-totk/libs/libeiffel`](skyline-totk/libs/libeiffel). |
| [And64InlineHook](https://github.com/Rprop/And64InlineHook) (Rprop) | MIT | Les crochets *inline* AArch64 dont dépendent tous les plugins, dans `skyline-totk/source/skyline/inlinehook/`. |
| [borealis](https://github.com/xfangfang/borealis) (xfangfang, d'après natinusala) | Apache-2.0 | L'interface du homebrew, en sous-module dans `totk-mod-manager/library/borealis` (figé sur `5f08b286`). Elle embarque elle-même fmt (MIT), tinyxml2 (zlib), tweeny (MIT), nanovg (zlib), yoga (MIT), libromfs, glad/glfw et SDL pour les versions PC, ainsi que les polices *Material Icons* (Apache-2.0, Google) et celles du système. |

## Portages et idées

| Projet | Licence | Ce qu'on lui doit |
|---|---|---|
| [TKMM / TkSharp](https://github.com/TKMM-Team/Tkmm) (TKMM-Team) | MIT | Le cœur du sujet : [`sharedlibs/totk-merge`](sharedlibs/totk-merge) est un portage en Rust de `TkSharp.Merging` (fusion BYML, SARC, RSTB, MSBT, GameDataList, patchs de code), et le format de paquet `.tkcl` est le sien. [`utils/tkmm-oracle`](utils/tkmm-oracle) exécute le vrai TkSharp — en sous-module dans `utils/TkSharp`, figé sur `be46b9ad` — pour vérifier que les deux donnent le même résultat, fichier par fichier. La [documentation de TKMM](https://tkmm.org/docs/settings) a servi de référence pour les réglages et les options de mods. |
| [ArcRopolis](https://github.com/Raytwo/arcropolis) (Raytwo) | — | L'idée qu'un mod puisse apporter son propre plugin, que le gestionnaire charge après la fusion. |
| [exlaunch](https://github.com/shadowninja108/exlaunch) (shadowninja108) | — | Référence sur l'injection de code et le format NPDM côté Switch. |
| [TotK-graphic-plugin](https://github.com/cucholix/TotK-graphic-plugin) (cucholix) | — | Exemple de plugin Skyline pour TotK, utile pour vérifier la compatibilité de l'ABI. |
| [nx-optimizer](https://github.com/MaxLastBreath/nx-optimizer) (MaxLastBreath) | — | Référence sur les patchs de code (`.pchtxt`) appliqués au jeu. |
| [SimpleModDownloader](https://github.com/PoloNX/SimpleModDownloader) (PoloNX) | GPL-3.0 | Le homebrew qui parcourt et installe les mods de GameBanana depuis la console, sur borealis lui aussi : la référence derrière l'onglet GameBanana de ce manager (recherche, tri, pages, fichiers d'un mod, contrôle MD5, extraction de l'archive). |
| [sys-clk](https://github.com/retronx-team/sys-clk) (RetroNX) | GPL-3.0 | Son service IPC, que le homebrew interroge pour monter les fréquences pendant une fusion puis remettre les réglages de l'overlay. Le protocole est ré-implémenté ici (`totk-mod-manager/source/core/sysclk.cpp`) : aucun code de sys-clk n'est repris. |

## Chaînes d'outils

| Outil | Rôle |
|---|---|
| [devkitPro](https://devkitpro.org/) — devkitA64, libnx, `npdmtool`, portlibs | Tout le code C/C++ pour la console. Son image [`devkitpro/devkita64`](https://hub.docker.com/r/devkitpro/devkita64) sert de base à celle du [`Dockerfile`](Dockerfile). |
| [cargo-skyline](https://crates.io/crates/cargo-skyline), [skyline-rs](https://github.com/ultimate-research/skyline-rs), [nnsdk-rs](https://github.com/ultimate-research/nnsdk-rs) (jam1garner, ultimate-research) | Rust pour les plugins Skyline : toolchain `skyline-v3`, `std` adapté, en-têtes nnSdk. |
| [linkle](https://github.com/MegatonHammer/linkle) (Megaton Hammer) | Conversion ELF → NRO (celle de devkitPro produit ici un `bss_size` nul). |
| [Meson](https://mesonbuild.com/) et [ninja](https://ninja-build.org/) | L'orchestration de ce dépôt, dans le conteneur. |
| [.NET](https://dotnet.microsoft.com/) | L'outil de comparaison avec TKMM. |
| [Atmosphère](https://github.com/Atmosphere-NX/Atmosphere), [hbmenu](https://github.com/switchbrew/nx-hbmenu) | Ce qui fait tourner tout ça sur une console. |
| [GameBanana](https://gamebanana.com/) | Le catalogue de mods que totk-mod-manager parcourt et depuis lequel il installe, via son API publique (apiv12). |
| [Ryujinx](https://ryujinx.org/) | Le banc d'essai sur PC. |
| [Docker](https://www.docker.com/) | L'unique prérequis de la machine : toutes les chaînes ci-dessus tiennent dans une image. |

## Bibliothèques

**Rust** (par cargo, voir [`Cargo.lock`](Cargo.lock)) : `ruzstd` et `miniz_oxide`/`adler` pour zstd
et deflate sans `std`, `hashbrown` et `foldhash` pour les tables, `skyline`, `nnsdk` et
`libc-nnsdk` pour l'accès au système de la console.

**C** (portlibs de devkitPro, liées au homebrew) : libcurl, libarchive, zlib, bzip2, xz, lz4,
zstd, expat.

## Rétro-ingénierie et formats

Les formats de fichiers du jeu (SARC, BYML, MSBT, RSTB/RESTBL, BFLYT/BFLAN, ZSTD à dictionnaires)
sont décrits publiquement depuis *Breath of the Wild* ; ce dépôt s'appuie sur cette documentation
et, pour ce qui n'y est pas, sur de la lecture du code du jeu faite ici :

- [ZeldaMods](https://zeldamods.org/) et [oead](https://github.com/zeldamods/oead) (leoetlino) :
  la référence sur SARC, BYML, RSTB et la compression zstd de la série ;
- [Switch Toolbox](https://github.com/KillzXGaming/Switch-Toolbox) (KillzXGaming) : la structure
  des dispositions d'interface `bflyt`/`bflan` ;
- les conventions de paquets et de patchs de code (`.pchtxt`) de la communauté de mods TotK.

Les adresses crochetées dans le code du jeu (jauges de vie, composant « vie », fonctions de texte
d'interface) ont été trouvées ici, dans le `main` de **TotK 1.2.1** ; elles sont documentées à côté
du code qui les utilise ([`plugins/enemy-hp/src/game.rs`](plugins/enemy-hp/src/game.rs),
[`boss.rs`](plugins/enemy-hp/src/boss.rs), [`regen.rs`](plugins/enemy-hp/src/regen.rs)) et
vérifiées au démarrage avant d'être posées.

## Le jeu

*The Legend of Zelda: Tears of the Kingdom*, Nintendo Switch et Ryujinx appartiennent à leurs
propriétaires respectifs. Ce dépôt ne contient **aucun fichier du jeu** : ni assets, ni clés, ni
`romfs`. Tout ce qui est construit ici lit les fichiers de la copie que possède l'utilisateur, sur
sa propre console ou dans son propre dump.

## Licence de ce dépôt

Le code écrit ici suit la licence MIT de Skyline, dont il dérive
([`skyline-totk/LICENSE`](skyline-totk/LICENSE)). Les parties reprises gardent la leur : MIT pour
Skyline, libeiffel, And64InlineHook et TkSharp, Apache-2.0 pour borealis, et leurs licences
respectives pour ce que borealis embarque (fmt, tinyxml2, tweeny, nanovg, yoga, Material Icons…).

Si un crédit manque ou est mal attribué, c'est une erreur : le signaler suffit à la corriger.
