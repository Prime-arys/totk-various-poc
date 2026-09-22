# Les mods

*[English version](mods.md)*

Un pack contient de quoi faire tourner des mods, pas des mods : ils se construisent à part et se
déposent dans `sd:/totk/mods/`. Le dépôt en construit un de bout en bout, **EnemyHp**, qui sert
d'exemple complet de mod à données *et* à code.

## Construire EnemyHp

```bash
./build.sh --romfs /chemin/vers/romfs mod
```

```powershell
.\build.ps1 -Romfs D:\romfs mod
```

Le romfs — un *dump* de **TotK 1.2.1** extrait — n'est pas dans le dépôt et n'y entre pas : il
est monté en lecture seule dans le conteneur, le temps de la construction. Les données du mod
sont calculées à partir des fichiers du jeu, donc il est indispensable.

Le résultat arrive dans `release/mods/EnemyHp/` :

| Fichier | Ce que c'est |
|---|---|
| `EnemyHp.tkcl` | les données, empaquetées par le code de TKMM (`utils/tkmm-oracle`, qui s'appuie sur le sous-module `utils/TkSharp`) : les chiffres au-dessus des jauges, la zone de texte sous la jauge des boss, et l'effet d'armure qui les allume |
| `plugin.nro` | le code : régénération des ennemis autour du joueur, les chiffres des boss, le rapport `enemy-hp.txt` |
| `enemy-hp.ini` | les réglages du plugin, relus à chaque lancement du jeu |
| `mod.ini` | nom, version, description, priorité |

Le dossier se copie tel quel dans `sd:/totk/mods/EnemyHp/`, puis s'active depuis le homebrew.

Le mod propose deux groupes d'options, choisis dans le gestionnaire : la **taille des chiffres**
(quatre tailles) et l'**armure** qui les affiche (la nouvelle tunique de Prodige seule, comme
dans *Breath of the Wild*, ou toutes).

## Les réglages de la régénération

`enemy-hp.ini`, à côté du `plugin.nro` :

Le fichier est écrit en anglais, comme toutes les configurations produites :

```ini
regen = 1                  # les ennemis dont la jauge est affichée regagnent leurs PV
regen_percent = 1.5        # part des PV max par seconde, en % (les décimales marchent)
regen_delay = 8            # secondes sans être touché avant que ça commence
regen_bosses = 1           # les boss et mini-boss aussi (ceux à grande jauge)
regen_percent_bosses = 0.2 # leur part à eux, s'ils doivent être plus lents
report = 0                 # enemy-hp.txt : les PV lus dans les fichiers fusionnés
debug = 0                  # journal détaillé dans skyline.log
```

## Faire son propre mod

Un mod est un dossier dans `sd:/totk/mods/` :

```text
totk/mods/Mon Mod/romfs/...        un mod en dossier (les fichiers tels qu'ils vont dans le jeu)
totk/mods/Mon Mod/Mon Mod.tkcl     ou un paquet TKMM
totk/mods/Mon Mod/mod.ini          facultatif : nom, version, priorité
totk/mods/Mon Mod/plugin.nro       facultatif : le code que le mod apporte
```

Le fusionneur lit les mods actifs, fusionne leurs fichiers (BYML, SARC, RSTB, MSBT, patchs de
code) et sert le résultat au jeu ; puis il charge le `plugin.nro` de chaque mod qui en fournit
un. Ce que le plugin peut demander au fusionneur est décrit dans
[le README du fusionneur](../totk-mod-merger-plugin/README.fr.md) ; l'API `tkm_*` vit dans
[`sharedlibs/totk-mod-merger-api`](../sharedlibs/totk-mod-merger-api).

Pour un plugin écrit ici, le plus simple est de partir de `plugins/online-example` (court) ou de
`plugins/enemy-hp` (complet : crochets, lecture du composant « vie », interface). Les ajouter au
`Cargo.toml` de la racine suffit à les construire :

```bash
./build.sh plugins          # release/plugins/*.nro
```

## Comparer sa fusion à celle de TKMM

`utils/tkmm-oracle` exécute le vrai TkSharp. C'est ce qui sert à vérifier, fichier par fichier,
que le fusionneur en Rust donne le même résultat que TKMM :

```bash
./build.sh shell
cargo run --release -p totk-merge --example compare_merge -- /romfs <mods...>
```

Les autres exemples de [`sharedlibs/totk-merge/examples/`](../sharedlibs/totk-merge/examples)
rejouent la lecture des formats du jeu sur un PC : `enemy_hp`, `armor_effects`, `byml_print`,
`sarc_list`, `msbt_dump`…
