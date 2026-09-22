//! How much life an enemy has, worked out from the game's own files.
//!
//! Every enemy is an actor with a pack of its own,
//! `Pack/Actor/<actor>.pack.zs`, holding the parameters of its components:
//!
//! ```text
//! Actor/<actor>.engine__actor__ActorParam.bgyml       Components.LifeRef ->
//! Component/LifeParam/<x>.game__component__LifeParam.bgyml   LifeParameters ->
//! Life/LifeParameters/<y>.game__life__LifeParameters.bgyml   MaxLife: 72
//! ```
//!
//! Each of those files may leave a field out and inherit it from the file its
//! `$parent` names, which is how a blue Bokoblin is described as "a red
//! Bokoblin, but...". The chase below follows those links, through the packs
//! of the actors it goes by.
//!
//! Nothing here knows where the files come from: inside the game they are read
//! through nn::fs (so the merged files, mods included), on a PC from a romfs
//! folder. That is what makes `cargo run --example enemy_hp` and the plugin
//! give the same answer.

use totk_formats::byml::Byml;
use totk_formats::sarc::Sarc;

/// Reads a file of the game by its romfs-relative path, decompressed.
pub type Files<'a> = &'a mut dyn FnMut(&str) -> Option<Vec<u8>>;

/// How far a `$parent` chain is followed before giving up.
const MAX_STEPS: usize = 12;

/// Where an enemy's life is written down.
#[derive(Debug, Clone)]
pub struct Life {
    pub max: i32,
    /// The pack holding the file that sets it, romfs-relative.
    pub pack: String,
    /// The file inside that pack.
    pub entry: String,
}

/// The file a reference names, as it is stored:
/// `"Work/Life/LifeParameters/X.game__life__LifeParameters.gyml"` and
/// `"?Component/LifeParam/Y.game__component__LifeParam.bgyml"` both become a
/// path inside the actor's pack.
fn referenced(reference: &str) -> String {
    let path = reference.trim_start_matches('?');
    let path = path.strip_prefix("Work/").unwrap_or(path);
    match path.strip_suffix(".gyml") {
        Some(stem) => format!("{}.bgyml", stem),
        None => path.to_string(),
    }
}

/// The actor a file inside a pack belongs to, e.g.
/// `"Actor/Enemy_Bokoblin_Junior.engine__actor__ActorParam.bgyml"` ->
/// `"Enemy_Bokoblin_Junior"`.
fn actor_of(entry: &str) -> &str {
    let name = entry.rsplit('/').next().unwrap_or(entry);
    name.split('.').next().unwrap_or(name)
}

fn node_at<'a>(node: &'a Byml, path: &[&str]) -> Option<&'a Byml> {
    let mut current = node;
    for key in path {
        current = current.as_map()?.get(*key)?;
    }
    Some(current)
}

/// The pack an actor's parameters live in.
pub fn pack_of(actor: &str) -> String {
    format!("Pack/Actor/{}.pack.zs", actor)
}

fn entry_of(pack: &[u8], entry: &str) -> Option<Byml> {
    let sarc = Sarc::parse(pack).ok()?;
    let data = sarc.get(entry)?.data;
    Byml::parse(data).ok().map(|(node, _)| node)
}

/// Where the chase is: the pack being read, and the file inside it.
struct Cursor {
    path: String,
    pack: Vec<u8>,
    entry: String,
}

impl Cursor {
    /// The file the cursor points at, wherever it turns out to live.
    fn read(&mut self, files: Files) -> Option<Byml> {
        if let Some(node) = entry_of(&self.pack, &self.entry) {
            return Some(node);
        }
        // Not in this pack: the actor it names has one of its own, and a few
        // files sit in the romfs on their own.
        let path = pack_of(actor_of(&self.entry));
        if let Some(pack) = files(&path) {
            if let Some(node) = entry_of(&pack, &self.entry) {
                self.path = path;
                self.pack = pack;
                return Some(node);
            }
        }
        let data = files(&self.entry)?;
        self.path = self.entry.clone();
        Byml::parse(&data).ok().map(|(node, _)| node)
    }
}

/// Follows `$parent` from the cursor until `path` is set, and returns it.
fn chase(files: Files, cursor: &mut Cursor, path: &[&str]) -> Option<Byml> {
    for _ in 0..MAX_STEPS {
        let node = cursor.read(files)?;
        if let Some(found) = node_at(&node, path) {
            return Some(found.clone());
        }
        let parent = node_at(&node, &["$parent"])?.as_str()?.to_string();
        cursor.entry = referenced(&parent);
    }
    None
}

/// The health an enemy starts a fight with, and the file that sets it.
/// `None` when the actor has no life at all (a trap, a prop), or is not
/// installed.
pub fn life(files: Files, actor: &str) -> Option<Life> {
    let mut cursor = Cursor {
        path: pack_of(actor),
        pack: files(&pack_of(actor))?,
        entry: format!("Actor/{}.engine__actor__ActorParam.bgyml", actor),
    };

    let life_ref = chase(files, &mut cursor, &["Components", "LifeRef"])?;
    cursor.entry = referenced(life_ref.as_str()?);

    let parameters = chase(files, &mut cursor, &["LifeParameters"])?;
    cursor.entry = referenced(parameters.as_str()?);

    // The chase leaves the cursor on the file that sets MaxLife, which is
    // what a mod has to change (see the make_enemy_hp_mod example).
    let max = match chase(files, &mut cursor, &["MaxLife"])? {
        Byml::Int(value) => value,
        Byml::UInt32(value) => value as i32,
        Byml::Float(value) => value as i32,
        _ => return None,
    };
    Some(Life {
        max,
        pack: cursor.path.clone(),
        entry: cursor.entry.clone(),
    })
}

/// Only the number, for callers that have no use for where it comes from.
pub fn max_life(files: Files, actor: &str) -> Option<i32> {
    life(files, actor).map(|life| life.max)
}

/// The enemies the plugin reports on when the mod folder holds no list of its
/// own. Every one of them is in the game since 1.0.
pub const DEFAULT_ENEMIES: &[&str] = &[
    "Enemy_Bokoblin_Junior",
    "Enemy_Bokoblin_Middle",
    "Enemy_Bokoblin_Senior",
    "Enemy_Bokoblin_Dark",
    "Enemy_Bokoblin_Bone_Junior",
    "Enemy_Horablin_Junior",
    "Enemy_Moriblin_Junior",
    "Enemy_Moriblin_Middle",
    "Enemy_Lizalfos_Junior",
    "Enemy_Lizalfos_Middle",
    "Enemy_Chuchu_Junior",
    "Enemy_Octarock_Forest",
    "Enemy_Keese_Fire",
    "Enemy_Zombie_Junior",
    "Enemy_Wizzrobe_Fire",
    "Enemy_Golem_Junior",
    "Enemy_Giant_Junior",
    "Enemy_Lynel_Junior",
    "Enemy_Lynel_Senior",
];
