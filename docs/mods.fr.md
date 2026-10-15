# Les mods

*[English version](mods.md)*

Un pack contient de quoi faire tourner des mods, pas des mods : ils se construisent à part et se
déposent dans `sd:/totk/mods/`. Le dépôt en construit un de bout en bout, **EnemyHp**, en deux
éditions qui servent d'exemples complets de mod à données *et* à code — une pour chaque façon
qu'a un mod d'apporter du code :

| Édition | Son code | Ses réglages | A besoin de |
|---|---|---|---|
| **EnemyHp** | un plugin Skyline (`plugin.nro`), en Rust | un fichier, `enemy-hp.ini` | skyline-totk et le fusionneur |
| **EnemyHp SE** | un module exefs (`subsdk9`) dans le `.tkcl`, construit avec exlaunch | les options du paquet | rien : le même fichier s'importe dans TKMM |

Les deux affichent les PV des ennemis en chiffres et font regagner les leurs aux ennemis autour
du joueur. On utilise l'une ou l'autre : elles accrochent les deux mêmes instructions du jeu, et
celle qui arrive en second les trouve prises et s'abstient.

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

### Les réglages de la régénération

`enemy-hp.ini`, à côté du `plugin.nro`. Le fichier est écrit en anglais, comme toutes les
configurations produites :

```ini
regen = 1                  # les ennemis dont la jauge est affichée regagnent leurs PV
regen_percent = 1.5        # part des PV max par seconde, en % (les décimales marchent)
regen_delay = 8            # secondes sans être touché avant que ça commence
regen_bosses = 1           # les boss et mini-boss aussi (ceux à grande jauge)
regen_percent_bosses = 0.2 # leur part à eux, s'ils doivent être plus lents
report = 0                 # enemy-hp.txt : les PV lus dans les fichiers fusionnés
debug = 0                  # journal détaillé dans skyline.log
```

## Construire EnemyHp SE

```bash
./build.sh --romfs /chemin/vers/romfs mod-se      # ou « mods » pour les deux éditions
```

```powershell
.\build.ps1 -Romfs D:\romfs mod-se
```

Le résultat est **un seul fichier**, `release/mods/EnemyHpSe/EnemyHpSe.tkcl`, qui contient tout,
à la façon dont TKMM empaquette un mod :

| Dans le paquet | Ce que c'est |
|---|---|
| `romfs` | les mêmes données que la première édition |
| `exefs/subsdk9` | le code : un module construit avec [exlaunch](https://github.com/shadowninja108/exlaunch) (le sous-module `utils/exlaunch`) à partir de [`mods/enemy-hp-se/source`](../mods/enemy-hp-se/source) — la régénération et les chiffres des boss ; pas de rapport |
| `exefs/main.npdm` | les permissions sous lesquelles le module est chargé : celles de skyline-totk, à l'octet près |
| les options | les deux groupes de la première édition, plus un par réglage (ci-dessous) |

Il va dans `sd:/totk/mods/EnemyHpSe/` pour le fusionneur — ou directement dans TKMM sur un PC, où
c'est un mod à code ordinaire, qui n'a besoin ni de Skyline ni du fusionneur.

### Ses options

Ce que la première édition lit dans `enemy-hp.ini` se choisit ici, dans le gestionnaire de mods
ou dans TKMM. Les noms sont en anglais, comme dans le paquet :

| Groupe | Options (celle par défaut en gras) |
|---|---|
| Number size (taille des chiffres) | Small, Medium, **Large**, Very large |
| Armour (armure) | **Champion's Tunic** (tunique de Prodige), Every armour (toutes) |
| Regeneration | Off, 0.5 %, 1 %, **1.5 %**, 2 %, 3 %, 5 % des PV max par seconde |
| Regeneration delay (délai) | 3, 5, **8**, 12, 20 secondes sans être touché |
| Boss regeneration (boss) | Off, 0.1 %, **0.2 %**, 0.5 %, 1 %, Same as enemies (comme les ennemis) |

Une option de paquet ne peut apporter que des fichiers, et un module ne peut lire que ceux du
jeu. Chacun des trois groupes de régénération fournit donc un petit fichier texte dans le romfs —
`EnemyHp/Regen.txt`, `EnemyHp/RegenDelay.txt`, `EnemyHp/RegenBoss.txt`, une ligne chacun, avec
les clés d'`enemy-hp.ini` — que le jeu fusionné sert comme n'importe quel autre fichier et que le
module lit la première fois qu'une jauge est dessinée
([`options.cpp`](../mods/enemy-hp-se/source/options.cpp)).

### Comment son code est chargé

Un module n'est chargé ni par Skyline ni par le fusionneur : c'est la console (ou Ryujinx) qui le
met en mémoire avec le jeu, à son **démarrage**. Ce que fait le fusionneur, c'est le poser là où
il est cherché — `atmosphere/contents/<titre>/exefs/subsdk1`, à côté du `subsdk9` de Skyline —
quand les mods sont fusionnés, et l'en retirer quand le mod est désactivé (voir
[le README du fusionneur](../totk-mod-merger-plugin/README.fr.md#mods-avec-du-code)). Deux
conséquences :

- **appliquez les mods depuis le gestionnaire avant de lancer le jeu.** Un changement que le
  fusionneur fait pendant que le jeu démarre ne se voit qu'au démarrage suivant — son journal le
  dit ;
- le module est fait pour **TotK 1.2.1**. Sur une autre version, il trouve le code du jeu
  différent et ne fait rien, comme le plugin de la première édition.

Le module dit ce qu'il a fait dans le journal de débogage (`svcOutputDebugString`, que Ryujinx
recopie dans son propre journal) : `[enemy-hp-se] regeneration on (enemy gauge hooked), boss gauge
hooked`, puis les options qu'il a lues la première fois que la jauge d'un ennemi s'affiche.

## Faire son propre mod

Un mod est un dossier dans `sd:/totk/mods/` :

```text
totk/mods/Mon Mod/romfs/...        un mod en dossier (les fichiers tels qu'ils vont dans le jeu)
totk/mods/Mon Mod/Mon Mod.tkcl     ou un paquet TKMM
totk/mods/Mon Mod/mod.ini          facultatif : nom, version, priorité
totk/mods/Mon Mod/plugin.nro       facultatif : du code, en plugin Skyline
totk/mods/Mon Mod/exefs/subsdk9    facultatif : du code, en module exefs (en dossier ou dans le .tkcl)
totk/mods/Mon Mod/atmosphere/exefs_patches/<nom>/<build id>.ips
                                   ou des patchs de code empaquetés pour Atmosphère, lus tels quels
```

Le fusionneur lit les mods actifs, fusionne leurs fichiers (BYML, SARC, RSTB, MSBT, patchs de
code) et sert le résultat au jeu ; puis il charge le `plugin.nro` de chaque mod qui en fournit
un, et laisse les modules des autres dans le dossier exefs du titre pour le prochain démarrage.
Ce qu'un plugin peut demander au fusionneur est décrit dans
[le README du fusionneur](../totk-mod-merger-plugin/README.fr.md) ; l'API `tkm_*` vit dans
[`sharedlibs/totk-mod-merger-api`](../sharedlibs/totk-mod-merger-api).

Pour un plugin écrit ici, le plus simple est de partir de `plugins/online-example` (court) ou de
`plugins/enemy-hp` (complet : crochets, lecture du composant « vie », interface). Les ajouter au
`Cargo.toml` de la racine suffit à les construire :

```bash
./build.sh plugins          # release/plugins/*.nro
```

Pour un module, partez de [`mods/enemy-hp-se`](../mods/enemy-hp-se) : son dossier `source/` est
un *programme* exlaunch (`main.cpp` avec `exl_main`, `setting.hpp`), et
[`utils/build-exlaunch.sh`](../utils/build-exlaunch.sh) construit n'importe quel dossier de ce
genre en un `subsdk9` et un `main.npdm` :

```bash
./build.sh -- bash utils/build-exlaunch.sh mon-mod --source mods/mon-mod/source --out output/mon-mod/exefs
```

Lequel choisir : un plugin est chargé tout de suite (pas de redémarrage), peut utiliser ce que
Skyline et le fusionneur exportent, et s'écrit ici en Rust ; un module est ce que les
utilisateurs de TKMM peuvent installer, et ce que sont les mods exlaunch existants.

## Comparer sa fusion à celle de TKMM

`utils/tkmm-oracle` exécute le vrai TkSharp. C'est ce qui sert à vérifier, fichier par fichier,
que le fusionneur en Rust donne le même résultat que TKMM — modules compris (`exefs/subsdk1`…) :

```bash
./build.sh --romfs /chemin/vers/romfs shell
dotnet "$(find output/dotnet -name tkmm-oracle.dll)" merge /romfs output/tmp/tkmm <mods...>
cargo run --release -p totk-merge --example compare_merge -- /romfs output/tmp/tkmm output/tmp/ours <mods...>
```

Les autres exemples de [`sharedlibs/totk-merge/examples/`](../sharedlibs/totk-merge/examples)
rejouent la lecture des formats du jeu sur un PC : `enemy_hp`, `armor_effects`, `byml_print`,
`sarc_list`, `msbt_dump`…

Quand un paquet ne fait pas ce qu'il devrait, `tkcl_contents` liste ce qu'il contient vraiment —
les fichiers qu'il modifie, ses patchs de code et le build qu'ils visent, et son code exefs
(`subsdk9`, `main.npdm`), pour le mod lui-même et pour chacune de ses options :

```bash
cargo run --release -p totk-merge --example tkcl_contents -- "/mods/Mon Mod.tkcl"
```
