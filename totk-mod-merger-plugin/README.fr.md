# totk-mod-merger-plugin

*[English version](README.md)*

Fusion de mods **directement sur la console**, pour The Legend of Zelda: Tears of the Kingdom.

C'est l'équivalent embarqué de [TKMM](https://github.com/TKMM-Team/Tkmm) : au démarrage du jeu, le
plugin calcule ce que chaque mod change par rapport au romfs du jeu (les *changelogs* de TKMM),
rejoue ces changements les uns sur les autres, reconstruit la table des tailles de ressources (RSTB)
et sert le résultat au jeu via `nn::fs`. Dans l'esprit
d'[ARCropolis](https://github.com/Raytwo/arcropolis) pour Smash.

- mods « dossier » (`romfs/`, `exefs/`) **et paquets TKMM `.tkcl`**, options comprises ;
- **profils** (listes de mods nommées, ordre et options par profil) ;
- **API C** (`tkm_*`) pour qu'un autre plugin (un mode en ligne…) impose sa propre liste de mods ;
- réutilise un export **RomFSlite** de TKMM comme mod de base ;
- résultat **équivalent à celui de TKMM**, vérifié fichier par fichier (voir [Tests](#tests)).

Tourne sur [skyline-totk](../skyline-totk). Le homebrew [totk-mod-manager](../totk-mod-manager)
gère les mods et les profils depuis la console, installe des mods depuis GameBanana et fait la
fusion **avant** de lancer le jeu (« Appliquer ») : il embarque le même code de fusion (les crates
de ce dépôt, compilées sans `std`), donc le plugin retrouve sa fusion et démarre sans attendre.

## Installation

1. Installer skyline-totk (voir son README).
2. Copier `totk-mod-merger-plugin.nro` dans
   `sd:/atmosphere/contents/0100F2C0115B6000/skyline/plugins/`
   — **pas** dans `.../romfs/skyline/plugins` : dès qu'un dossier `romfs` existe pour le titre,
   Atmosphère construit un romfs « LayeredFS » par-dessus les ~300 000 fichiers de TotK à chaque
   démarrage (lent, et gourmand en mémoire au point d'empêcher le jeu de démarrer sur les firmwares
   récents). Le plugin prévient dans son log si ce dossier existe.
3. Déposer les mods dans `sd:/totk/mods/`, **chacun dans son propre dossier** (ou les installer
   avec totk-mod-manager) :

```
sd:/totk/
├── config.ini                    (optionnel ; profile = profil actif)
├── mods/
│   ├── Mon Mod/                  mod « dossier »
│   │   ├── mod.ini               (optionnel : nom, version, auteur, description…)
│   │   ├── romfs/...
│   │   ├── exefs/*.pchtxt|*.ips  (optionnel : patchs de code)
│   │   └── plugin.nro            (optionnel : code du mod, voir Mods avec du code)
│   └── Autre Mod/                paquet TKMM
│       ├── mod.ini               (optionnel)
│       └── Autre Mod.tkcl
├── profiles/
│   └── Défaut.ini                listes de mods (voir Profils)
├── locale.txt                    (généré : langue que lit le jeu)
├── cache/                        (généré : changelogs des mods dossier, conflicts.tsv)
├── merged/                       (généré : fusions gardées, voir Cache de fusion)
└── merger.log                    (généré)
```

Un mod rangé un dossier plus bas (`Mon Mod/Mon Mod v2/romfs`, comme beaucoup d'archives se
décompressent) est aussi trouvé. Un `.tkcl` posé en vrac dans `mods/` est ignoré (le log l'indique ;
totk-mod-manager le range dans un dossier à son lancement).

La fusion n'a lieu qu'au premier démarrage avec une combinaison de mods jamais fusionnée ; ensuite
le résultat est réutilisé tel quel (démarrage instantané). `force_merge = 1` ou la suppression de
`merged/` la relance. Avec totk-mod-manager, elle se fait dans le homebrew et le jeu démarre
directement.

### Cache de fusion

Les `merge_cache_size` dernières fusions (10 par défaut) restent sur la carte SD. Revenir à des mods
déjà fusionnés (changer de profil, réactiver un mod, remettre une option) ressert la fusion gardée
sans rien recalculer. Au-delà, la moins récemment utilisée est supprimée après la fusion suivante.

```
sd:/totk/merged/
├── store/<xx>/<empreinte>-<taille>   fichiers fusionnés, nommés d'après leur contenu (xxHash64)
├── <id>/                             une fusion : index.tsv (chemin du jeu → fichier servi),
│                                     patches.tsv, plan.txt, locales.txt, profile.txt,
│                                     conflicts.tsv, puis stamp.txt en dernier (fusion complète)
└── recent.txt                        les fusions, de la plus récente à la plus ancienne
```

Un fichier qu'une autre fusion a déjà produit n'est pas réécrit : refusionner les mêmes mods n'écrit
rien, et une variante (un mod de moins) n'écrit que les fichiers qui changent. Sur 4 vrais mods
(1 321 fichiers servis, 141 Mio) : 16 fichiers écrits pour la même liste moins deux mods, retour à la
première fusion en 0,01 s. Les fichiers gardés tels quels dans les dossiers des mods ne sont jamais
copiés. Une fusion interrompue (pas de `stamp.txt`) est ignorée puis supprimée.

La fin de chaque fusion est détaillée dans le journal :
`merged 2708 files (1321 served) in 3.9s: reading mods 0.8s, comparing them 0.0s, merging 3.1s (of which writing packs 0.2s); 1175 file(s) written (141 MiB), 0 already stored`.

## Configuration

`sd:/totk/config.ini` — voir [`config.example.ini`](config.example.ini). Les réglages utiles :

| Clé | Défaut | Rôle |
|---|---|---|
| `profile` | *(vide)* | profil actif (`sd:/totk/profiles/<nom>.ini`) ; vide : tous les mods, par `priority` |
| `mods_dir` / `profiles_dir` | `sd:/totk/mods` / `sd:/totk/profiles` | dossiers des mods et des profils |
| `merge_at_boot` | `1` | mods changés depuis la dernière fusion : fusionner au démarrage (`1`) ou garder la dernière fusion jusqu'à « Appliquer » dans le manager (`0`, démarrage toujours rapide) |
| `merge_cache_size` | `10` | nombre de fusions gardées (voir Cache de fusion) |
| `locales` | `auto` | textes (`Mals`) à fusionner : `auto` (la langue que lit le jeu), `all`, ou une liste `USen,EUfr` |
| `use_romfslite` | `1` | utiliser un export RomFSlite de TKMM comme mod de base |
| `apply_patches` | `1` | appliquer en mémoire les patchs `.ips`/`.pchtxt` des mods |
| `mod_plugins` | `1` | charger les plugins fournis par les mods (voir Mods avec du code) |
| `shop_param_limit` | `512` | limite des boutiques relevée par TKMM à chaque fusion (`0` : désactivé) |
| `control_timeout_ms` | `60000` | attente maximale d'un plugin qui a pris la main (API) |
| `log_redirects` / `verbose` | `0` | journaux de diagnostic |

**Langues.** TotK ne lit qu'une archive de textes, celle de sa langue. En `auto`, le plugin note au
démarrage laquelle il ouvre (`sd:/totk/locale.txt`) et les fusions suivantes ne traitent que
celle-là : sur 4 mods réels qui touchent aux textes, 145 Mo écrits sur la SD au lieu de 274. Tant
que la langue n'est pas connue (première fusion), toutes sont fusionnées. Une fusion qui contient
plus de langues que nécessaire reste valide ; changer la langue de la console refait la fusion au
démarrage suivant.

Par mod, `<mod>/mod.ini` (écrit par totk-mod-manager lors d'une installation) :

```ini
name = Nom affiché
version = 1.2
author = Auteur
description = Une ligne\nou plusieurs
url = https://gamebanana.com/mods/123456
thumbnail = thumbnail.jpg
plugins = 1             # 0 : garder les fichiers du mod, ne pas charger son code
# Sans profil seulement :
enabled = 1
priority = 100          # plus haut = fusionné après = gagne les conflits

[options]
# Options par défaut de ce mod (un profil peut les remplacer) :
# groupe = option(s), séparées par « ; »
Couleur = Rouge
Armes = Épées; Arcs
```

### Profils

`sd:/totk/profiles/<nom>.ini`, le profil actif étant `profile` dans `config.ini`. Les mods y sont
listés par nom de dossier, **le premier l'emporte** sur les suivants (comme la liste de TKMM) ;
un mod installé mais absent du profil n'est pas chargé.

```ini
[mod]
folder = Weapons of Legend Redux
enabled = 1
option.Type d'arme = Épées; Arcs

[mod]
folder = Even More Wonderful Capsules
enabled = 0
```

### Mods avec du code

Un mod peut fournir son propre **plugin Skyline**, que le merger charge dans le jeu une fois la
fusion servie :

```
sd:/totk/mods/Mon Mod/
├── mod.ini
├── romfs/...                  (facultatif : un mod peut n'être que du code)
├── plugin.nro                 chargé après la fusion
└── plugins/*.nro              (ou plusieurs, chargés par ordre alphabétique)
```

C'est ce qui permet à **plusieurs mods de code de coexister** : une console n'a qu'un seul exefs
(`atmosphere/contents/<titre>/exefs`), déjà occupé par Skyline, donc deux mods qui remplacent chacun
`subsdk9` ne peuvent pas être installés ensemble ; sous forme de plugins, ils se chargent l'un après
l'autre.

- Seuls les plugins des mods **activés dans le profil** sont chargés, dans l'ordre de la fusion.
- Un même `.nro` fourni par deux mods n'est chargé qu'une fois.
- `plugins = 0` dans le `mod.ini` garde les fichiers du mod et laisse son code de côté ;
  `mod_plugins = 0` dans `config.ini` les désactive tous (totk-mod-manager expose les deux).
- Le chargement passe par `nn::ro`, comme celui de Skyline : lecture, enregistrement des empreintes,
  puis `main`. Le log liste ce qui est chargé.

Un tel plugin est un plugin Skyline ordinaire (hooks, `nn::fs`, patchs mémoire). Il tourne **après**
la fusion : il lit donc les fichiers fusionnés, mais il est trop tard pour choisir les mods (pour
cela, un plugin dans `skyline/plugins`, voir *API pour les autres plugins*). Deux fonctions lui
disent d'où il vient :

```rust
let dir  = totk_mod_merger_api::current_mod_dir();   // "sd:/totk/mods/Mon Mod"
let name = totk_mod_merger_api::current_mod_name();  // "Mon Mod"
```

> Le tas du jeu n'a que quelques mégaoctets libres quand les plugins tournent : un plugin qui lit
> des fichiers du jeu doit prendre ses gros tampons ailleurs, comme le fait
> [`plugins/enemy-hp/src/scratch.rs`](../plugins/enemy-hp/src/scratch.rs) (mémoire mappée par
> skyline-totk). Sans cela, la première allocation d'un mégaoctet fait planter le jeu.

Exemple complet : [`plugins/enemy-hp`](../plugins/enemy-hp) — un mod qui n'est que du code
(`mod.ini` + `plugin.nro`). Il lit les paramètres des ennemis **dans les fichiers fusionnés** et
écrit leurs points de vie dans `enemy-hp.txt`, à côté du mod :

```
Enemy_Bokoblin_Junior   25
Enemy_Bokoblin_Middle   72
Enemy_Lynel_Senior      4000
```

Les mêmes valeurs se vérifient sur PC :
`cargo run --release -p totk-merge --example enemy_hp -- <romfs>` (le calcul est le fichier du
plugin, inclus tel quel).

**Les afficher au-dessus de la barre de vie**, comme la tunique de Prodige de Breath of the Wild,
ne demande pas de code : TotK le fait toujours, il lui manque seulement les données.

- Le code de la jauge ennemie (vers `0x12c7b90` dans le `main` de 1.2.1) calcule le rapport PV
  actuels / PV max pour l'animation `Gauge`, puis, si l'armure portée a l'effet **`VisualizeLife`**,
  joue l'animation `TextVisible` et écrit les deux nombres, mis en forme par les messages
  `LayoutMsg/EnemyInfo_00` `0000` et `0001`, dans les panneaux **`T_CurrentLife_00`** et
  **`T_MaxLife_00`**.
- Or aucune armure de TotK n'a cet effet, `blyt/PaEnemyLife_00.bflyt` n'a plus de texte (juste
  l'ancre vide `N_TextVisible_00` que cette animation affiche) et `EnemyInfo_00.msbt` n'est plus
  dans les archives de messages.

Deux exemples reconstruisent tout ça :
[`make_enemy_life_ui_mod`](../sharedlibs/totk-merge/examples/make_enemy_life_ui_mod.rs) ajoute les deux
panneaux sous l'ancre (police `Normal_00`, une variante de la disposition par taille de texte) et
les deux messages (la balise « nombre » d'un message du jeu) ;
[`make_visualize_life_mod`](../sharedlibs/totk-merge/examples/make_visualize_life_mod.rs) donne l'effet à la
Nouvelle tunique de Prodige (`Armor_1106..1110_Upper`) ou à toutes les armures, en gardant leurs
effets d'origine. [`plugins/enemy-hp/build-mod.sh`](../plugins/enemy-hp/build-mod.sh) assemble le tout
en un `.tkcl` de 284 Ko avec deux groupes d'options (taille des chiffres, armure), plus le plugin :

```bash
./build.sh --romfs <romfs> mod                       # à la racine du dépôt
../plugins/enemy-hp/build-mod.sh <romfs> <dossier>   # ou seul, dans le conteneur
```

Vérifié en jeu : « 35/35 », « 840/840 » au-dessus des jauges avec la tunique portée.

**Les boss et mini-boss** (Hinox, Golems, Molduga, Gleeok, boss des temples…) ont leur propre
jauge, en haut de l'écran : `blyt/BossLife_00.bflyt`, mise à jour à chaque image par
`0x1ae2178` (l'écran `UIBossLifeScreen`), qui lit les mêmes champs de PV. Elle n'a jamais eu de
chiffres et le jeu n'a pas de code pour en écrire, donc cette fois c'est le plugin
([`plugins/enemy-hp/src/boss.rs`](../plugins/enemy-hp/src/boss.rs)) : un crochet à `0x1ae22b0`
(composant vie dans `x0`, l'écran dans `x19`) écrit « actuels/max » dans le panneau
**`T_BossLife_00`** que `make_enemy_life_ui_mod` ajoute, vide, sous le début de la barre (la barre
est ancrée à gauche et s'allonge avec les PV max du boss). Il passe par la fonction du jeu qui
écrit le nom du boss, `0xb519b0(layout = [écran + 40], nom du panneau, message, 1, 0)`, avec un
message `{texte UTF-16, longueur, -1}` comme le construit `0x1235fc4` ; seulement quand le texte
change, et seulement si l'octet que testent les jauges ennemies est levé (`[[0x462ec80] + 2204]`,
l'effet `VisualizeLife`). Le même crochet fait régénérer le boss, sauf avec `regen_bosses = 0`
(et `regen_percent_bosses` leur donne un rythme à part : ils ont des milliers de PV).

La barre, elle, est dessinée par trois animations (`[écran + 416]`, `+ 424` et `+ 448`) : le jeu
met la première là où sont les PV et laisse les autres la rattraper, mais seulement quand
l'animation de dégât est finie — or des PV rendus changent la vie à chaque image, ce qui la
relance sans cesse, et la barre restait donc au dernier coup reçu pendant que les chiffres
montaient. Quand il rend des PV, le plugin place lui-même les trois animations sur l'image
correspondante (`SetFrame`, vtable + 208), exactement comme le jeu le fait quand la jauge se
remplit (`0x1ae2228`).
Ce que le plugin sait du code du jeu est regroupé dans
[`plugins/enemy-hp/src/game.rs`](../plugins/enemy-hp/src/game.rs).

**La régénération**, elle, est du code : le plugin du mod
([`plugins/enemy-hp/src/regen.rs`](../plugins/enemy-hp/src/regen.rs)) pose un crochet *inline* à
`0x12c7a48`, là où le code de la jauge vient d'obtenir le composant « vie » de l'ennemi affiché,
juste avant d'en lire les PV : `[[vie + 6192] + 8]` (actuels), `[[vie + 6200] + 8]` (max), moins
`[[vie + 6216] + 8]` quand ce pointeur existe — les champs que la fonction de dégâts du jeu
(`0x64b4b8`) modifie et que les chiffres affichent. (`[[vie + 6208] + 8]`, que lisent les
fonctions `0x15ebf38`/`0x16f62a4`, est une seconde réserve qui encaisse les coups avant les PV,
vide pour un ennemi ordinaire : ce ne sont pas ses PV.) Il y remonte les PV de
`regen_percent` % du max par seconde (décimales acceptées, point ou virgule), `regen_delay`
secondes après le dernier coup reçu, sans
jamais dépasser le max ni ressusciter un ennemi à 0 PV ; l'écriture est atomique, comme celle du
jeu, et cède la place à un coup qui tombe au même moment. Les ennemis concernés sont ceux dont le
jeu affiche la jauge, c'est-à-dire ceux autour du joueur. Le crochet n'est posé que si
`totk_get_version() == 10201` et si les octets du site sont ceux attendus ; sinon le journal le
dit et le reste du mod fonctionne. Réglages : `enemy-hp.ini` dans le dossier du mod ; `debug = 1`
y écrit dans `skyline.log` ce que voit le crochet (premier appel, chaque ennemi, coups, PV rendus,
ennemis ignorés et pourquoi).

## Ce que fait la fusion

C'est un portage de `TkSharp.Merging` (TKMM 2.x), fusionneur par fusionneur :

| Fichiers | Traitement |
|---|---|
| `*.bgyml`, `*.byml` | fusion clé par clé ; tableaux par position, par valeur ou **par clé** (`Actors`/`Hash`, `BoneList`/`BoneName`… — les ~150 tables de TKMM) |
| `RSDB/*.rstbl.byml` | fusion par ligne (`__RowId`, `Name`, `NameHash`, `FullTagId`) ; table des tags par entrée |
| `GameData/GameDataList` | fusion par hash dans chaque table, puis recalcul des métadonnées de sauvegarde |
| `Mals/*.sarc` (`.msbt`) | fusion par libellé de texte, pour chaque langue |
| `.pack` | chaque fichier interne est fusionné séparément puis replacé dans tous les packs qui le contiennent |
| `.sarc`, `.blarc`, `.bfarc`, `.bkres`, `.genvb`, `.ta` | fusion récursive des fichiers internes |
| `*.rsizetable` | reconstruite (formules de TKMM, dont `.ainb`/`.asb`/`.bstar`/`.mc`) |
| `exefs/*.ips`, `*.pchtxt` | patchs de la version du jeu fusionnés et appliqués en mémoire au démarrage |
| autres (modèles, textures, sons…) | le mod de plus haute priorité gagne |

### Conflits

Avant de fusionner, le moteur compare ce que changent les mods (module `conflicts`) :

- **fichier** : plusieurs mods fournissent un fichier qui ne se fusionne pas, avec des contenus
  différents (des copies identiques ne comptent pas) ;
- **valeurs** : dans un fichier fusionné (`.bgyml`/`.byml`, lignes RSDB et GameData, `.msbt`,
  archives `.sarc`…), plusieurs mods donnent à la même valeur des contenus différents, ou l'un
  remplace/supprime un nœud dans lequel un autre modifie des valeurs. Les ajouts ne comptent pas, et
  un mod qui donne la même valeur que le mod prioritaire ne perd rien. Les textes sont comparés dans
  la langue du jeu.

La fusion continue dans tous les cas (le mod prioritaire l'emporte). Les conflits sont écrits dans
`merger.log` et dans `sd:/totk/cache/conflicts.tsv`, que totk-mod-manager affiche ; le manager les
présente aussi avant d'appliquer, avec la possibilité d'annuler (`totk_merge::set_conflict_sink`).
L'analyse ne lit que les fichiers touchés par au moins deux mods : 0,0 s sur les 4 mods réels de
test, qui ont un vrai conflit (`ELink2/elink2.Product.belnk` remplacé par deux mods).

Un fichier fourni tel quel par un mod dossier n'est pas recopié : il est servi depuis son dossier.
Les fichiers produits sont des trames zstd « brutes » (blocs non compressés) : du zstd valide, sans
coût CPU sur la console, un peu plus volumineux sur la SD.

### Ce qui n'est pas repris de TKMM

- **Changelogs de textures `__Combined.bntx`** d'un `.tkcl` : ignorés avec un avertissement (les
  fichiers BNTX complets fonctionnent, le plus prioritaire gagne).
- **Correction des matériaux `.bfres.mc`** que TKMM applique pour les shaders de la 1.4 : inutile
  en 1.2.1, non portée.
- **Code** (`subsdk*`, `main` d'un `exefs`) et **cheats** : ignorés. Les mods de code doivent être
  des plugins Skyline — c'est justement ce qui leur permet de coexister.
- La vérification « fichier identique à une ancienne version » de TKMM (liste de sommes de
  contrôle de 13 Mo) n'est pas embarquée : un mod dossier construit pour une version antérieure du
  jeu est comparé à la version installée.

## RomFSlite

[RomFSlite](https://tkmm.org/docs/settings) est une fonction de l'optimiseur UltraCam /
[nx-optimizer](https://github.com/MaxLastBreath/nx-optimizer) : son code injecté sert au jeu le
contenu de `atmosphere/contents/0100F2C0115B6000/romfslite/` directement, **sans passer par le
LayeredFS d'Atmosphère** (qui manque de mémoire sur les firmwares récents avec les 300 000 fichiers
de TotK). TKMM sait exporter sa fusion dans ce dossier.

Réutiliser *son code* n'est pas possible : il est fermé et vit dans l'exefs d'UltraCam, qui entre en
conflit avec celui de skyline-totk. Mais **ce plugin applique la même technique** (redirection
`nn::fs` depuis la SD, jamais de dossier `romfs` pour le titre), donc les mêmes performances :
aucune reconstruction LayeredFS au démarrage, et une fusion faite une seule fois puis mise en cache.

Et le **format** RomFSlite est pris en charge : si le dossier `romfslite` existe, son contenu est
fusionné sous tous les autres mods. On peut donc garder un profil TKMM exporté depuis le PC et
ajouter des mods par-dessus sur la console. (Si UltraCam est aussi installé, désactivez son
RomFSlite pour éviter que les deux servent des fichiers.)

## API pour les autres plugins

Un plugin peut prendre la main sur la liste des mods, par exemple un mode en ligne qui télécharge
le pack commun à tous les joueurs. Tant qu'il a la main, **les mods de la SD ne sont pas chargés**
(sauf s'il le demande), seuls ceux qu'il ajoute sont fusionnés, et le résultat va dans un dossier
séparé (le cache des mods locaux reste valide).

- Contrat : [`include/totk_mod_merger.h`](include/totk_mod_merger.h)
- Bindings Rust : [`sharedlibs/totk-mod-merger-api`](../sharedlibs/totk-mod-merger-api)
- Exemple complet : [`plugins/online-example`](../plugins/online-example) (télécharge
  `manifest.txt` et les `.tkcl` listés depuis un serveur HTTP, les impose, et retombe sur les
  derniers packs téléchargés ou sur les mods locaux si le serveur ne répond pas)

```rust
use totk_mod_merger_api::ModMerger;

#[skyline::main(name = "online")]
pub fn main() {
    let Some(merger) = ModMerger::find() else { return };
    let Some(control) = merger.take_control("online", 30_000) else { return };
    let worker = std::thread::spawn(move || {
        let pack = download_pack();                       // votre code
        let index = control.add_mod(&pack, Some("Pack en ligne")).unwrap();
        control.select_option(index, "Règles", "Classé");
        control.set_merged_dir("sd:/totk/online/merged");
        control.commit();                                 // la fusion démarre
    });
    std::mem::forget(worker); // ne jamais laisser tomber un JoinHandle sur Skyline (voir plus bas)
}
```

Déroulement : skyline-totk lance tous les `main` des plugins (dans un ordre quelconque), puis appelle
le merger via `skyline_totk_on_plugins_loaded`. Le jeu est alors toujours bloqué sur le montage de
son romfs : le merger attend `tkm_commit`/`tkm_release_control` (ou le délai), fusionne, installe la
redirection, puis appelle les callbacks `tkm_on_merged`. `skyline_totk_init_sockets` permet
d'utiliser le réseau aussi tôt.

> Piège Skyline : laisser tomber le `JoinHandle` d'un thread appelle `pthread_detach`, qui fait
> planter nnSdk. Joignez-le ou oubliez-le (`std::mem::forget`).

## Mémoire et performances

Au moment où les plugins tournent, le tas du jeu n'a que quelques mégaoctets libres. Le plugin a donc
son propre tas (`src/alloc.rs`) sur de la mémoire mappée depuis le noyau par skyline-totk
(`totk_map_memory`), utilisé seulement pendant la fusion puis rendu (`trim`) — seul ce qui sert
ensuite (la table de redirection) reste sur le tas du jeu.

Les gros documents sont lus à la demande (une GameDataList compte 1,4 million de nœuds), les maps
BYML sont des vecteurs triés à clés partagées, et l'écriture BYML ne duplique ni les chaînes ni les
nœuds : la fusion de test la plus lourde (GameDataList + ActorInfo + textes de 14 langues + packs)
culmine à **~120 Mio** au lieu de 213. Sous Ryujinx, cette fusion prend ~8 s au premier démarrage,
puis 0 s (cache).

## Compilation

Depuis la racine du dépôt, tout se construit dans le conteneur (voir
[docs/build.fr.md](../docs/build.fr.md)) :

```bash
./build.sh totk-mod-merger-plugin.nro   # le fusionneur
./build.sh plugins                      # les plugins d'exemple, dans release/plugins/
```

Ou seul, dans un shell du conteneur (`./build.sh shell`) :

```bash
utils/build-nro.sh totk-mod-merger-plugin
utils/build-nro.sh online-example        # plugin d'exemple (API)
utils/build-nro.sh enemy-hp              # plugin d'exemple fourni par un mod
```

`cargo skyline build` n'est pas utilisé directement : la 3.5 fournit une *target spec* écrite pour
un rustc plus récent que `skyline-v3` ; [`utils/build-nro.sh`](../utils/build-nro.sh) l'adapte puis
convertit l'ELF avec `linkle` (`elf2nro` produit ici un NRO avec `bss_size = 0`, qui plante).

## Tests

```bash
cargo test -p totk-formats -p totk-merge
# contre les vrais fichiers du jeu (romfs extrait) :
TOTK_VANILLA_DIR=/chemin/romfs cargo test -p totk-formats -p totk-merge -- --nocapture
```

### Parité avec TKMM

[`utils/tkmm-oracle`](../utils/tkmm-oracle) exécute le code de TKMM lui-même (TkSharp) pour empaqueter
un `.tkcl` ou fusionner des mods ; `compare_merge` fusionne les mêmes mods avec ce plugin et compare
les résultats contenu par contenu (arbres BYML, entrées d'archives, libellés MSBT, entrées RSTB,
patchs).

```bash
dotnet output/dotnet/bin/TkmmOracle/release/tkmm-oracle.dll merge <romfs> out-tkmm modA modB paquet.tkcl
cargo run --release -p totk-merge --example compare_merge -- <romfs> out-tkmm travail modA modB paquet.tkcl
cargo run --release -p totk-merge --example make_fixtures -- <romfs> fixtures   # mods de test réalistes
cargo run --release -p totk-merge --example peak_memory -- <romfs> travail mods...
```

Résultats (TotK 1.2.1) :

| Jeu de mods | Fichiers | Résultat |
|---|---|---|
| 2 mods générés qui se chevauchent (même document dans un pack, ActorInfo, GameDataList, textes, tags) | 7 | équivalent |
| 1 mod dossier + 1 `.tkcl` empaqueté par TKMM | 7 | équivalent |
| 5 vrais mods (dont 2 sur le même pack, un `.bcett` à tableaux par clé, patchs `.pchtxt`) | 42 | équivalent, RSTB comprise |

Sous Ryujinx : fusion de 4 mods (dont un `.tkcl` avec option) au démarrage, patchs appliqués, jeu
jusqu'à l'écran titre avec les fichiers fusionnés ; plugin d'exemple en ligne : prise de contrôle,
téléchargement, repli sur les mods locaux, callback de fin. Reste à confirmer sur console.

Mods avec du code, sous Ryujinx : le mod `EnemyHp` (`mod.ini` + `plugin.nro`, aucun fichier) est
chargé après la fusion, retrouve son dossier, lit 19 ennemis en 0,4 s et écrit son rapport. Avec un
mod qui met le Bokoblin bleu à 1 PV par-dessus (`make_enemy_hp_mod`), le rapport indique 1 au lieu
de 72 : le plugin lit bien les fichiers fusionnés. À noter : le tas du jeu a refusé le premier
mégaoctet demandé par le plugin, d'où `scratch.rs`.

## Architecture

```
src/                          plugin Skyline : entrée, API tkm_*, hooks nn::fs, patchs, tas
../sharedlibs/totk-merge/     orchestration et fusion, no_std + alloc (feature std par défaut)
  engine.rs control.rs mods.rs profile.rs cache.rs   fusion, prise de contrôle, mods, profils, cache
  conflicts.rs merge_cache.rs                        conflits entre mods, fusions gardées
  tkcl.rs rom.rs canonical.rs config.rs ini.rs       paquets TKMM, fichiers du jeu, réglages
  builder.rs merger.rs                               TkChangelogBuilder / TkMerger
  byml_*.rs rsdb.rs gamedata.rs                      fusionneurs
../sharedlibs/totk-formats/   BYML, SARC, MSBT, RESTBL, zstd, ZIP ; sys.rs : fichiers/horloge
../sharedlibs/totk-mod-merger-api/  bindings Rust de l'API
src/plugins.rs                chargement des plugins fournis par les mods
../plugins/online-example/    plugin d'exemple (prend la main sur la liste des mods)
../plugins/enemy-hp/          mod d'exemple qui n'est que du code
include/totk_mod_merger.h     contrat C de l'API
../utils/tkmm-oracle/         TKMM de référence pour les comparaisons
```

Les deux crates ne dépendent que de `alloc`. Tout accès aux fichiers passe par
`totk_formats::sys` : `std::fs` avec la feature `std` (le plugin, où Skyline l'implémente sur
`nn::fs`, et les tests sur PC), ou des fonctions `tkm_host_*` fournies par le programme hôte sans
elle — c'est ainsi que totk-mod-manager compile exactement le même code pour
`aarch64-unknown-none`. Les chemins restent écrits `sd:/…` partout (le homebrew les résout en
`sdmc:/…`), si bien que l'index et les empreintes de fusion sont identiques des deux côtés.

```bash
# vérifier la compilation sans std :
cargo +nightly-2024-10-09 build -p totk-merge --no-default-features \
    --target aarch64-unknown-none -Zbuild-std=core,alloc
```

`data/PackFileLookup.pkcache.zs` (index des fichiers internes aux packs) provient de TKMM
(licence MIT, © TKMM-Team).
