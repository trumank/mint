use std::{ffi::c_void, fmt::Display};

use crate::globals;

pub type FnFFrameStep =
    unsafe extern "system" fn(stack: &mut kismet::FFrame, *mut UObject, result: *mut c_void);
pub type FnFFrameStepExplicitProperty = unsafe extern "system" fn(
    stack: &mut kismet::FFrame,
    result: *mut c_void,
    property: *const FProperty,
);

pub type FnFNameToString = unsafe extern "system" fn(&FName, &mut FString);
impl Display for FName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut string = FString::new();
        unsafe {
            (globals().fname_to_string())(self, &mut string);
        };
        write!(f, "{string}")
    }
}

pub type FnUObjectBaseUtilityGetPathName =
    unsafe extern "system" fn(&UObjectBase, Option<&UObject>, &mut FString);
impl UObjectBase {
    pub fn get_path_name(&self, stop_outer: Option<&UObject>) -> String {
        let mut string = FString::new();
        unsafe {
            (globals().uobject_base_utility_get_path_name())(self, stop_outer, &mut string);
        }
        string.to_string()
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct FMalloc {
    vtable: *const FMallocVTable,
}
unsafe impl Sync for FMalloc {}
unsafe impl Send for FMalloc {}
impl FMalloc {
    pub fn malloc(&self, count: usize, alignment: u32) -> *mut c_void {
        unsafe { ((*self.vtable).malloc)(self, count, alignment) }
    }
    pub fn realloc(&self, original: *mut c_void, count: usize, alignment: u32) -> *mut c_void {
        unsafe { ((*self.vtable).realloc)(self, original, count, alignment) }
    }
    pub fn free(&self, original: *mut c_void) {
        unsafe { ((*self.vtable).free)(self, original) }
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct FMallocVTable {
    pub __vec_del_dtor: *const (),
    pub exec: *const (),
    pub malloc:
        unsafe extern "system" fn(this: &FMalloc, count: usize, alignment: u32) -> *mut c_void,
    pub try_malloc:
        unsafe extern "system" fn(this: &FMalloc, count: usize, alignment: u32) -> *mut c_void,
    pub realloc: unsafe extern "system" fn(
        this: &FMalloc,
        original: *mut c_void,
        count: usize,
        alignment: u32,
    ) -> *mut c_void,
    pub try_realloc: unsafe extern "system" fn(
        this: &FMalloc,
        original: *mut c_void,
        count: usize,
        alignment: u32,
    ) -> *mut c_void,
    pub free: unsafe extern "system" fn(this: &FMalloc, original: *mut c_void),
    pub quantize_size: *const (),
    pub get_allocation_size: *const (),
    pub trim: *const (),
    pub setup_tls_caches_on_current_thread: *const (),
    pub clear_and_disable_tlscaches_on_current_thread: *const (),
    pub initialize_stats_metadata: *const (),
    pub update_stats: *const (),
    pub get_allocator_stats: *const (),
    pub dump_allocator_stats: *const (),
    pub is_internally_thread_safe: *const (),
    pub validate_heap: *const (),
    pub get_descriptive_name: *const (),
}

bitflags::bitflags! {
    #[derive(Debug, Clone)]
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
}
bitflags::bitflags! {
    #[derive(Debug, Clone)]
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
struct FFieldClass {
    // TODO
    name: FName,
}

#[derive(Debug)]
#[repr(C)]
struct FFieldVariant {
    container: *const c_void,
    b_is_uobject: bool,
}

#[derive(Debug)]
#[repr(C)]
pub struct FField {
    class_private: *const FFieldClass,
    owner: FFieldVariant,
    next: *const FField,
    name_private: FName,
    flags_private: EObjectFlags,
}

pub struct FProperty {
    // TODO
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

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct FName {
    pub comparison_index: FNameEntryId,
    pub number: u32,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct FNameEntryId {
    pub value: u32,
}

pub type FString = TArray<u16>;

#[derive(Debug)]
#[repr(C)]
pub struct TArray<T> {
    data: *const T,
    num: i32,
    max: i32,
}
impl<T> TArray<T> {
    fn new() -> Self {
        Self {
            data: std::ptr::null(),
            num: 0,
            max: 0,
        }
    }
}
impl<T> Drop for TArray<T> {
    fn drop(&mut self) {
        unsafe {
            std::ptr::drop_in_place(std::ptr::slice_from_raw_parts_mut(
                self.data.cast_mut(),
                self.num as usize,
            ))
        }
        globals().gmalloc().free(self.data as *mut c_void);
    }
}
impl<T> Default for TArray<T> {
    fn default() -> Self {
        Self {
            data: std::ptr::null(),
            num: 0,
            max: 0,
        }
    }
}
impl<T> TArray<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            data: globals().gmalloc().malloc(
                capacity * std::mem::size_of::<T>(),
                std::mem::align_of::<T>() as u32,
            ) as *const T,
            num: 0,
            max: capacity as i32,
        }
    }
    pub fn len(&self) -> usize {
        self.num as usize
    }
    pub fn capacity(&self) -> usize {
        self.max as usize
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn as_slice(&self) -> &[T] {
        if self.num == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.data, self.num as usize) }
        }
    }
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        if self.num == 0 {
            &mut []
        } else {
            unsafe { std::slice::from_raw_parts_mut(self.data as *mut _, self.num as usize) }
        }
    }
    pub fn clear(&mut self) {
        let elems: *mut [T] = self.as_mut_slice();

        unsafe {
            self.num = 0;
            std::ptr::drop_in_place(elems);
        }
    }
    pub fn reserve(&mut self, additional: usize) {
        if self.num + additional as i32 >= self.max {
            self.max = u32::next_power_of_two((self.max + additional as i32) as u32) as i32;
            let new = globals().gmalloc().realloc(
                self.data as *mut c_void,
                self.max as usize * std::mem::size_of::<T>(),
                std::mem::align_of::<T>() as u32,
            ) as *const T;
            self.data = new;
        }
    }
    pub fn push(&mut self, new_value: T) {
        self.reserve(1);
        unsafe {
            std::ptr::write(self.data.add(self.num as usize).cast_mut(), new_value);
        }
        self.num += 1;
    }
    pub fn extend_from_slice(&mut self, other: &[T])
    where
        T: Copy,
    {
        self.reserve(other.len());
        // TODO optimize
        for elm in other {
            self.push(*elm);
        }
    }
}

impl<T> From<&[T]> for TArray<T>
where
    T: Copy,
{
    fn from(value: &[T]) -> Self {
        let mut new = Self::with_capacity(value.len());
        new.extend_from_slice(value);
        new
    }
}
impl From<&str> for FString {
    fn from(value: &str) -> Self {
        Self::from(
            widestring::U16CString::from_str(value)
                .unwrap()
                .as_slice_with_nul(),
        )
    }
}

impl Display for FString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let slice = self.as_slice();
        let last = slice.len()
            - slice
                .iter()
                .cloned()
                .rev()
                .position(|c| c != 0)
                .unwrap_or_default();
        write!(
            f,
            "{}",
            widestring::U16Str::from_slice(&slice[..last])
                .to_string()
                .unwrap()
        )
    }
}

#[derive(Debug, Default)]
#[repr(C)]
pub struct FVector {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Default)]
#[repr(C)]
pub struct FLinearColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

pub mod kismet {
    use super::*;

    #[derive(Debug)]
    #[repr(C)]
    pub struct FFrame {
        pub base: FOutputDevice,
        pub node: *const c_void,
        pub object: *mut UObject,
        pub code: *const c_void,
        pub locals: *const c_void,
        pub most_recent_property: *const FProperty,
        pub most_recent_property_address: *const c_void,
        pub flow_stack: [u8; 0x30],
        pub previous_frame: *const FFrame,
        pub out_parms: *const c_void,
        pub property_chain_for_compiled_in: *const FField,
        pub current_native_function: *const c_void,
        pub b_array_context_failed: bool,
    }

    pub fn arg<T: Sized>(stack: &mut FFrame, output: &mut T) {
        let output = output as *const _ as *mut _;
        unsafe {
            if stack.code.is_null() {
                let cur = stack.property_chain_for_compiled_in;
                stack.property_chain_for_compiled_in = (*cur).next;
                (globals().fframe_step_explicit_property())(stack, output, cur as *const FProperty);
            } else {
                (globals().fframe_step())(stack, stack.object, output);
            }
        }
    }
}
