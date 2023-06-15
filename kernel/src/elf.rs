use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use bitflags::bitflags;
use elf::endian::AnyEndian;
use elf::segment::ProgramHeader;
use elf::{ElfBytes, ParseError};
use x86_64::VirtAddr;

/// Wrapper around a parsed ELF header for executables.
pub(crate) struct ElfExecutableHeader<'a> {
    parsed: ElfBytes<'a, AnyEndian>,
    entrypoint: VirtAddr,
    loadable_segments: Vec<LoadableSegment>,
}

#[derive(Debug)]
pub(crate) enum ElfExecutableHeaderError {
    ParseError(ParseError),
    Other(String),
}

impl<'a> ElfExecutableHeader<'a> {
    pub(crate) fn parse(bytes: &'a [u8]) -> Result<Self, ElfExecutableHeaderError> {
        let parsed = ElfBytes::<AnyEndian>::minimal_parse(bytes)
            .map_err(ElfExecutableHeaderError::ParseError)?;

        if parsed.ehdr.e_type != elf::abi::ET_EXEC {
            return Err(ElfExecutableHeaderError::Other(format!(
                "expected ET_EXEC but found {:?}",
                parsed.ehdr.e_type
            )));
        }

        if parsed.ehdr.e_machine != elf::abi::EM_X86_64 {
            return Err(ElfExecutableHeaderError::Other(format!(
                "expected EM_X86_64 but found {:?}",
                parsed.ehdr.e_machine
            )));
        }

        let entrypoint = VirtAddr::new(parsed.ehdr.e_entry);

        let Some(segments) = parsed
            .segments() else {
                return Err(ElfExecutableHeaderError::Other(
                    String::from("no segments found")
                ));
            };

        let mut loadable_segments = Vec::new();
        for program_header in segments {
            if program_header.p_type != elf::abi::PT_LOAD {
                continue;
            }

            if program_header.p_paddr > 0 && program_header.p_paddr != program_header.p_vaddr {
                return Err(ElfExecutableHeaderError::Other(format!(
                    "invalid p_addr: {program_header:?}"
                )));
            }

            let file_offset = program_header.p_offset;
            let vaddr = VirtAddr::new(program_header.p_vaddr);
            let mem_size = program_header.p_memsz;
            let flags =
                LoadableSegmentFlags::from_bits(program_header.p_flags).ok_or_else(|| {
                    ElfExecutableHeaderError::Other(format!("invalid flags: {program_header:?}"))
                })?;
            let alignment = program_header.p_align;

            loadable_segments.push(LoadableSegment {
                raw_header: program_header,
                file_offset,
                vaddr,
                mem_size,
                flags,
                alignment,
            });
        }

        Ok(Self {
            parsed,
            entrypoint,
            loadable_segments,
        })
    }
}

impl fmt::Debug for ElfExecutableHeader<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ElfExecutableHeader")
            .field("header", &self.parsed.ehdr)
            .field("entrypoint", &self.entrypoint)
            .field("loadable_segments", &self.loadable_segments)
            .finish()
    }
}

#[derive(Debug)]
#[allow(dead_code)] // TODO: Remove allow(dead_code) when we actually use this
pub(crate) struct LoadableSegment {
    raw_header: ProgramHeader,
    file_offset: u64,
    vaddr: VirtAddr,
    mem_size: u64,
    flags: LoadableSegmentFlags,
    alignment: u64,
}

bitflags! {
    #[derive(Debug)]
    #[repr(transparent)]
    pub(super) struct LoadableSegmentFlags: u32 {
        const EXECUTABLE = 1;
        const WRITABLE = 2;
        const READABLE = 4;
    }
}
