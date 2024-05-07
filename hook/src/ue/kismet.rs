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

impl FFrame {
    pub fn arg<T: Sized + Default>(self: &mut FFrame) -> T {
        let mut ret: T = Default::default();
        unsafe {
            let ptr = &mut ret as *mut T as *mut _;

            if self.code.is_null() {
                let cur = self.property_chain_for_compiled_in;
                self.property_chain_for_compiled_in = (*cur).next;
                (globals().fframe_step_explicit_property())(self, ptr, cur as *const FProperty);
            } else {
                (globals().fframe_step())(self, self.object, ptr);
            }
        }
        ret
    }
}
