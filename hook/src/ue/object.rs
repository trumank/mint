#![allow(unused_macros)]

use element_ptr::element_ptr;
use std::ptr::NonNull;

use super::*;

bitflags::bitflags! {
    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct EObjectFlags: u32 {
        const RF_NoFlags = 0x0000;
        const RF_Public = 0x0001;
        const RF_Standalone = 0x0002;
        const RF_MarkAsNative = 0x0004;
        const RF_Transactional = 0x0008;
        const RF_ClassDefaultObject = 0x0010;
        const RF_ArchetypeObject = 0x0020;
        const RF_Transient = 0x0040;
        const RF_MarkAsRootSet = 0x0080;
        const RF_TagGarbageTemp = 0x0100;
        const RF_NeedInitialization = 0x0200;
        const RF_NeedLoad = 0x0400;
        const RF_KeepForCooker = 0x0800;
        const RF_NeedPostLoad = 0x1000;
        const RF_NeedPostLoadSubobjects = 0x2000;
        const RF_NewerVersionExists = 0x4000;
        const RF_BeginDestroyed = 0x8000;
        const RF_FinishDestroyed = 0x00010000;
        const RF_BeingRegenerated = 0x00020000;
        const RF_DefaultSubObject = 0x00040000;
        const RF_WasLoaded = 0x00080000;
        const RF_TextExportTransient = 0x00100000;
        const RF_LoadCompleted = 0x00200000;
        const RF_InheritableComponentTemplate = 0x00400000;
        const RF_DuplicateTransient = 0x00800000;
        const RF_StrongRefOnFrame = 0x01000000;
        const RF_NonPIEDuplicateTransient = 0x02000000;
        const RF_Dynamic = 0x04000000;
        const RF_WillBeLoaded = 0x08000000;
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct EFunctionFlags: u32 {
        const FUNC_None = 0x0000;
        const FUNC_Final = 0x0001;
        const FUNC_RequiredAPI = 0x0002;
        const FUNC_BlueprintAuthorityOnly = 0x0004;
        const FUNC_BlueprintCosmetic = 0x0008;
        const FUNC_Net = 0x0040;
        const FUNC_NetReliable = 0x0080;
        const FUNC_NetRequest = 0x0100;
        const FUNC_Exec = 0x0200;
        const FUNC_Native = 0x0400;
        const FUNC_Event = 0x0800;
        const FUNC_NetResponse = 0x1000;
        const FUNC_Static = 0x2000;
        const FUNC_NetMulticast = 0x4000;
        const FUNC_UbergraphFunction = 0x8000;
        const FUNC_MulticastDelegate = 0x00010000;
        const FUNC_Public = 0x00020000;
        const FUNC_Private = 0x00040000;
        const FUNC_Protected = 0x00080000;
        const FUNC_Delegate = 0x00100000;
        const FUNC_NetServer = 0x00200000;
        const FUNC_HasOutParms = 0x00400000;
        const FUNC_HasDefaults = 0x00800000;
        const FUNC_NetClient = 0x01000000;
        const FUNC_DLLImport = 0x02000000;
        const FUNC_BlueprintCallable = 0x04000000;
        const FUNC_BlueprintEvent = 0x08000000;
        const FUNC_BlueprintPure = 0x10000000;
        const FUNC_EditorOnly = 0x20000000;
        const FUNC_Const = 0x40000000;
        const FUNC_NetValidate = 0x80000000;
        const FUNC_AllFlags = 0xffffffff;
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct EClassFlags: i32 {
        const CLASS_None = 0x0000;
        const CLASS_Abstract = 0x0001;
        const CLASS_DefaultConfig = 0x0002;
        const CLASS_Config = 0x0004;
        const CLASS_Transient = 0x0008;
        const CLASS_Parsed = 0x0010;
        const CLASS_MatchedSerializers = 0x0020;
        const CLASS_ProjectUserConfig = 0x0040;
        const CLASS_Native = 0x0080;
        const CLASS_NoExport = 0x0100;
        const CLASS_NotPlaceable = 0x0200;
        const CLASS_PerObjectConfig = 0x0400;
        const CLASS_ReplicationDataIsSetUp = 0x0800;
        const CLASS_EditInlineNew = 0x1000;
        const CLASS_CollapseCategories = 0x2000;
        const CLASS_Interface = 0x4000;
        const CLASS_CustomConstructor = 0x8000;
        const CLASS_Const = 0x00010000;
        const CLASS_LayoutChanging = 0x00020000;
        const CLASS_CompiledFromBlueprint = 0x00040000;
        const CLASS_MinimalAPI = 0x00080000;
        const CLASS_RequiredAPI = 0x00100000;
        const CLASS_DefaultToInstanced = 0x00200000;
        const CLASS_TokenStreamAssembled = 0x00400000;
        const CLASS_HasInstancedReference = 0x00800000;
        const CLASS_Hidden = 0x01000000;
        const CLASS_Deprecated = 0x02000000;
        const CLASS_HideDropDown = 0x04000000;
        const CLASS_GlobalUserConfig = 0x08000000;
        const CLASS_Intrinsic = 0x10000000;
        const CLASS_Constructed = 0x20000000;
        const CLASS_ConfigDoNotCheckDefaults = 0x40000000;
        const CLASS_NewerVersionExists = i32::MIN;
    }


    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct EClassCastFlags : u64
    {
        const CASTCLASS_None = 0x0000000000000000;

        const CASTCLASS_UField = 0x0000000000000001;
        const CASTCLASS_FInt8Property = 0x0000000000000002;
        const CASTCLASS_UEnum = 0x0000000000000004;
        const CASTCLASS_UStruct = 0x0000000000000008;
        const CASTCLASS_UScriptStruct = 0x0000000000000010;
        const CASTCLASS_UClass = 0x0000000000000020;
        const CASTCLASS_FByteProperty = 0x0000000000000040;
        const CASTCLASS_FIntProperty = 0x0000000000000080;
        const CASTCLASS_FFloatProperty = 0x0000000000000100;
        const CASTCLASS_FUInt64Property = 0x0000000000000200;
        const CASTCLASS_FClassProperty = 0x0000000000000400;
        const CASTCLASS_FUInt32Property = 0x0000000000000800;
        const CASTCLASS_FInterfaceProperty = 0x0000000000001000;
        const CASTCLASS_FNameProperty = 0x0000000000002000;
        const CASTCLASS_FStrProperty = 0x0000000000004000;
        const CASTCLASS_FProperty = 0x0000000000008000;
        const CASTCLASS_FObjectProperty = 0x0000000000010000;
        const CASTCLASS_FBoolProperty = 0x0000000000020000;
        const CASTCLASS_FUInt16Property = 0x0000000000040000;
        const CASTCLASS_UFunction = 0x0000000000080000;
        const CASTCLASS_FStructProperty = 0x0000000000100000;
        const CASTCLASS_FArrayProperty = 0x0000000000200000;
        const CASTCLASS_FInt64Property = 0x0000000000400000;
        const CASTCLASS_FDelegateProperty = 0x0000000000800000;
        const CASTCLASS_FNumericProperty = 0x0000000001000000;
        const CASTCLASS_FMulticastDelegateProperty = 0x0000000002000000;
        const CASTCLASS_FObjectPropertyBase = 0x0000000004000000;
        const CASTCLASS_FWeakObjectProperty = 0x0000000008000000;
        const CASTCLASS_FLazyObjectProperty = 0x0000000010000000;
        const CASTCLASS_FSoftObjectProperty = 0x0000000020000000;
        const CASTCLASS_FTextProperty = 0x0000000040000000;
        const CASTCLASS_FInt16Property = 0x0000000080000000;
        const CASTCLASS_FDoubleProperty = 0x0000000100000000;
        const CASTCLASS_FSoftClassProperty = 0x0000000200000000;
        const CASTCLASS_UPackage = 0x0000000400000000;
        const CASTCLASS_ULevel = 0x0000000800000000;
        const CASTCLASS_AActor = 0x0000001000000000;
        const CASTCLASS_APlayerController = 0x0000002000000000;
        const CASTCLASS_APawn = 0x0000004000000000;
        const CASTCLASS_USceneComponent = 0x0000008000000000;
        const CASTCLASS_UPrimitiveComponent = 0x0000010000000000;
        const CASTCLASS_USkinnedMeshComponent = 0x0000020000000000;
        const CASTCLASS_USkeletalMeshComponent = 0x0000040000000000;
        const CASTCLASS_UBlueprint = 0x0000080000000000;
        const CASTCLASS_UDelegateFunction = 0x0000100000000000;
        const CASTCLASS_UStaticMeshComponent = 0x0000200000000000;
        const CASTCLASS_FMapProperty = 0x0000400000000000;
        const CASTCLASS_FSetProperty = 0x0000800000000000;
        const CASTCLASS_FEnumProperty = 0x0001000000000000;
        const CASTCLASS_USparseDelegateFunction = 0x0002000000000000;
        const CASTCLASS_FMulticastInlineDelegateProperty = 0x0004000000000000;
        const CASTCLASS_FMulticastSparseDelegateProperty = 0x0008000000000000;
        const CASTCLASS_FFieldPathProperty = 0x0010000000000000;
        const CASTCLASS_FLargeWorldCoordinatesRealProperty = 0x0080000000000000;
        const CASTCLASS_FOptionalProperty = 0x0100000000000000;
        const CASTCLASS_FVerseValueProperty = 0x0200000000000000;
        const CASTCLASS_UVerseVMClass = 0x0400000000000000;
    }

    #[derive(Debug, Clone)]
    #[repr(C)]
    pub struct  EPropertyFlags: u64 {
        const CPF_None = 0x0000;
        const CPF_Edit = 0x0001;
        const CPF_ConstParm = 0x0002;
        const CPF_BlueprintVisible = 0x0004;
        const CPF_ExportObject = 0x0008;
        const CPF_BlueprintReadOnly = 0x0010;
        const CPF_Net = 0x0020;
        const CPF_EditFixedSize = 0x0040;
        const CPF_Parm = 0x0080;
        const CPF_OutParm = 0x0100;
        const CPF_ZeroConstructor = 0x0200;
        const CPF_ReturnParm = 0x0400;
        const CPF_DisableEditOnTemplate = 0x0800;
        const CPF_Transient = 0x2000;
        const CPF_Config = 0x4000;
        const CPF_DisableEditOnInstance = 0x00010000;
        const CPF_EditConst = 0x00020000;
        const CPF_GlobalConfig = 0x00040000;
        const CPF_InstancedReference = 0x00080000;
        const CPF_DuplicateTransient = 0x00200000;
        const CPF_SaveGame = 0x01000000;
        const CPF_NoClear = 0x02000000;
        const CPF_ReferenceParm = 0x08000000;
        const CPF_BlueprintAssignable = 0x10000000;
        const CPF_Deprecated = 0x20000000;
        const CPF_IsPlainOldData = 0x40000000;
        const CPF_RepSkip = 0x80000000;
        const CPF_RepNotify = 0x100000000;
        const CPF_Interp = 0x200000000;
        const CPF_NonTransactional = 0x400000000;
        const CPF_EditorOnly = 0x800000000;
        const CPF_NoDestructor = 0x1000000000;
        const CPF_AutoWeak = 0x4000000000;
        const CPF_ContainsInstancedReference = 0x8000000000;
        const CPF_AssetRegistrySearchable = 0x10000000000;
        const CPF_SimpleDisplay = 0x20000000000;
        const CPF_AdvancedDisplay = 0x40000000000;
        const CPF_Protected = 0x80000000000;
        const CPF_BlueprintCallable = 0x100000000000;
        const CPF_BlueprintAuthorityOnly = 0x200000000000;
        const CPF_TextExportTransient = 0x400000000000;
        const CPF_NonPIEDuplicateTransient = 0x800000000000;
        const CPF_ExposeOnSpawn = 0x1000000000000;
        const CPF_PersistentInstance = 0x2000000000000;
        const CPF_UObjectWrapper = 0x4000000000000;
        const CPF_HasGetValueTypeHash = 0x8000000000000;
        const CPF_NativeAccessSpecifierPublic = 0x10000000000000;
        const CPF_NativeAccessSpecifierProtected = 0x20000000000000;
        const CPF_NativeAccessSpecifierPrivate = 0x40000000000000;
        const CPF_SkipSerialization = 0x80000000000000;
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct UObjectBase {
    pub vtable: *const c_void,
    pub object_flags: EObjectFlags,
    pub internal_index: i32,
    pub class_private: *const UClass,
    pub name_private: FName,
    pub outer_private: *const UObject,
}

#[derive(Debug)]
#[repr(C)]
pub struct UObjectBaseUtility {
    pub uobject_base: UObjectBase,
}

#[derive(Debug)]
#[repr(C)]
pub struct UObject {
    pub uobject_base_utility: UObjectBaseUtility,
}

#[derive(Debug)]
#[repr(C)]
pub struct FOutputDevice {
    vtable: *const c_void,
    b_suppress_event_tag: bool,
    b_auto_emit_line_terminator: bool,
}

#[derive(Debug)]
#[repr(C)]
pub struct UField {
    pub uobject: UObject,
    pub next: *const UField,
}

#[derive(Debug)]
#[repr(C)]
pub struct FStructBaseChain {
    pub struct_base_chain_array: *const *const FStructBaseChain,
    pub num_struct_bases_in_chain_minus_one: i32,
}

#[derive(Debug)]
#[repr(C)]
pub struct FFieldClass {
    pub name: FName,
    pub id: u64,
    pub cast_flags: EClassCastFlags,
    pub class_flags: EClassFlags,
    pub super_class: *const FFieldClass,
    pub default_object: *const FField,
    pub construct_fn:
        extern "system" fn(*const FFieldVariant, *const FName, EObjectFlags) -> *const FField,
    pub unqiue_name_index_counter: i32, //FThreadSafeCounter,
}

#[derive(Debug)]
#[repr(C)]
pub struct FFieldVariant {
    container: *const c_void,
    b_is_uobject: bool,
}

#[derive(Debug)]
#[repr(C)]
pub struct FField {
    pub vtable: *const c_void,
    pub class_private: *const FFieldClass,
    pub owner: FFieldVariant,
    pub next: *const FField,
    pub name_private: FName,
    pub flags_private: EObjectFlags,
}

#[derive(Debug)]
#[repr(C)]
pub struct FProperty {
    pub ffield: FField,
    pub array_dim: i32,
    pub element_size: i32,
    pub property_flags: EPropertyFlags,
    pub rep_index: u16,
    pub blueprint_replication_condition: u8, //TEnumAsByte<enum ELifetimeCondition>,
    pub offset_internal: i32,
    pub rep_notify_func: FName,
    pub property_link_next: *const FProperty,
    pub next_ref: *const FProperty,
    pub destructor_link_next: *const FProperty,
    pub post_construct_link_next: *const FProperty,
}

#[derive(Debug)]
#[repr(C)]
pub struct UStruct {
    pub ufield: UField,
    pub fstruct_base_chain: FStructBaseChain,
    pub super_struct: *const UStruct,
    pub children: *const UField,
    pub child_properties: *const FField,
    pub properties_size: i32,
    pub min_alignment: i32,
    pub script: TArray<u8>,
    pub property_link: *const FProperty,
    pub ref_link: *const FProperty,
    pub destructor_link: *const FProperty,
    pub post_construct_link: *const FProperty,
    pub script_and_property_object_references: TArray<*const UObject>,
    pub unresolved_script_properties: *const (), //TODO pub TArray<TTuple<TFieldPath<FField>,int>,TSizedDefaultAllocator<32> >*
    pub unversioned_schema: *const (),           //TODO const FUnversionedStructSchema*
}

#[derive(Debug)]
#[repr(C)]
pub struct UFunction {
    pub ustruct: UStruct,
    pub function_flags: EFunctionFlags,
    pub num_parms: u8,
    pub parms_size: u16,
    pub return_value_offset: u16,
    pub rpc_id: u16,
    pub rpc_response_id: u16,
    pub first_property_to_init: *const FProperty,
    pub event_graph_function: *const UFunction,
    pub event_graph_call_offset: i32,
    pub func: unsafe extern "system" fn(*mut UObject, *mut kismet::FFrame, *mut c_void),
}

#[derive(Debug)]
#[repr(C)]
pub struct UClass {
    pub ustruct: UStruct,
}

#[rustfmt::skip]
macro_rules! decl_uobject_base {
    ($parent_member:expr) => {
        unsafe fn uobject_base(self) -> NonNull<UObjectBase> where Self: Sized;
    };
}
#[rustfmt::skip]
macro_rules! impl_uobject_base {
    ($parent_member:expr) => {
        unsafe fn uobject_base(self) -> NonNull<UObjectBase> {
            ::element_ptr::element_ptr!(self => .$parent_member)
        }
    };
}
pub trait UObjectBaseTrait {
    unsafe fn get_path_name(self, stop_outer: Option<NonNull<UObject>>) -> String
    where
        Self: Sized;
    unsafe fn class(self) -> Option<NonNull<UClass>>
    where
        Self: Sized;
}
impl UObjectBaseTrait for NonNull<UObjectBase> {
    unsafe fn get_path_name(self, stop_outer: Option<NonNull<UObject>>) -> String {
        let mut string = FString::new();
        (globals().uobject_base_utility_get_path_name())(self, stop_outer, &mut string);
        string.to_string()
    }
    unsafe fn class(self) -> Option<NonNull<UClass>> {
        element_ptr!(self => .class_private.*).nn()
    }
}

#[rustfmt::skip]
macro_rules! decl_uobject_base_utility {
    ($parent_member:expr) => {
        decl_uobject_base!($parent_member.uobject_base);
        unsafe fn uobject_base_utility(self) -> NonNull<UObjectBaseUtility> where Self: Sized;
    };
}
#[rustfmt::skip]
macro_rules! impl_uobject_base_utility {
    ($parent_member:expr) => {
        impl_uobject_base!($parent_member.uobject_base);
        unsafe fn uobject_base_utility(self) -> NonNull<UObjectBaseUtility> {
            ::element_ptr::element_ptr!(self => .$parent_member)
        }
    };
}
#[allow(unused)]
pub trait UObjectBaseUtilityTrait {
    decl_uobject_base!(uobject_base);
}
impl UObjectBaseUtilityTrait for NonNull<UObjectBaseUtility> {
    impl_uobject_base!(uobject_base);
}

#[rustfmt::skip]
macro_rules! decl_uobject {
    ($parent_member:expr) => {
        decl_uobject_base_utility!($parent_member.uobject_base_utility);
        unsafe fn uobject(self) -> NonNull<UObject> where Self: Sized;
    };
}
#[rustfmt::skip]
macro_rules! impl_uobject {
    ($parent_member:expr) => {
        impl_uobject_base_utility!($parent_member.uobject_base_utility);
        unsafe fn uobject(self) -> NonNull<UObject> {
            ::element_ptr::element_ptr!(self => .$parent_member)
        }
    };
}
#[allow(unused)]
pub trait UObjectTrait {
    decl_uobject_base_utility!(uobject_base_utility);
}
impl UObjectTrait for NonNull<UObject> {
    impl_uobject_base_utility!(uobject_base_utility);
}

#[rustfmt::skip]
macro_rules! decl_ufield {
    ($parent_member:expr) => {
        decl_uobject!($parent_member.uobject);
        unsafe fn ufield(self) -> NonNull<UField> where Self: Sized;
    };
}
#[rustfmt::skip]
macro_rules! impl_ufield {
    ($parent_member:expr) => {
        impl_uobject!($parent_member.uobject);
        unsafe fn ufield(self) -> NonNull<UField> {
            ::element_ptr::element_ptr!(self => .$parent_member)
        }
    };
}
#[allow(unused)]
pub trait UFieldTrait {
    decl_uobject!(uobject);
}
impl UFieldTrait for NonNull<UField> {
    impl_uobject!(uobject);
}

#[rustfmt::skip]
macro_rules! decl_ustruct {
    ($parent_member:expr) => {
        decl_ufield!($parent_member.ufield);
        unsafe fn ustruct(self) -> NonNull<UStruct> where Self: Sized;
    };
}
#[rustfmt::skip]
macro_rules! impl_ustruct {
    ($parent_member:expr) => {
        impl_ufield!($parent_member.ufield);
        unsafe fn ustruct(self) -> NonNull<UStruct> {
            ::element_ptr::element_ptr!(self => .$parent_member)
        }
    };
}
#[allow(unused)]
pub trait UStructTrait {
    decl_ufield!(ufield);
    unsafe fn child_properties(self)
    where
        Self: Sized;
}
impl UStructTrait for NonNull<UStruct> {
    impl_ufield!(ufield);
    unsafe fn child_properties(self) {
        let mut next_ustruct = Some(self);

        while let Some(ustruct) = next_ustruct {
            let name = element_ptr!(ustruct.uobject_base() => .name_private.*);
            println!("class={name}");

            let mut next_field: Option<NonNull<FField>> =
                element_ptr!(ustruct => .child_properties.*).nn();
            let mut i = 0;
            while let Some(field) = next_field {
                if element_ptr!(field => .class_private.*.cast_flags.*)
                    .contains(EClassCastFlags::CASTCLASS_FProperty)
                {
                    let prop: NonNull<FProperty> = field.cast();
                    let offset: i32 = element_ptr!(prop => .offset_internal.*);

                    let name = element_ptr!(field => .name_private.*);
                    let string = name.to_string();
                    println!("{i}: 0x{offset:X} {string}");
                    i += 1;
                }
                next_field = element_ptr!(field => .next.*).nn();
            }
            next_ustruct = element_ptr!(ustruct => .super_struct.*).nn();
        }
    }
}

#[rustfmt::skip]
macro_rules! decl_ufunction {
    ($parent_member:expr) => {
        decl_ustruct!($parent_member.ustruct);
        unsafe fn ufunction(self) -> NonNull<UFunction> where Self: Sized;
    };
}
#[rustfmt::skip]
macro_rules! impl_ufunction {
    ($parent_member:expr) => {
        impl_ustruct!($parent_member.ustruct);
        unsafe fn ufunction(self) -> NonNull<UFunction> {
            ::element_ptr::element_ptr!(self => .$parent_member)
        }
    };
}
#[allow(unused)]
pub trait UFunctionTrait {
    decl_ustruct!(ustruct);
}
impl UFunctionTrait for NonNull<UFunction> {
    impl_ustruct!(ustruct);
}

#[allow(unused)]
pub trait UClassTrait {
    decl_ustruct!(ustruct);
}
impl UClassTrait for NonNull<UClass> {
    impl_ustruct!(ustruct);
}

pub trait NN<T> {
    fn nn(self) -> Option<NonNull<T>>;
}
impl<T> NN<T> for *const T {
    fn nn(self) -> Option<NonNull<T>> {
        NonNull::new(self.cast_mut())
    }
}
impl<T> NN<T> for *mut T {
    fn nn(self) -> Option<NonNull<T>> {
        NonNull::new(self)
    }
}
trait CastOptionNN<T, O> {
    fn cast(self) -> Option<NonNull<O>>;
}
impl<T, O> CastOptionNN<T, O> for Option<NonNull<T>> {
    fn cast(self) -> Option<NonNull<O>> {
        self.map(|s| s.cast())
    }
}
