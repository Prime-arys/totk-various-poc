# Pack console — skyline-totk, totk-mod-merger, totk-mod-manager

*An English version of this notice sits next to it: `README.md`.*

Ce pack contient le nécessaire pour faire tourner des mods de *Tears of the Kingdom* sur une
Switch, **sans aucun mod** : les mods se construisent à part et se déposent ensuite dans
`totk/mods/`.

| Fichier | Ce que c'est |
|---|---|
| `atmosphere/contents/0100F2C0115B6000/exefs/subsdk9` + `main.npdm` | skyline-totk : Skyline adapté au jeu |
| `atmosphere/contents/0100F2C0115B6000/skyline/plugins/totk-mod-merger-plugin.nro` | le fusionneur de mods |
| `switch/totk-mod-manager.nro` | le homebrew qui gère les mods et lance la fusion |
| `skyline/totk/config.ini` | réglages de skyline-totk (journal, chargement des plugins) |
| `totk/config.ini` | réglages du fusionneur (profils, cache, journal) |

`contenu.txt`, à côté de ce fichier, liste tout avec les tailles et les empreintes MD5.

## Prérequis

- **Tears of the Kingdom 1.2.1**.
- **Atmosphère avec sigpatches** : skyline-totk remplace le `main.npdm` du jeu.
- Aucun autre mod exefs pour TotK : renommer `sd:/atmosphere/contents/0100F2C0115B6000/`
  s'il existe déjà, et ne pas laisser de dossier `romfs/` à cet endroit.

## Installation

Copier le **contenu** du dossier `SD` à la racine de la carte SD.

Pour mettre à jour un pack déjà installé, il suffit de remplacer
`switch/totk-mod-manager.nro` et
`atmosphere/contents/0100F2C0115B6000/skyline/plugins/totk-mod-merger-plugin.nro`.

## Utilisation

1. Sur le menu HOME, **maintenir R en lançant Tears of the Kingdom** : le menu homebrew s'ouvre.
2. Lancer **TotK Mod Manager**, y activer et ordonner les mods, puis **Appliquer** (**X**).
3. Lancer le jeu depuis le message de fin, ou normalement.

Les mods se rangent un par dossier dans `totk/mods/` (voir `totk/mods/LISEZMOI.txt`).
En cas de problème, `sd:/totk/merger.log` et `sd:/skyline/totk/skyline.log` racontent ce qui
s'est passé.
