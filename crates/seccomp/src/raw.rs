//! The raw FFI to libseccomp.
//! This is all unsafe. Use it only if you understand what values should be returned,
//! what objects you need to manage, etc.
#![allow(non_camel_case_types)]

use std::ffi::{c_char, c_int, c_uint, c_void};

/// A SECCOMP context.
pub type scmp_filter_ctx = *mut c_void;

/// Syscall data.
#[repr(C)]
pub struct seccomp_data {
    pub nr: c_int,
    pub arch: u32,
    pub instruction_pointer: u64,
    pub args: [u64; 6],
}

/// A notification from the kernel.
#[repr(C)]
pub struct seccomp_notif {
    pub id: u64,
    pub pid: u32,
    pub flags: u32,
    pub data: seccomp_data,
}

/// A Notification Response structure.
#[repr(C)]
pub struct seccomp_notif_resp {
    pub id: u64,
    pub val: i64,
    pub error: i32,
    pub flags: u32,
}

/// Kill the process
pub static SCMP_ACT_KILL_PROCESS: u32 = 0x80000000;

/// Kill the thread
pub static SCMP_ACT_KILL_THREAD: u32 = 0x00000000;

/// Trap Signal
pub static SCMP_ACT_TRAP: u32 = 0x00030000;

/// Request a decision from the notify monitor.
pub static SCMP_ACT_NOTIFY: u32 = 0x7fc00000;

/// Log the request to the Audit log
pub static SCMP_ACT_LOG: u32 = 0x7ffc0000;

/// Allow the action.
pub static SCMP_ACT_ALLOW: u32 = 0x7fff0000;

/// Attributes. ACT_DEFAULT is not included because seccomp_init already takes it.
#[repr(C)]
pub enum scmp_filter_attr {
    SCMP_FLTATR_ACT_BADARCH = 2,
    SCMP_FLTATR_CTL_NNP = 3,
    SCMP_FLTATR_CTL_TSYNC = 4,
    SCMP_FLTATR_API_TSKIP = 5,
    SCMP_FLTATR_CTL_LOG = 6,
    SCMP_FLTATR_CTL_SSB = 7,
    SCMP_FLTATR_CTL_OPTIMIZE = 8,
    SCMP_FLTATR_API_SYSRAWRC = 9,
}

#[link(name = "seccomp")]
unsafe extern "C" {

    /// Get the current API level.
    ///
    /// 0. Reserved.
    /// 1. Base Level
    /// 2. Support for TSYNC.
    /// 3. Support for LOG.
    /// 4. Support for KILL_PROCESS
    /// 5. Support for NOTIFY
    /// 6. Simultaneous support for TSYNC and NOTIFY.
    ///
    /// Note: This function is never used by wrappers. You are
    /// expected to test the API if you want to use a particular feature.
    pub fn seccomp_api_get() -> c_uint;

    /// Initialize a SECCOMP context
    pub fn seccomp_init(def_action: u32) -> scmp_filter_ctx;

    /// Release a context.
    pub fn seccomp_release(ctx: scmp_filter_ctx);

    /// Set an attribute. See seccomp.h for expected values, or see the Attributes trait.
    pub fn seccomp_attr_set(ctx: scmp_filter_ctx, attr: scmp_filter_attr, value: u32) -> c_int;

    /// Resolve names, like "ptrace" to the associated number for the current architecture
    pub fn seccomp_syscall_resolve_name(name: *const c_char) -> c_int;

    /// Resolve a syscall number with an architecture to the name.
    pub fn seccomp_syscall_resolve_num_arch(arch_token: u32, num: c_int) -> *mut c_char;

    pub fn seccomp_syscall_resolve_name_arch(arch_token: u32, name: *const c_char) -> c_int;

    /// Get the native architecture.
    pub fn seccomp_arch_native() -> u32;

    /// Add a rule.
    pub fn seccomp_rule_add(
        ctx: scmp_filter_ctx,
        action: u32,
        syscall: c_int,
        arg_cnt: c_uint,
        ...
    ) -> c_int;

    /// Set the priority of a syscall.
    pub fn seccomp_set_priority(ctx: scmp_filter_attr, syscall: c_int, priority: u8) -> c_int;

    /// Export the filter to BPF for Bubblewrap.
    pub fn seccomp_export_bpf(ctx: scmp_filter_ctx, fd: c_int) -> c_int;

    /// Load the filter into the current process.
    pub fn seccomp_load(ctx: scmp_filter_ctx) -> c_int;

    /// Allocate a notification pair.
    pub fn seccomp_notify_alloc(
        req: *mut *mut seccomp_notif,
        resp: *mut *mut seccomp_notif_resp,
    ) -> c_int;

    /// Free a notification pair.
    pub fn seccomp_notify_free(req: *mut seccomp_notif, resp: *mut seccomp_notif_resp);

    /// Receive an event from the kernel.
    pub fn seccomp_notify_receive(fd: c_int, req: *mut seccomp_notif) -> c_int;

    /// Send a response to an event.
    pub fn seccomp_notify_respond(fd: c_int, resp: *mut seccomp_notif_resp) -> c_int;

    /// Check if a event is still valid.
    pub fn seccomp_notify_id_valid(fd: c_int, id: u64) -> c_int;

    /// Get the Notify FD to receive and respond over.
    pub fn seccomp_notify_fd(ctx: scmp_filter_ctx) -> c_int;
}
