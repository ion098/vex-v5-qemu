//! Global Timer Register

pub type XTime = u64;

extern "C" {
    pub fn XTime_GetTime(XTime_Global: *mut XTime);
}
