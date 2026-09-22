//! How TKMM identifies array entries in BYML documents.
//!
//! Many arrays in TotK are really keyed tables ("Actors" keyed by "Hash",
//! "BoneList" by "BoneName", ...). TKMM records edits to them by key instead of
//! position so that two mods touching different rows of the same table both
//! apply. The tables below are ports of BymlArrayChangelogBuilderProvider and
//! BymlMergerKeyNameProvider and must stay in step with them.

use core::hash::{Hash, Hasher};

use hashbrown::HashMap;
use totk_formats::byml::Byml;

use crate::prelude::*;

/// Field(s) an array entry is identified by. A dotted name reads a nested map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyName {
    pub primary: &'static str,
    pub secondary: Option<&'static str>,
}

const fn key(primary: &'static str) -> Option<KeyName> {
    Some(KeyName {
        primary,
        secondary: None,
    })
}

const fn pair(primary: &'static str, secondary: &'static str) -> Option<KeyName> {
    Some(KeyName {
        primary,
        secondary: Some(secondary),
    })
}

/// The value of an entry's key field(s).
#[derive(Debug, Clone, Default)]
pub struct BymlKey {
    pub primary: Option<Byml>,
    pub secondary: Option<Byml>,
}

impl BymlKey {
    pub fn is_empty(&self) -> bool {
        self.primary.is_none()
    }

    pub fn matches(&self, other: &BymlKey) -> bool {
        totk_formats::byml::option_value_eq(&self.primary, &other.primary)
            && totk_formats::byml::option_value_eq(&self.secondary, &other.secondary)
    }

    /// A hashable stand-in. Like .NET's hash codes, floating point values hash
    /// by their exact bits, so keyed lookups need exact matches.
    pub fn repr(&self) -> KeyRepr {
        KeyRepr(scalar_repr(&self.primary), scalar_repr(&self.secondary))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyRepr(ScalarRepr, ScalarRepr);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ScalarRepr {
    Missing,
    Null,
    String(String),
    Bool(bool),
    Int(i32),
    UInt32(u32),
    Int64(i64),
    UInt64(u64),
    Float(u32),
    Double(u64),
    Other(u64),
}

fn scalar_repr(node: &Option<Byml>) -> ScalarRepr {
    match node {
        None => ScalarRepr::Missing,
        Some(Byml::Null) => ScalarRepr::Null,
        Some(Byml::String(s)) => ScalarRepr::String(s.clone()),
        Some(Byml::Bool(v)) => ScalarRepr::Bool(*v),
        Some(Byml::Int(v)) => ScalarRepr::Int(*v),
        Some(Byml::UInt32(v)) => ScalarRepr::UInt32(*v),
        Some(Byml::Int64(v)) => ScalarRepr::Int64(*v),
        Some(Byml::UInt64(v)) => ScalarRepr::UInt64(*v),
        Some(Byml::Float(v)) => ScalarRepr::Float(v.to_bits()),
        Some(Byml::Double(v)) => ScalarRepr::Double(v.to_bits()),
        Some(other) => {
            let mut hasher = core::hash::BuildHasher::build_hasher(&foldhash::quality::FixedState::with_seed(0));
            format!("{:?}", other).hash(&mut hasher);
            ScalarRepr::Other(hasher.finish())
        }
    }
}

impl KeyName {
    /// BymlKeyName.GetKey: an empty key unless `node` is a map.
    pub fn get_key(&self, node: &Byml) -> BymlKey {
        let Some(map) = node.as_map() else {
            return BymlKey::default();
        };
        BymlKey {
            primary: lookup(map, self.primary),
            secondary: self.secondary.and_then(|name| lookup(map, name)),
        }
    }

    /// BymlKeyName.TryGetKey.
    pub fn try_get_key(&self, node: &Byml) -> Option<BymlKey> {
        let key = self.get_key(node);
        (!key.is_empty()).then_some(key)
    }
}

fn lookup(map: &totk_formats::byml::Map, path: &str) -> Option<Byml> {
    match path.split_once('.') {
        None => map.get(path).cloned(),
        Some((first, rest)) => map.get(first)?.as_map().and_then(|nested| lookup(nested, rest)),
    }
}

/// Index of every keyed entry of an array (BymlExtensions.CreateIndexCache):
/// the last entry wins when two share a key.
pub fn create_index_cache(array: &[Byml], key_name: &KeyName) -> HashMap<KeyRepr, usize> {
    let mut cache = HashMap::new();
    if array.is_empty() || (array.len() == 1 && array[0].as_map().is_none()) {
        return cache;
    }
    for (index, entry) in array.iter().enumerate() {
        match key_name.try_get_key(entry) {
            Some(key) => {
                cache.insert(key.repr(), index);
            }
            None => crate::debug!(
                "vanilla entry {} does not have the key field {:?}",
                index,
                key_name.primary
            ),
        }
    }
    cache
}

/// The bgyml type TKMM derives from a file name: "Foo.game__Bar.bgyml" →
/// "game__Bar".
pub fn bgyml_type(canonical: &str) -> String {
    let name = canonical.rsplit('/').next().unwrap_or(canonical);
    let without_extension = match name.rfind('.') {
        Some(index) => &name[..index],
        None => name,
    };
    match without_extension.rfind('.') {
        Some(index) => without_extension[index + 1..].to_string(),
        None => String::new(),
    }
}

/// How a changelog is recorded for a named array (changelog building side).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrayBuilder {
    /// Entries matched by value.
    Default,
    /// Entries matched by key field(s).
    Keyed(KeyName),
    /// "Property" arrays, matched by their NameHash.
    NameHash,
    /// Entries compared position by position.
    DirectIndex,
}

const LOCATION_AREA_ARRAYS: &[&str] = &[
    "CaveEntranceNormal", "CaveEntranceSpecial", "CaveEntranceWell", "CheckPoint", "City", "District",
    "DragonTears", "Dungeon", "Ground", "ShopArmor", "ShopDye", "ShopGeneral", "ShopInn", "ShopJewelry", "Shrine",
    "SkyArchipelago", "SpotBig", "SpotBigArtifact", "SpotBigMagma", "SpotBigMountain", "SpotBigOther",
    "SpotBigTimber", "SpotBigWater", "SpotBigWithNameIcon", "SpotMiddle", "SpotMiddleArtifact", "SpotMiddleMagma",
    "SpotMiddleMountain", "SpotMiddleOther", "SpotMiddleTimber", "SpotMiddleWater", "SpotSmallArtifact",
    "SpotSmallMagma", "SpotSmallMountain", "SpotSmallOther", "SpotSmallTimber", "SpotSmallWater", "Stable", "Tower",
    "Underground",
];

const NAME_KEYED_ARRAYS: &[&str] = &[
    "AnimationDrive", "CColEntityNamePathAry", "CColSensorNamePathAry", "CheckPointSetting", "Cloth",
    "ClothAdvandecOption", "ClothList", "ClothReaction", "CollectItem", "CollidableList",
    "ControllerEntityNamePathAry", "ControllerSensorNamePathAry", "ControllerSensorUnitAry",
    "ExternalShapeNamePathAry", "HelperBoneList", "IntData", "LookIKControllerNamePathAry",
    "LookingControllerNamePathAry", "MatterRigidBodyNamePathAry", "MeshList", "ParamTable", "RagdollReaction",
    "RagdollReactionList", "RagdollStructure", "Reaction", "RigidBodyEntityNamePathAry",
    "RigidBodySensorNamePathAry", "ShapeList", "ShapeNamePathAry", "StringData", "TailBoneControllerNamePathAry",
    "Node",
];

const BLACKBOARD_ARRAYS: &[&str] = &[
    "BlackboardParamBoolArray", "BlackboardParamCustomTypeArray", "BlackboardParamF32Array",
    "BlackboardParamMtx33fArray", "BlackboardParamMtx34fArray", "BlackboardParamPtrArray",
    "BlackboardParamQuatfArray", "BlackboardParamS32Array", "BlackboardParamS8Array",
    "BlackboardParamStringArray", "BlackboardParamU32Array", "BlackboardParamU64Array", "BlackboardParamU8Array",
    "BlackboardParamVec3fArray", "EditBBParams",
];

const COMPONENT_NAME_ARRAYS: &[&str] = &[
    "CharacterComponentPresetCollection", "LayerHitMaskEntityCollection", "LayerHitMaskSensorCollection",
    "MaterialCollection", "MaterialPresetCollection", "MotionPropertiesCollection",
    "PhysicsMaterialMappingInfoCollection", "SubLayerHitMaskEntityCollection", "SubLayerHitMaskSensorCollection",
    "UserShapeTagMaskCollection",
];

/// Key names shared by both providers. `None` means "not a keyed array".
fn common_key_name(name: &str, bgyml_type: &str, depth: i32) -> Option<KeyName> {
    match name {
        "Animal" | "Enemy" | "FallFloorInsect" | "Fish" | "GrassCut" | "Insect" | "NotDecayedLargeSwordList"
        | "NotDecayedSmallSwordList" | "NotDecayedSpearList" | "RainBonusMaterial" | "Seafood"
        | "SpObjCapsuleBlockMaster" | "Weapon" | "bow" | "bows" | "shields" | "weapons" | "helmets" => key("name"),
        "Actors" => match bgyml_type {
            "bcett" => key("Hash"),
            "game__component__ArmyManagerParam" => key("ActorName"),
            _ => None,
        },
        "Table" => match bgyml_type {
            "game__colorvariation__ConversionActorNameToParasailPatternSetTable" => key("ParasailPattern"),
            "game__ecosystem__DecayedWeaponMappingTable" => key("EquipmentDeathCountGmdHash"),
            _ => None,
        },
        "BoneInfoArray" => match bgyml_type {
            "game__component__DragonParam" => key("Hash"),
            "phive__LookIKResourceHeaderParam" | "phive__TailBoneResourceHeaderParam" => key("BoneName"),
            _ => None,
        },
        "Elements" => match bgyml_type {
            "game__enemy__DrakeSubModelInfo" => key("BoneName"),
            "game__gamebalance__LevelSensorTargetDefine" => pair("ActorNameHash", "Plus"),
            _ => None,
        },
        "Items" => match bgyml_type {
            "game__pouchcontent__EnhancementMaterial" => key("Actor"),
            _ => None,
        },
        "List" => match bgyml_type {
            "game__sound__ShrineSpotBgmTypeInfoList" => key("DungeonIndexStr"),
            _ => None,
        },
        "Contents" => match bgyml_type {
            "game__ui__FairyFountainGlobalSetting" => key("Actor"),
            _ => None,
        },
        "SettingTable" => match bgyml_type {
            "game__ui__LargeDungeonFloorDefaultSettingTable" => key("DungeonType"),
            _ => None,
        },
        "BrainVerbs" => key("ActionSeqContainer"),
        "ActionVerbContainerElements" => key("ActionVerb"),
        "ResidentActors" | "Settings" | "ShootableShareActorSettings" | "GoodsList" => key("Actor"),
        "BindActorInfo" => key("ActorHolderKey"),
        "RegisteredActorArray" | "RequirementList" | "Rewards" => key("ActorName"),
        "SharpInfoList" | "SharpInfoBowList" | "SharpInfoShieldList" => key("ActorNameHash"),
        "PictureBookParamArray" => key("ActorNameShort"),
        "NavMeshObjects" => key("Alias"),
        "AliasEntityList" => key("AliasEntity"),
        "AliasSensorList" => key("AliasSensor"),
        "Anchors" => key("AnchorName"),
        "ArmorEffect" => key("ArmorEffectType"),
        "HornTypeAndAttachmentMapping" => pair("AttachmentName", "HornBoneName"),
        "AttackParams" => key("AttackType"),
        n if BLACKBOARD_ARRAYS.contains(&n) => key("BBKey"),
        "DragonInfoList" => key("BindPointRespawnGameDataHash"),
        "OperationAngular" | "OperationLinear" => key("Body"),
        "BindBoneList" | "BoneList" | "BoneModifierSet" | "Bones" | "FruitOffsetTranslation" | "ModelBindSettings"
        | "StickWeaponBone" => key("BoneName"),
        "Categories" => key("CallbackName"),
        "CaveParams" => key("CaveInstanceId"),
        "CheckList" => key("CheckType"),
        "Object" => key("ChemicalMaterial"),
        n if COMPONENT_NAME_ARRAYS.contains(&n) => key("ComponentName"),
        "HingeArray" | "RangeArray" => key("ConstraintName"),
        "CropYieldTable" => key("CropName"),
        "DungeonBossDifficultyGameData" => key("DefeatedNumGameDataHash"),
        "DoCondition" | "FinCondition" | "PickConditions" | "SuccessCondition" => key("DefineNameHash"),
        "FallenActorTable" => key("DropActorName"),
        "SmallDungeonLocationList" => key("DungeonIndexStr"),
        "VariationListForArmorDye" => key("DyeColor"),
        "HackEquip" => key("EquipUserBbKey"),
        "AdventureMemorySetArray" | "GlobalResidentEventList" => key("EventName"),
        "AutoPlayBoneVisibilities" | "AutoPlayMaterials" => key("FileName"),
        "WinningRateTable" => key("FlintstonesNum"),
        "ModelVariationAnims" => key("Fmab"),
        "OptionParam" => key("FootIKMode"),
        "PartialList" => key("GameData"),
        "PlacementGroups" => key("GroupID"),
        "EffectLimiterGroup" | "HiddenMaterialGroupList" => key("GroupName"),
        "Textures" => key("guid"),
        "Points" => key("Hash"),
        "Rails" => match depth {
            0 => key("Hash"),
            _ => None,
        },
        "HeadshotDamageParameters" => key("HeadshotBoneName"),
        "TransitionParam" => key("Index"),
        "OverwriteParam" => key("InstanceId"),
        "Interests" | "StrongInterests" => key("InterestType"),
        "ConditionArray" | "OverrideASEvReactVerbSettings" | "SwitchParam" | "TriggerParams" | "Triggers" => {
            key("Key")
        }
        "OverrideASEventReactSettings" => pair("Key", "Intensity"),
        "OverrideReactionVerbSettings" => pair("KeyActionVerb", "Setting.OverrideActionVerb"),
        "ShootableActorSettings" => pair("KeyHash", "Actor"),
        "AttachmentGroupList" | "EnemyGroupList" => key("Label"),
        "ShopWeaponGroupList" | "WeaponGroupList" => pair("Label", "EquipmentType"),
        "ActionSeqs" => key("LabelHash"),
        n if LOCATION_AREA_ARRAYS.contains(&n) => match bgyml_type {
            "locationarea" => key("LocationName"),
            _ => None,
        },
        "MiasmaAreaParam" => key("MiasmaAreaType"),
        n if NAME_KEYED_ARRAYS.contains(&n) => key("Name"),
        "TowingHookParams" => key("NameHash"),
        "ExtraNewsSourceInfo" | "TopNewsSourceInfo" => key("NewsKeyName"),
        "PictureBookPackInfoArray" => key("PackActor"),
        "VariationListForParasail" => key("Pattern"),
        "PhshMesh" => key("PhshMeshPath"),
        "PlgGdTable" => key("PlgGuid"),
        "PropertyDefinitions" | "Sources" => key("PropertyNameHash"),
        "Connections" => key("RailHash"),
        "CustomCullInfos" | "SpecialCullInfos" => key("ResourceName"),
        "Seats" => key("RidableType"),
        "VisibleSageOnNonMember" => key("SageType"),
        "SeriesArmorEffectList" => key("SeriesName"),
        "CropActorTable" => key("SrcActorName"),
        "SuspiciosBuffs" => key("SuspiciousBuffType"),
        "BoostOnlyTable" => key("Target"),
        "PartialConfigs" => key("TargetName"),
        "OverwritePropertiesEffect" | "OverwritePropertiesSound" => key("TargetTypeNameHash"),
        "BGParamArray" => key("TexName"),
        "TipsSetArray" => key("TipsType"),
        "TmbMesh" => key("TmbMeshPath"),
        "ActorPositionData" | "EventEntry" => key("$type"),
        "SB" | "T" | "U" => key("Umii"),
        "AlreadyReadInfo" => key("UpdateGameDataFlag"),
        "ConditionList" => key("WeaponEssence"),
        "WeaponTypeAndSubModelMapping" => key("WeaponType"),
        _ => None,
    }
}

/// BymlMergerKeyNameProvider.GetKeyName.
pub fn merger_key_name(name: &str, bgyml_type: &str, depth: i32) -> Option<KeyName> {
    match name {
        "Property" => key("NameHash"),
        // Workaround for selective DefaultValue arrays in GDL
        "DefaultValue" => match bgyml_type {
            "Struct" => key("Hash"),
            _ => None,
        },
        _ => common_key_name(name, bgyml_type, depth),
    }
}

/// BymlArrayChangelogBuilderProvider.GetChangelogBuilder.
pub fn array_builder(name: &str, bgyml_type: &str, depth: i32) -> ArrayBuilder {
    match name {
        "Property" => return ArrayBuilder::NameHash,
        "Translate" | "Rotate" | "Scale" | "MarginNegative" | "MarginPositive" | "Rot" | "Trans" | "Pivot"
        | "PlayerPosOnClearEvent" | "EnokidaCameraPos" | "WeakPointActorArray" | "WeakPointUser"
        | "NearWoodStoragePos" | "StaffRollSetArray" => return ArrayBuilder::DirectIndex,
        "ArmorCategory" | "ArmorDyeColor" | "ArmorSeries" | "BowEffect" | "FoodEffect"
        | "MaterialBowAttachmentTag" | "MaterialCategory" | "ShieldEffect" | "WeaponCategory" | "WeaponEffect" => {
            return match bgyml_type {
                "game__ui__PouchSortTable" => ArrayBuilder::DirectIndex,
                _ => ArrayBuilder::Default,
            }
        }
        "RecipeArray" => {
            return match bgyml_type {
                "game__cooking__RecipeCardTable" => ArrayBuilder::DirectIndex,
                _ => ArrayBuilder::Default,
            }
        }
        _ => {}
    }

    match common_key_name(name, bgyml_type, depth) {
        Some(key_name) => ArrayBuilder::Keyed(key_name),
        None if is_known_array(name) => ArrayBuilder::Default,
        None => match bgyml_type {
            "game__component__ConditionParam" => ArrayBuilder::DirectIndex,
            _ => ArrayBuilder::Default,
        },
    }
}

/// Names the builder provider lists explicitly, whose fallback is the default
/// builder rather than the ConditionParam special case.
fn is_known_array(name: &str) -> bool {
    matches!(
        name,
        "Actors" | "Table" | "BoneInfoArray" | "Elements" | "Items" | "List" | "Contents" | "SettingTable" | "Rails"
    ) || LOCATION_AREA_ARRAYS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_bgyml_types() {
        assert_eq!(
            bgyml_type("Component/ArmyManagerParam/Foo.game__component__ArmyManagerParam.bgyml"),
            "game__component__ArmyManagerParam"
        );
        assert_eq!(bgyml_type("Banc/Foo.bcett.byml"), "bcett");
        assert_eq!(bgyml_type("Foo.byml"), "");
    }

    #[test]
    fn providers_agree_with_tkmm() {
        assert_eq!(merger_key_name("Actors", "bcett", 1), key("Hash"));
        assert_eq!(merger_key_name("Actors", "", 1), None);
        assert_eq!(merger_key_name("Rails", "", 0), key("Hash"));
        assert_eq!(merger_key_name("Rails", "", 1), None);
        assert_eq!(array_builder("Property", "", 0), ArrayBuilder::NameHash);
        assert_eq!(array_builder("Translate", "", 0), ArrayBuilder::DirectIndex);
        assert_eq!(array_builder("Actors", "game__component__ConditionParam", 0), ArrayBuilder::Default);
        assert_eq!(array_builder("Whatever", "game__component__ConditionParam", 0), ArrayBuilder::DirectIndex);
        assert_eq!(array_builder("Bones", "", 0), ArrayBuilder::Keyed(KeyName { primary: "BoneName", secondary: None }));
        // "DefaultValue" is only keyed for merging, and only in GDL structs.
        assert_eq!(array_builder("DefaultValue", "Struct", 0), ArrayBuilder::Default);
        assert_eq!(merger_key_name("DefaultValue", "Struct", 0), key("Hash"));
    }

    #[test]
    fn keys_read_nested_fields() {
        let mut setting = totk_formats::byml::Map::new();
        setting.insert("OverrideActionVerb".into(), Byml::from("Jump"));
        let mut entry = totk_formats::byml::Map::new();
        entry.insert("KeyActionVerb".into(), Byml::from("Run"));
        entry.insert("Setting".into(), Byml::Map(setting));
        let node = Byml::Map(entry);

        let name = pair("KeyActionVerb", "Setting.OverrideActionVerb").unwrap();
        let key = name.get_key(&node);
        assert_eq!(key.primary, Some(Byml::from("Run")));
        assert_eq!(key.secondary, Some(Byml::from("Jump")));
    }
}
