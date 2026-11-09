//! Minimal ELF64 loader for the embedded kernel.
//!
//! Loads PT_LOAD segments into LOADER_DATA pages at their p_vaddr and
//! returns the kernel entry virtual address. No dynamic linking, no
//! relocations beyond what the kernel's static link already resolved -
//! the kernel is built position-dependent for Phase 1.

use uefi::table::boot::{AllocateType, BootServices, MemoryType};

#[repr(C)]
#[derive(Clone, Copy)]
struct Elf64Ehdr {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Elf64Phdr {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

const PT_LOAD: u32 = 1;

/// Parse 'elf_bytes', copy every PT_LOAD segment into freshly allocated
/// pages at p_vaddr, and return the kernel entry virtual address.
pub fn load(elf_bytes: &[u8], bs: &BootServices) -> u64 {
    if elf_bytes.len() < core::mem::size_of::<Elf64Ehdr>() {
        panic!("kernel ELF too small");
    }
    // SAFETY: we trust the embedded kernel ELF produced by our own build.
    // The length check above guards the read; alignment of &[u8] is 1 but
    // we cast through a raw pointer - ELF64 structs are naturally aligned.
    let ehdr: &Elf64Ehdr =
        unsafe { &*(elf_bytes.as_ptr() as *const Elf64Ehdr) };

    // Sanity: ELF magic + 64-bit class.
    if &ehdr.e_ident[..4] != b"\x7fELF" || ehdr.e_ident[4] != 2 {
        panic!("kernel ELF: bad magic/class");
    }

    let ph_off = ehdr.e_phoff as usize;
    let ph_size = ehdr.e_phentsize as usize;
    let ph_num = ehdr.e_phnum as usize;

    for i in 0..ph_num {
        let off = ph_off + i * ph_size;
        if off + core::mem::size_of::<Elf64Phdr>() > elf_bytes.len() {
            break;
        }
        // SAFETY: bounds-checked above; program headers are u64-aligned.
        let phdr: &Elf64Phdr =
            unsafe { &*(elf_bytes[off..].as_ptr() as *const Elf64Phdr) };
        if phdr.p_type != PT_LOAD {
            continue;
        }
        copy_segment(elf_bytes, phdr, bs);
    }

    ehdr.e_entry
}

fn copy_segment(elf_bytes: &[u8], phdr: &Elf64Phdr, bs: &BootServices) {
    let vaddr = phdr.p_vaddr;
    let filesz = phdr.p_filesz as usize;
    let memsz = phdr.p_memsz as usize;

    let pages = (memsz + 0xFFF) / 0x1000;
    if pages == 0 {
        return;
    }
    let base = bs
        .allocate_pages(
            AllocateType::Address(vaddr),
            MemoryType::LOADER_DATA,
            pages,
        )
        .unwrap_or_else(|_| {
            // If the exact address is taken, Phase 1 panics so the build
            // catches kernel/linker address conflicts early. Phase 2 will
            // allocate anywhere and remap via page_tables.
            panic!("kernel segment address conflict");
        });

    // Zero the whole memsz (covers .bss), then copy filesz from the ELF.
    // SAFETY: freshly allocated LOADER_DATA pages, owned solely by us;
    // src/dst do not overlap because dst is zeroed first.
    unsafe {
        core::ptr::write_bytes(base as *mut u8, 0, memsz);
        let src = elf_bytes.as_ptr().add(phdr.p_offset as usize);
        core::ptr::copy_nonoverlapping(src, base as *mut u8, filesz);
    }
}
