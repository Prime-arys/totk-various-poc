# Pack émulateur (Ryujinx) — skyline-totk, totk-mod-merger, totk-mod-manager

*An English version of this notice sits next to it: `README.md`.*

Le même contenu que le pack console, rangé là où Ryujinx l'attend, et **sans aucun mod** : les
mods se construisent à part et se déposent ensuite dans `sdcard/totk/mods/`.

| Fichier | Où il va |
|---|---|
| `Ryujinx/mods/contents/0100f2c0115b6000/skyline-totk/exefs/{subsdk9,main.npdm}` | mod exefs du jeu |
| `Ryujinx/sdcard/atmosphere/contents/0100F2C0115B6000/skyline/plugins/totk-mod-merger-plugin.nro` | le fusionneur, sur la SD émulée |
| `Ryujinx/sdcard/switch/totk-mod-manager-ryujinx.nro` | le homebrew (version émulateur) |
| `Ryujinx/sdcard/skyline/totk/config.ini`, `Ryujinx/sdcard/totk/config.ini` | les réglages |

`contenu.txt`, à côté de ce fichier, liste tout avec les tailles et les empreintes MD5.

## Prérequis

- Ryujinx avec **Tears of the Kingdom 1.2.1** (le jeu de base plus sa mise à jour).
- Les mods exefs des autres outils (UltraCam…) désactivés pour ce jeu.

## Installation

Copier le contenu du dossier `Ryujinx` dans le dossier de données de Ryujinx
(`%AppData%\Ryujinx` sous Windows, `~/.config/Ryujinx` sous Linux) : les dossiers `mods` et
`sdcard` s'y ajoutent sans rien écraser d'autre.

Le mod exefs doit apparaître dans *Clic droit sur le jeu → Gérer les mods*.

## Utilisation

Le homebrew se lance avec *Fichier → Charger un homebrew*, en choisissant
`sdcard/switch/totk-mod-manager-ryujinx.nro` — c'est la version dont l'instruction que le JIT de
Ryujinx refuse a été retirée ; la version console plante à son lancement ici.

Ensuite comme sur console : activer les mods, **Appliquer**, puis lancer le jeu. Les journaux
sont dans `sdcard/totk/merger.log` et `sdcard/skyline/totk/skyline.log`.
