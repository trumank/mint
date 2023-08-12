use anyhow::Result;
use unreal_asset::{
    asset::name_map::NameMap,
    engine_version::EngineVersion,
    exports::{Export, ExportBaseTrait},
    fproperty::{
        FArrayProperty, FBoolProperty, FByteProperty, FClassProperty, FDelegateProperty,
        FEnumProperty, FGenericProperty, FInterfaceProperty, FMapProperty,
        FMulticastDelegateProperty, FMulticastInlineDelegateProperty, FNumericProperty,
        FObjectProperty, FProperty, FSetProperty, FSoftClassProperty, FSoftObjectProperty,
        FStructProperty,
    },
    kismet::{
        ExByteConst, ExCallMath, ExContext, ExDefaultVariable, ExFalse, ExFloatConst,
        ExInstanceVariable, ExIntConst, ExJumpIfNot, ExLet, ExLetObj, ExLocalVariable,
        ExLocalVirtualFunction, ExNameConst, ExNothing, ExObjectConst, ExReturn, ExSelf,
        ExSetArray, ExStringConst, ExStructConst, ExStructMemberContext, ExTextConst, ExTrue,
        FieldPath, KismetExpression, KismetExpressionDataTrait, KismetPropertyPointer,
    },
    object_version::{ObjectVersion, ObjectVersionUE5},
    reader::archive_trait::ArchiveTrait,
    types::PackageIndex,
    Asset, Import,
};

use std::{
    collections::HashMap,
    fs,
    ops::{Deref, DerefMut},
    path::Path,
};
use std::{collections::HashSet, io::Cursor};

/// Holds mutations to bytecode offsets: from -> to
type NamedSpliceMappings = HashMap<(Option<String>, PackageIndex), HashMap<usize, usize>>;
type AssetInstructionMap = HashMap<PackageIndex, Vec<TrackedStatement>>;

#[derive(Clone, Copy)]
pub struct AssetVersion {
    version: ObjectVersion,
    version_ue5: ObjectVersionUE5,
}
impl AssetVersion {
    pub fn new_from<C: std::io::Read + std::io::Seek>(asset: &Asset<C>) -> Self {
        Self {
            version: asset.get_object_version(),
            version_ue5: asset.get_object_version_ue5(),
        }
    }
}

#[derive(Debug)]
pub struct TrackedStatement {
    pub origin: (Option<String>, PackageIndex),
    pub points_to: Option<(Option<String>, PackageIndex)>,
    pub original_offset: Option<usize>,
    pub ex: KismetExpression,
}

impl Deref for TrackedStatement {
    type Target = KismetExpression;
    fn deref(&self) -> &Self::Target {
        &self.ex
    }
}
impl DerefMut for TrackedStatement {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ex
    }
}

#[derive(Debug)]
pub struct Hook<'a> {
    pub function: PackageIndex,
    pub statements: Vec<&'a TrackedStatement>,
    pub start_offset: usize,
    pub end_offset: Option<usize>,
}

pub fn read_asset<P: AsRef<Path>>(
    path: P,
    version: EngineVersion,
) -> Result<Asset<Cursor<Vec<u8>>>> {
    let uasset = Cursor::new(fs::read(&path)?);
    let uexp = Cursor::new(fs::read(path.as_ref().with_extension("uexp"))?);
    let asset = Asset::new(uasset, Some(uexp), version, None)?;

    /*
    let mut out_uasset = Cursor::new(vec![]);
    let mut out_uexp = Cursor::new(vec![]);
    asset.write_data(&mut out_uasset, Some(&mut out_uexp))?;
    if uasset.get_ref() != out_uasset.get_ref() || uexp.get_ref() != out_uexp.get_ref() {
        error!(
            "binary equality not maintained: {}",
            path.as_ref().display()
        );
    } else {
        info!(
            "preliminary binary equality check passed {}",
            path.as_ref().display()
        );
    }
    */

    Ok(asset)
}

fn get_size(ex: &KismetExpression, version: AssetVersion) -> Result<usize> {
    use unreal_asset::kismet::KismetExpressionTrait;
    let mut buf = Cursor::new(vec![]);
    let mut scratch = unreal_asset::reader::raw_writer::RawWriter::new(
        &mut buf,
        version.version,
        version.version_ue5,
        false,
        NameMap::new(),
    );
    Ok(1 + ex.write(&mut scratch)?)
}

/// walk all expressions and subexpressions
pub fn walk(ex: &mut KismetExpression, f: &dyn Fn(&mut KismetExpression)) {
    f(ex);
    match ex {
        KismetExpression::ExFieldPathConst(ex) => walk(&mut ex.value, f),
        KismetExpression::ExSoftObjectConst(ex) => walk(&mut ex.value, f),
        KismetExpression::ExAddMulticastDelegate(ex) => {
            walk(&mut ex.delegate, &f);
            walk(&mut ex.delegate_to_add, &f);
        }
        KismetExpression::ExArrayConst(ex) => ex.elements.iter_mut().for_each(|ex| walk(ex, &f)),
        KismetExpression::ExArrayGetByRef(ex) => {
            walk(&mut ex.array_variable, &f);
            walk(&mut ex.array_index, &f);
        }
        KismetExpression::ExAssert(ex) => walk(&mut ex.assert_expression, f),
        KismetExpression::ExBindDelegate(ex) => {
            walk(&mut ex.delegate, &f);
            walk(&mut ex.object_term, &f);
        }
        KismetExpression::ExCallMath(ex) => ex.parameters.iter_mut().for_each(|ex| walk(ex, &f)),
        KismetExpression::ExCallMulticastDelegate(ex) => {
            ex.parameters.iter_mut().for_each(|ex| walk(ex, &f));
            walk(&mut ex.delegate, &f);
        }
        KismetExpression::ExClassContext(ex) => {
            walk(&mut ex.object_expression, &f);
            walk(&mut ex.context_expression, &f);
        }
        KismetExpression::ExClearMulticastDelegate(ex) => walk(&mut ex.delegate_to_clear, &f),
        KismetExpression::ExComputedJump(ex) => walk(&mut ex.code_offset_expression, &f),
        KismetExpression::ExContext(ex) => {
            walk(&mut ex.object_expression, &f);
            walk(&mut ex.context_expression, &f);
        }
        KismetExpression::ExContextFailSilent(ex) => {
            walk(&mut ex.object_expression, &f);
            walk(&mut ex.context_expression, &f);
        }
        KismetExpression::ExCrossInterfaceCast(ex) => walk(&mut ex.target, &f),
        KismetExpression::ExDynamicCast(ex) => walk(&mut ex.target_expression, &f),
        KismetExpression::ExFinalFunction(ex) => {
            ex.parameters.iter_mut().for_each(|ex| walk(ex, &f))
        }
        KismetExpression::ExInterfaceContext(ex) => walk(&mut ex.interface_value, &f),
        KismetExpression::ExInterfaceToObjCast(ex) => walk(&mut ex.target, &f),
        KismetExpression::ExJumpIfNot(ex) => walk(&mut ex.boolean_expression, &f),
        KismetExpression::ExLet(ex) => walk(&mut ex.expression, &f),
        KismetExpression::ExLetBool(ex) => {
            walk(&mut ex.variable_expression, &f);
            walk(&mut ex.assignment_expression, &f);
        }
        KismetExpression::ExLetDelegate(ex) => {
            walk(&mut ex.variable_expression, &f);
            walk(&mut ex.assignment_expression, &f);
        }
        KismetExpression::ExLetMulticastDelegate(ex) => {
            walk(&mut ex.variable_expression, &f);
            walk(&mut ex.assignment_expression, &f);
        }
        KismetExpression::ExLetObj(ex) => {
            walk(&mut ex.variable_expression, &f);
            walk(&mut ex.assignment_expression, &f);
        }
        KismetExpression::ExLetValueOnPersistentFrame(ex) => {
            walk(&mut ex.assignment_expression, &f);
        }
        KismetExpression::ExLetWeakObjPtr(ex) => {
            walk(&mut ex.variable_expression, &f);
            walk(&mut ex.assignment_expression, &f);
        }
        KismetExpression::ExLocalFinalFunction(ex) => {
            ex.parameters.iter_mut().for_each(|ex| walk(ex, &f))
        }
        KismetExpression::ExLocalVirtualFunction(ex) => {
            ex.parameters.iter_mut().for_each(|ex| walk(ex, &f))
        }
        KismetExpression::ExMapConst(ex) => ex.elements.iter_mut().for_each(|ex| walk(ex, &f)),
        KismetExpression::ExMetaCast(ex) => {
            walk(&mut ex.target_expression, &f);
        }
        KismetExpression::ExObjToInterfaceCast(ex) => {
            walk(&mut ex.target, &f);
        }
        KismetExpression::ExPopExecutionFlowIfNot(ex) => {
            walk(&mut ex.boolean_expression, &f);
        }
        KismetExpression::ExPrimitiveCast(ex) => {
            walk(&mut ex.target, &f);
        }
        KismetExpression::ExRemoveMulticastDelegate(ex) => {
            walk(&mut ex.delegate, &f);
            walk(&mut ex.delegate_to_add, &f);
        }
        KismetExpression::ExReturn(ex) => {
            walk(&mut ex.return_expression, &f);
        }
        KismetExpression::ExSetArray(ex) => {
            if let Some(ex) = ex.assigning_property.as_mut() {
                walk(ex, &f);
            }
            ex.elements.iter_mut().for_each(|ex| walk(ex, &f));
        }
        KismetExpression::ExSetConst(ex) => ex.elements.iter_mut().for_each(|ex| walk(ex, &f)),
        KismetExpression::ExSetMap(ex) => {
            walk(&mut ex.map_property, &f);
            ex.elements.iter_mut().for_each(|ex| walk(ex, &f));
        }
        KismetExpression::ExSetSet(ex) => {
            walk(&mut ex.set_property, &f);
            ex.elements.iter_mut().for_each(|ex| walk(ex, &f));
        }
        KismetExpression::ExSkip(ex) => {
            walk(&mut ex.skip_expression, &f);
        }
        KismetExpression::ExStructConst(ex) => {
            ex.value.iter_mut().for_each(|ex| walk(ex, &f));
        }
        KismetExpression::ExStructMemberContext(ex) => {
            walk(&mut ex.struct_expression, &f);
        }
        KismetExpression::ExSwitchValue(ex) => {
            walk(&mut ex.index_term, &f);
            walk(&mut ex.default_term, &f);
            for case in ex.cases.iter_mut() {
                walk(&mut case.case_index_value_term, &f);
                walk(&mut case.case_term, &f);
            }
        }
        KismetExpression::ExVirtualFunction(ex) => {
            ex.parameters.iter_mut().for_each(|ex| walk(ex, &f))
        }
        _ => {}
    }
}

/// find and shift any ExSwitchValue
/// primarily used to make offsets relative before transformation and return them back to absolute
fn shift_switch(ex: &mut KismetExpression, shift: i32) {
    walk(ex, &|ex| {
        if let KismetExpression::ExSwitchValue(ex) = ex {
            ex.end_goto_offset = ex.end_goto_offset.checked_add_signed(shift).unwrap();
            for case in ex.cases.iter_mut() {
                case.next_offset = case.next_offset.checked_add_signed(shift).unwrap();
            }
        }
    });
}

fn find_ubergraph<C: std::io::Read + std::io::Seek>(asset: &Asset<C>) -> Option<PackageIndex> {
    for (i, e) in asset.asset_data.exports.iter().enumerate() {
        if let unreal_asset::exports::Export::FunctionExport(f) = &e {
            if f.get_base_export()
                .object_name
                .get_content(|s| s.starts_with("ExecuteUbergraph"))
            {
                return Some(PackageIndex::from_export(i as i32).unwrap());
            };
        }
    }
    None
}

fn find_struct_latent_action<C: std::io::Read + std::io::Seek>(
    asset: &Asset<C>,
) -> Option<PackageIndex> {
    asset
        .imports
        .iter()
        .enumerate()
        .find(|(_, i)| {
            i.class_package.get_content(|s| s == "/Script/CoreUObject")
                && i.class_name.get_content(|s| s == "ScriptStruct")
                && i.object_name.get_content(|s| s == "LatentActionInfo")
        })
        .map(|(pi, _)| PackageIndex::from_import(pi as i32).unwrap())
}

/// walk all expressions and subexpressions
#[rustfmt::skip]
pub fn copy_expression<C: std::io::Read + std::io::Seek>(from: &Asset<C>, to: &mut Asset<C>, fn_from: PackageIndex, fn_to: PackageIndex, ex: &KismetExpression) -> KismetExpression {
    match ex {
        KismetExpression::ExLocalVariable(ex) => ExLocalVariable { token: ex.token,
            variable: copy_kismetpropertypointer(from, to, fn_from, fn_to, &ex.variable),
        }.into(),
        KismetExpression::ExInstanceVariable(ex) => ExInstanceVariable { token: ex.token,
            variable: copy_kismetpropertypointer(from, to, fn_from, fn_to, &ex.variable),
        }.into(),
        KismetExpression::ExDefaultVariable(ex) => ExDefaultVariable { token: ex.token,
            variable: copy_kismetpropertypointer(from, to, fn_from, fn_to, &ex.variable),
        }.into(),
        KismetExpression::ExReturn(ex) => ExReturn { token: ex.token,
            return_expression: Box::new(copy_expression(from, to, fn_from, fn_to, &ex.return_expression)),
        }.into(),
        KismetExpression::ExJump(ex) => ex.clone().into(),
        KismetExpression::ExJumpIfNot(ex) => ExJumpIfNot { token: ex.token,
            code_offset: ex.code_offset,
            boolean_expression: Box::new(copy_expression(from, to, fn_from, fn_to, &ex.boolean_expression)),
        }.into(),
        //KismetExpression::ExAssert(ex) => {}
        KismetExpression::ExNothing(ex) => ExNothing { token: ex.token }.into(),
        KismetExpression::ExLet(ex) => ExLet { token: ex.token,
            value: copy_kismetpropertypointer(from, to, fn_from, fn_to, &ex.value),
            variable: Box::new(copy_expression(from, to, fn_from, fn_to, &ex.variable)),
            expression: Box::new(copy_expression(from, to, fn_from, fn_to, &ex.expression)),
        }.into(),
        //KismetExpression::ExClassContext(ex) => {}
        //KismetExpression::ExMetaCast(ex) => {}
        //KismetExpression::ExLetBool(ex) => {}
        //KismetExpression::ExEndParmValue(ex) => {}
        //KismetExpression::ExEndFunctionParms(ex) => {}
        KismetExpression::ExSelf(ex) => ExSelf { token: ex.token }.into(),
        //KismetExpression::ExSkip(ex) => {}
        KismetExpression::ExContext(ex) => ExContext { token: ex.token,
            object_expression: Box::new(copy_expression(from, to, fn_from, fn_to, &ex.object_expression)),
            offset: ex.offset,
            r_value_pointer: copy_kismetpropertypointer(from, to, fn_from, fn_to, &ex.r_value_pointer),
            context_expression: Box::new(copy_expression(from, to, fn_from, fn_to, &ex.context_expression)),
        }.into(),
        //KismetExpression::ExContextFailSilent(ex) => {}
        //KismetExpression::ExVirtualFunction(ex) => {}
        //KismetExpression::ExFinalFunction(ex) => {}
        KismetExpression::ExIntConst(ex) => ExIntConst { token: ex.token, value: ex.value, }.into(),
        KismetExpression::ExFloatConst(ex) => ExFloatConst { token: ex.token, value: ex.value, }.into(),
        KismetExpression::ExStringConst(ex) => ExStringConst { token: ex.token, value: ex.value.clone(), }.into(),
        KismetExpression::ExObjectConst(ex) => ExObjectConst { token: ex.token,
            value: copy_package(from, to, ex.value),
        }.into(),
        KismetExpression::ExNameConst(ex) => ExNameConst { token: ex.token,
            value: to.add_fname(&ex.value.get_owned_content()),
        }.into(),
        //KismetExpression::ExRotationConst(ex) => {}
        //KismetExpression::ExVectorConst(ex) => {}
        KismetExpression::ExByteConst(ex) => ExByteConst { token: ex.token, value: ex.value, }.into(),
        //KismetExpression::ExIntZero(ex) => {}
        //KismetExpression::ExIntOne(ex) => {}
        KismetExpression::ExTrue(ex) => ExTrue { token: ex.token }.into(),
        KismetExpression::ExFalse(ex) => ExFalse { token: ex.token }.into(),
        KismetExpression::ExTextConst(ex) => ExTextConst { token: ex.token, value: ex.value.clone(), }.into(), // TODO: copy text
        //KismetExpression::ExNoObject(ex) => {}
        //KismetExpression::ExTransformConst(ex) => {}
        //KismetExpression::ExIntConstByte(ex) => {}
        //KismetExpression::ExNoInterface(ex) => {}
        //KismetExpression::ExDynamicCast(ex) => {}
        KismetExpression::ExStructConst(ex) => ExStructConst { token: ex.token,
            struct_value: copy_package(from, to, ex.struct_value),
            struct_size: ex.struct_size,
            value: ex.value.iter().map(|ex| copy_expression(from, to, fn_from, fn_to, ex)).collect(),
        }.into(),
        //KismetExpression::ExEndStructConst(ex) => {}
        KismetExpression::ExSetArray(ex) => ExSetArray { token: ex.token,
            assigning_property: ex.assigning_property.as_ref().map(|ex| copy_expression(from, to, fn_from, fn_to, ex).into()),
            array_inner_prop: ex.array_inner_prop.map(|pi| copy_package(from, to, pi)),
            elements: ex.elements.iter().map(|ex| copy_expression(from, to, fn_from, fn_to, ex)).collect(),
        }.into(),
        //KismetExpression::ExEndArray(ex) => {}
        //KismetExpression::ExPropertyConst(ex) => {}
        //KismetExpression::ExUnicodeStringConst(ex) => {}
        //KismetExpression::ExInt64Const(ex) => {}
        //KismetExpression::ExUInt64Const(ex) => {}
        //KismetExpression::ExPrimitiveCast(ex) => {}
        //KismetExpression::ExSetSet(ex) => {}
        //KismetExpression::ExEndSet(ex) => {}
        //KismetExpression::ExSetMap(ex) => {}
        //KismetExpression::ExEndMap(ex) => {}
        //KismetExpression::ExSetConst(ex) => {}
        //KismetExpression::ExEndSetConst(ex) => {}
        //KismetExpression::ExMapConst(ex) => {}
        //KismetExpression::ExEndMapConst(ex) => {}
        KismetExpression::ExStructMemberContext(ex) => ExStructMemberContext { token: ex.token,
            struct_member_expression: copy_kismetpropertypointer(from, to, fn_from, fn_to, &ex.struct_member_expression),
            struct_expression: Box::new(copy_expression(from, to, fn_from, fn_to, &ex.struct_expression)),
        }.into(),
        //KismetExpression::ExLetMulticastDelegate(ex) => {}
        //KismetExpression::ExLetDelegate(ex) => {}
        KismetExpression::ExLocalVirtualFunction(ex) => ExLocalVirtualFunction { token: ex.token,
            virtual_function_name: to.add_fname(&ex.virtual_function_name.get_owned_content()),
            parameters: ex.parameters.iter().map(|ex| copy_expression(from, to, fn_from, fn_to, ex)).collect(),
        }.into(),
        //KismetExpression::ExLocalFinalFunction(ex) => {}
        //KismetExpression::ExLocalOutVariable(ex) => {}
        //KismetExpression::ExDeprecatedOp4A(ex) => {}
        //KismetExpression::ExInstanceDelegate(ex) => {}
        KismetExpression::ExPushExecutionFlow(ex) => ex.clone().into(),
        KismetExpression::ExPopExecutionFlow(ex) => ex.clone().into(),
        //KismetExpression::ExComputedJump(ex) => {}
        //KismetExpression::ExPopExecutionFlowIfNot(ex) => {}
        //KismetExpression::ExBreakpoint(ex) => {}
        //KismetExpression::ExInterfaceContext(ex) => {}
        //KismetExpression::ExObjToInterfaceCast(ex) => {}
        KismetExpression::ExEndOfScript(ex) => ex.clone().into(),
        //KismetExpression::ExCrossInterfaceCast(ex) => {}
        //KismetExpression::ExInterfaceToObjCast(ex) => {}
        //KismetExpression::ExWireTracepoint(ex) => {}
        KismetExpression::ExSkipOffsetConst(ex) => ex.clone().into(),
        //KismetExpression::ExAddMulticastDelegate(ex) => {}
        //KismetExpression::ExClearMulticastDelegate(ex) => {}
        //KismetExpression::ExTracepoint(ex) => {}
        KismetExpression::ExLetObj(ex) => ExLetObj { token: ex.token,
            variable_expression: Box::new(copy_expression(from, to, fn_from, fn_to, &ex.variable_expression)),
            assignment_expression: Box::new(copy_expression(from, to, fn_from, fn_to, &ex.assignment_expression)),
        }.into(),
        //KismetExpression::ExLetWeakObjPtr(ex) => {}
        //KismetExpression::ExBindDelegate(ex) => {}
        //KismetExpression::ExRemoveMulticastDelegate(ex) => {}
        //KismetExpression::ExCallMulticastDelegate(ex) => {}
        //KismetExpression::ExLetValueOnPersistentFrame(ex) => {}
        //KismetExpression::ExArrayConst(ex) => {}
        //KismetExpression::ExEndArrayConst(ex) => {}
        //KismetExpression::ExSoftObjectConst(ex) => {}
        KismetExpression::ExCallMath(ex) => ExCallMath { token: ex.token,
            stack_node: copy_package(from, to, ex.stack_node),
            parameters: ex.parameters.iter().map(|ex| copy_expression(from, to, fn_from, fn_to, ex)).collect(),
        }.into(),
        //KismetExpression::ExSwitchValue(ex) => {}
        //KismetExpression::ExInstrumentationEvent(ex) => {}
        //KismetExpression::ExArrayGetByRef(ex) => {}
        //KismetExpression::ExClassSparseDataVariable(ex) => {}
        //KismetExpression::ExFieldPathConst(ex) => {}
        _ => todo!("{:#?}", ex.get_token()),
    }
}

fn get_generic_property(prop: &FProperty) -> &FGenericProperty {
    match prop {
        FProperty::FGenericProperty(p) => p,
        FProperty::FEnumProperty(p) => &p.generic_property,
        FProperty::FArrayProperty(p) => &p.generic_property,
        FProperty::FSetProperty(p) => &p.generic_property,
        FProperty::FObjectProperty(p) => &p.generic_property,
        FProperty::FSoftObjectProperty(p) => &p.generic_property,
        FProperty::FClassProperty(p) => &p.generic_property,
        FProperty::FSoftClassProperty(p) => &p.generic_property,
        FProperty::FDelegateProperty(p) => &p.generic_property,
        FProperty::FMulticastDelegateProperty(p) => &p.generic_property,
        FProperty::FMulticastInlineDelegateProperty(p) => &p.generic_property,
        FProperty::FInterfaceProperty(p) => &p.generic_property,
        FProperty::FMapProperty(p) => &p.generic_property,
        FProperty::FBoolProperty(p) => &p.generic_property,
        FProperty::FByteProperty(p) => &p.generic_property,
        FProperty::FStructProperty(p) => &p.generic_property,
        FProperty::FNumericProperty(p) => &p.generic_property,
    }
}

fn copy_fgenericproperty<C: std::io::Read + std::io::Seek>(
    _from: &Asset<C>,
    to: &mut Asset<C>,
    p: &FGenericProperty,
) -> FGenericProperty {
    FGenericProperty {
        name: to.add_fname(&p.name.get_owned_content()),
        flags: p.flags,
        array_dim: p.array_dim,
        element_size: p.element_size,
        property_flags: p.property_flags,
        rep_index: p.rep_index,
        rep_notify_func: to.add_fname(&p.rep_notify_func.get_owned_content()),
        blueprint_replication_condition: p.blueprint_replication_condition,
        serialized_type: p
            .serialized_type
            .as_ref()
            .map(|n| to.add_fname(&n.get_owned_content())),
    }
}

fn copy_fproperty<C: std::io::Read + std::io::Seek>(
    from: &Asset<C>,
    to: &mut Asset<C>,
    fp: &FProperty,
) -> FProperty {
    match fp {
        FProperty::FGenericProperty(p) => copy_fgenericproperty(from, to, p).into(),
        FProperty::FEnumProperty(p) => FEnumProperty {
            generic_property: copy_fgenericproperty(from, to, &p.generic_property),
            enum_value: copy_package(from, to, p.enum_value),
            underlying_prop: copy_fproperty(from, to, &p.underlying_prop).into(),
        }
        .into(),
        FProperty::FArrayProperty(p) => FArrayProperty {
            generic_property: copy_fgenericproperty(from, to, &p.generic_property),
            inner: copy_fproperty(from, to, &p.inner).into(),
        }
        .into(),
        FProperty::FSetProperty(p) => FSetProperty {
            generic_property: copy_fgenericproperty(from, to, &p.generic_property),
            element_prop: copy_fproperty(from, to, &p.element_prop).into(),
        }
        .into(),
        FProperty::FObjectProperty(p) => FObjectProperty {
            generic_property: copy_fgenericproperty(from, to, &p.generic_property),
            property_class: copy_package(from, to, p.property_class),
        }
        .into(),
        FProperty::FSoftObjectProperty(p) => FSoftObjectProperty {
            generic_property: copy_fgenericproperty(from, to, &p.generic_property),
            property_class: copy_package(from, to, p.property_class),
        }
        .into(),
        FProperty::FClassProperty(p) => FClassProperty {
            generic_property: copy_fgenericproperty(from, to, &p.generic_property),
            property_class: copy_package(from, to, p.property_class),
            meta_class: copy_package(from, to, p.meta_class),
        }
        .into(),
        FProperty::FSoftClassProperty(p) => FSoftClassProperty {
            generic_property: copy_fgenericproperty(from, to, &p.generic_property),
            property_class: copy_package(from, to, p.property_class),
            meta_class: copy_package(from, to, p.meta_class),
        }
        .into(),
        FProperty::FDelegateProperty(p) => FDelegateProperty {
            generic_property: copy_fgenericproperty(from, to, &p.generic_property),
            signature_function: copy_package(from, to, p.signature_function),
        }
        .into(),
        FProperty::FMulticastDelegateProperty(p) => FMulticastDelegateProperty {
            generic_property: copy_fgenericproperty(from, to, &p.generic_property),
            signature_function: copy_package(from, to, p.signature_function),
        }
        .into(),
        FProperty::FMulticastInlineDelegateProperty(p) => FMulticastInlineDelegateProperty {
            generic_property: copy_fgenericproperty(from, to, &p.generic_property),
            signature_function: copy_package(from, to, p.signature_function),
        }
        .into(),
        FProperty::FInterfaceProperty(p) => FInterfaceProperty {
            generic_property: copy_fgenericproperty(from, to, &p.generic_property),
            interface_class: copy_package(from, to, p.interface_class),
        }
        .into(),
        FProperty::FMapProperty(p) => FMapProperty {
            generic_property: copy_fgenericproperty(from, to, &p.generic_property),
            key_prop: copy_fproperty(from, to, &p.key_prop).into(),
            value_prop: copy_fproperty(from, to, &p.value_prop).into(),
        }
        .into(),
        FProperty::FBoolProperty(p) => FBoolProperty {
            generic_property: copy_fgenericproperty(from, to, &p.generic_property),
            field_size: p.field_size,
            byte_offset: p.byte_offset,
            byte_mask: p.byte_mask,
            field_mask: p.field_mask,
            native_bool: p.native_bool,
            value: p.value,
        }
        .into(),
        FProperty::FByteProperty(p) => FByteProperty {
            generic_property: copy_fgenericproperty(from, to, &p.generic_property),
            enum_value: copy_package(from, to, p.enum_value),
        }
        .into(),
        FProperty::FStructProperty(p) => FStructProperty {
            generic_property: copy_fgenericproperty(from, to, &p.generic_property),
            struct_value: copy_package(from, to, p.struct_value),
        }
        .into(),
        FProperty::FNumericProperty(p) => FNumericProperty {
            generic_property: copy_fgenericproperty(from, to, &p.generic_property),
        }
        .into(),
    }
}

fn copy_kismetpropertypointer<C: std::io::Read + std::io::Seek>(
    from: &Asset<C>,
    to: &mut Asset<C>,
    fn_from: PackageIndex,
    fn_to: PackageIndex,
    p: &KismetPropertyPointer,
) -> KismetPropertyPointer {
    KismetPropertyPointer {
        old: p.old.map(|pi| copy_package(from, to, pi)),
        new: p.new.as_ref().map(|fp| {
            if fp.resolved_owner.index == 0 {
                FieldPath {
                    path: fp
                        .path
                        .iter()
                        .map(|n| to.add_fname(&n.get_owned_content()))
                        .collect(),
                    resolved_owner: fp.resolved_owner,
                }
            } else if fp.resolved_owner == fn_from {
                let ff = if let Some(Export::FunctionExport(f)) = from.get_export(fn_from) {
                    f
                } else {
                    unreachable!("fn_to must be a FunctionExport")
                };

                assert_eq!(fp.path.len(), 1, "path should have only one element");
                let name = &fp.path[0];
                let new_prop = copy_fproperty(
                    from,
                    to,
                    ff.struct_export
                        .loaded_properties
                        .iter()
                        .find(|p| get_generic_property(p).name.eq_content(name))
                        .unwrap_or_else(|| {
                            panic!("invalid property reference {}", name.get_owned_content())
                        }),
                );

                let ft = if let Some(Export::FunctionExport(f)) = to.get_export_mut(fn_to) {
                    f
                } else {
                    unreachable!("fn_to must be a FunctionExport")
                };

                if !ft
                    .struct_export
                    .loaded_properties
                    .iter()
                    .any(|p| get_generic_property(p).name.eq_content(name))
                {
                    ft.struct_export.loaded_properties.push(new_prop);
                } else {
                    // TODO: verify existing prop has the proper type or there's a name conflict
                }

                FieldPath {
                    path: fp
                        .path
                        .iter()
                        .map(|n| to.add_fname(&n.get_owned_content()))
                        .collect(),
                    resolved_owner: fn_to,
                }
            } else if fp.resolved_owner.is_import() {
                copy_fieldpath(from, to, fp)
            } else {
                todo!("resolved_owner != fn_from");
            }
        }),
        //new: p.new.as_ref().map(|fp| copy_fieldpath(from, to, &fp)),
    }
}

fn copy_fieldpath<C: std::io::Read + std::io::Seek>(
    from: &Asset<C>,
    to: &mut Asset<C>,
    path: &FieldPath,
) -> FieldPath {
    FieldPath {
        path: path
            .path
            .iter()
            .map(|n| to.add_fname(&n.get_owned_content()))
            .collect(),
        resolved_owner: copy_package(from, to, path.resolved_owner),
    }
}

fn copy_package<C: std::io::Read + std::io::Seek>(
    from: &Asset<C>,
    to: &mut Asset<C>,
    package: PackageIndex,
) -> PackageIndex {
    if package.is_import() {
        let from_import = from.get_import(package).unwrap();
        let to_outer = copy_package(from, to, from_import.outer_index);
        if let Some(existing) = to.find_import(
            &from_import.class_package,
            &from_import.class_name,
            to_outer,
            &from_import.object_name,
        ) {
            return PackageIndex::new(existing);
        } else {
            let new = Import {
                class_package: to.add_fname(&from_import.class_package.get_owned_content()),
                class_name: to.add_fname(&from_import.class_name.get_owned_content()),
                outer_index: to_outer,
                object_name: to.add_fname(&from_import.object_name.get_owned_content()),
                optional: false,
            };
            return to.add_import(new);
        }
    } else if package.is_export() {
        todo!(
            "{}",
            from.get_export(package)
                .unwrap()
                .get_base_export()
                .object_name
                .get_owned_content()
        )
    }
    package
}

fn to_tracked_statements(
    version: AssetVersion,
    origin: &(Option<String>, PackageIndex),
    exp: Vec<KismetExpression>,
) -> Vec<TrackedStatement> {
    let mut i = 0;
    exp.into_iter()
        .map(|mut ex| {
            let oi = i;
            i += get_size(&ex, version).unwrap();
            shift_switch(&mut ex, -(oi as i32));
            TrackedStatement {
                origin: origin.clone(),
                points_to: None,
                original_offset: Some(oi),
                ex,
            }
        })
        .collect()
}

fn resolve_tracked_statements<C: std::io::Read + std::io::Seek>(
    asset: &Asset<C>,
    mappings: &NamedSpliceMappings,
    inst: Vec<TrackedStatement>,
) -> Vec<KismetExpression> {
    let ubergraph = find_ubergraph(asset);
    let struct_latent_action = find_struct_latent_action(asset);

    inst.into_iter()
        .map(|mut inst| {
            let dest = inst.points_to.as_ref().unwrap_or(&inst.origin);
            match &mut inst.ex {
                // fix jumps into ubergraph
                KismetExpression::ExLocalFinalFunction(ex) => {
                    if Some(ex.stack_node) == ubergraph {
                        match ex.parameters[0] {
                            KismetExpression::ExIntConst(ref mut p) => {
                                p.value = mappings[&(inst.origin.0, ex.stack_node)]
                                    [&(p.value as usize)]
                                    as i32;
                            }
                            _ => todo!("non ExIntConst ubergraph jump"),
                        }
                    }
                }
                KismetExpression::ExJumpIfNot(ex) => {
                    ex.code_offset = mappings[dest][&(ex.code_offset as usize)] as u32;
                }
                KismetExpression::ExJump(ex) => {
                    ex.code_offset = mappings[dest][&(ex.code_offset as usize)] as u32;
                }
                KismetExpression::ExPushExecutionFlow(ex) => {
                    ex.pushing_address = mappings[dest][&(ex.pushing_address as usize)] as u32;
                }
                KismetExpression::ExCallMath(ex) => {
                    if let Some(sla) = struct_latent_action {
                        for p in ex.parameters.iter_mut() {
                            if let KismetExpression::ExStructConst(ref mut p) = p {
                                if p.struct_value == sla {
                                    // split so we can get a mut reference to the offset
                                    let (a, rest) = p.value.split_at_mut(1);
                                    let (b, rest) = rest.split_at_mut(1);
                                    let (c, d) = rest.split_at_mut(1);
                                    if let (
                                        KismetExpression::ExSkipOffsetConst(offset),
                                        KismetExpression::ExIntConst(_int),
                                        KismetExpression::ExNameConst(name),
                                        KismetExpression::ExSelf(_s),
                                    ) = (&mut a[0], &b[0], &mut c[0], &d[0])
                                    {
                                        // TODO: check name actually points to
                                        // ubergraph
                                        offset.value =
                                            mappings[dest][&(offset.value as usize)] as u32;
                                        name.value = asset
                                            .get_export(ubergraph.unwrap())
                                            .unwrap()
                                            .get_base_export()
                                            .object_name
                                            .clone();
                                    } else if let (
                                        KismetExpression::ExIntConst(offset),
                                        KismetExpression::ExIntConst(_int),
                                        KismetExpression::ExNameConst(_name),
                                        KismetExpression::ExSelf(_s),
                                    ) = (&mut a[0], &b[0], &mut c[0], &d[0])
                                    {
                                        // used in LoadAssetClass and is -1
                                        // possibly means no output?
                                        if offset.value != -1 {
                                            offset.value =
                                                mappings[dest][&(offset.value as usize)] as i32;
                                        }
                                    } else {
                                        todo!(
                                            "malformed LatentActionInfo struct {:#?}",
                                            p.value
                                                .iter()
                                                .map(|ex| ex.get_token())
                                                .collect::<Vec<_>>()
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
            inst.ex
        })
        .collect()
}

pub fn extract_tracked_statements<C: std::io::Read + std::io::Seek>(
    asset: &mut Asset<C>,
    version: AssetVersion,
    origin: &Option<String>,
) -> AssetInstructionMap {
    HashMap::from_iter(
        asset
            .asset_data
            .exports
            .iter_mut()
            .enumerate()
            .filter_map(|(i, e)| {
                if let Export::FunctionExport(f) = e {
                    Some((
                        PackageIndex::from_export(i as i32).unwrap(),
                        to_tracked_statements(
                            version,
                            &(origin.clone(), PackageIndex::from_export(i as i32).unwrap()),
                            std::mem::take(&mut f.struct_export.script_bytecode).unwrap(),
                        ),
                    ))
                } else {
                    None
                }
            }),
    )
}
pub fn inject_tracked_statements<C: std::io::Read + std::io::Seek>(
    asset: &mut Asset<C>,
    version: AssetVersion,
    mut statements: AssetInstructionMap,
) {
    let mut mapping = NamedSpliceMappings::new();
    for (_pi, inst) in statements.iter_mut() {
        let mut index = 0;
        for inst in inst {
            if let Some(oo) = inst.original_offset {
                mapping
                    .entry(inst.origin.clone())
                    .or_default()
                    .insert(oo, index);
            }
            let o = index;
            index += get_size(&inst.ex, version).unwrap();
            shift_switch(&mut inst.ex, o as i32);
        }
    }

    for (pi, statements) in statements.into_iter() {
        let bytecode = resolve_tracked_statements(asset, &mapping, statements);
        if let Some(Export::FunctionExport(f)) = asset.get_export_mut(pi) {
            f.struct_export.script_bytecode = Some(bytecode);
        }
    }
}

pub fn find_hooks<'a, C: std::io::Read + std::io::Seek>(
    asset: &'a Asset<C>,
    statements: &'a AssetInstructionMap,
) -> HashMap<String, Hook<'a>> {
    type Address = (PackageIndex, usize);
    let mut mapping: HashMap<Address, &TrackedStatement> = HashMap::new();
    let mut jumps: HashMap<Address, Vec<Address>> = HashMap::new();

    let ubergraph = find_ubergraph(asset);
    let struct_latent_action = find_struct_latent_action(asset);

    for (pi, inst) in statements.iter() {
        let mut iter = inst.iter().peekable();
        while let Some(inst) = iter.next() {
            if let Some(oo) = inst.original_offset {
                let addr = (*pi, oo);
                mapping.insert(addr, inst);
                match &inst.ex {
                    KismetExpression::ExComputedJump(_) => {
                        /* TODO ubergraph */
                        continue;
                    }
                    KismetExpression::ExJump(ex) => {
                        jumps
                            .entry(addr)
                            .or_default()
                            .push((*pi, ex.code_offset as usize));
                        continue;
                    }
                    KismetExpression::ExJumpIfNot(ex) => {
                        jumps
                            .entry(addr)
                            .or_default()
                            .push((*pi, ex.code_offset as usize));
                    }
                    KismetExpression::ExPushExecutionFlow(ex) => {
                        jumps
                            .entry(addr)
                            .or_default()
                            .push((*pi, ex.pushing_address as usize));
                    }
                    KismetExpression::ExPopExecutionFlow(_) => continue,
                    KismetExpression::ExReturn(_) => continue,
                    KismetExpression::ExEndOfScript(_) => continue,
                    KismetExpression::ExCallMath(ex) => {
                        if let Some(sla) = struct_latent_action {
                            for p in &ex.parameters {
                                if let KismetExpression::ExStructConst(p) = p {
                                    if p.struct_value == sla {
                                        if let [KismetExpression::ExSkipOffsetConst(offset), KismetExpression::ExIntConst(_int), KismetExpression::ExNameConst(_name), KismetExpression::ExSelf(_s)] =
                                            &p.value[..]
                                        {
                                            // TODO: check name actually points to
                                            // ubergraph
                                            jumps
                                                .entry(addr)
                                                .or_default()
                                                .push((ubergraph.unwrap(), offset.value as usize));
                                        } else if let [KismetExpression::ExIntConst(offset), KismetExpression::ExIntConst(_int), KismetExpression::ExNameConst(_name), KismetExpression::ExSelf(_s)] =
                                            &p.value[..]
                                        {
                                            // used in LoadAssetClass and is -1
                                            // possibly means no output?
                                            jumps
                                                .entry(addr)
                                                .or_default()
                                                .push((ubergraph.unwrap(), offset.value as usize));
                                        } else {
                                            todo!(
                                                "malformed LatentActionInfo struct {:#?}",
                                                p.value
                                                    .iter()
                                                    .map(|ex| ex.get_token())
                                                    .collect::<Vec<_>>()
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
                if let Some(next) = iter.peek() {
                    if !matches!(next.ex, KismetExpression::ExEndOfScript(_)) {
                        if let Some(oo) = next.original_offset {
                            jumps.entry(addr).or_default().push((*pi, oo));
                        }
                    }
                }
            }
        }
    }

    // print jumps
    /*
    for (a, bs) in &jumps {
        for b in bs {
            trace!("{}:{} -> {}:{}", a.0.index, a.1, b.0.index, b.1);
        }
    }
    */

    let mut hooks = HashMap::new();
    for (addr, statement) in &mapping {
        match &statement.ex {
            KismetExpression::ExLocalVirtualFunction(f)
                if f.virtual_function_name.get_content(|s| s == "HOOK START") =>
            {
                if let [KismetExpression::ExStringConst(s)] = &f.parameters[..] {
                    let function = addr.0;
                    let name = s.value.to_owned();
                    let mut end_offset = None;
                    let start_offset = jumps[addr][0].1;

                    let mut addresses = HashSet::<Address>::new();
                    let mut visited = HashSet::<Address>::new();
                    let mut to_visit = jumps[addr].clone();
                    while let Some(next) = to_visit.pop() {
                        assert_eq!(next.0, function, "Hooks cannot jump between functions");
                        addresses.insert(next);
                        visited.insert(next);
                        let inst = mapping[&next];
                        match &inst.ex {
                            KismetExpression::ExLocalVirtualFunction(f)
                                if f.virtual_function_name.get_content(|s| s == "HOOK END") =>
                            {
                                end_offset = inst.original_offset;
                            }
                            _ => {
                                if let Some(j) = jumps.get(&next) {
                                    to_visit.extend(j.iter().filter(|a| !visited.contains(a)));
                                }
                            }
                        }
                    }
                    let mut statements = addresses.iter().map(|a| mapping[a]).collect::<Vec<_>>();
                    statements.sort_by_key(|s| s.original_offset);
                    hooks.insert(
                        name,
                        Hook {
                            function,
                            statements,
                            start_offset,
                            end_offset,
                        },
                    );
                }
            }
            _ => {}
        }
    }
    hooks
}
