use std::ops::{Deref, DerefMut};
use std::slice;
use std::mem;

#[cfg(any(target_arch = "x86",
          target_arch = "arm",
          target_arch = "mips",
          target_arch = "mipsel",
          target_arch = "powerpc",
          target_arch = "le32"))]
#[allow(non_camel_case_types)]
type size_t = u32;

#[cfg(any(target_arch = "x86_64",
          target_arch = "aarch64"))]
#[allow(non_camel_case_types)]
type size_t = u64;

#[repr(u8)]
#[allow(non_camel_case_types)]
enum c_void {
    __variant1,
    __variant2,
}

#[link_name = "c"]
extern {
    fn calloc(num: size_t, size: size_t) -> *mut c_void;
    fn free(ptr: *mut c_void);
    fn abort();
}

impl <T: Copy> Drop for ZeroArray<T> {
    fn drop(&mut self) {
        unsafe { free(self.ptr as *mut c_void); }
    }
}

impl <T: Copy> Deref for ZeroArray<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        unsafe { slice::from_raw_parts(self.ptr, self.size) }
    }
}

impl <T: Copy> DerefMut for ZeroArray<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        unsafe { slice::from_raw_parts_mut(self.ptr, self.size) }
    }
}

pub struct ZeroArray<T> where T: Copy {
    ptr: *mut T,
    size: usize,
}

impl <T: Copy> ZeroArray<T> {
    pub unsafe fn new(size: u32) -> ZeroArray<T> {
        let p = calloc(size as size_t, mem::size_of::<T>() as size_t);
        if p.is_null() {  // OOM
            abort();
        }
        ZeroArray {
            ptr: p as *mut T,
            size: size as usize
        }
    }
}