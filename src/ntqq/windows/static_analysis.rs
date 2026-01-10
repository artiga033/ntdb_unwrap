// TODO: most code and logic of this module are done in help or totally by LLMs, needs further review.

use super::*;
use capstone::Capstone;
use capstone::arch::{BuildsCapstone, BuildsCapstoneSyntax};
use object::pe::{IMAGE_DIRECTORY_ENTRY_EXCEPTION, ImageRuntimeFunctionEntry};
use object::{Object, ObjectSection};
use snafu::{OptionExt, ResultExt};
use std::cmp::Ordering;
use std::fs;
/// All the fields are in RVA.
pub struct TargetFunction {
    /// The begin address of the function.
    /// int64 f(const void *a1, const char *a2, int64 a3, unsigned int a4)
    /// ref: https://docs.aaqwq.top/decrypt/NTQQ%20(Windows).html#%E6%89%BE%E5%88%B0%E6%95%B0%E6%8D%AE%E5%BA%93-passphrase
    pub function_offset: u64,
    pub lea_instr_offset: u64,
}
/// Disassemble the installed QQ binary to find the offset of the decryption hook function.
/// ref: https://github.com/QQBackup/QQDecrypt/blob/main/docs/decrypt/NTQQ%20(Windows).md
pub fn find_hook_function_offset(qq: &InstalledQQInfo) -> Result<TargetFunction> {
    let version_dir = {
        let versions_dir = qq.install_dir.join("versions");
        if let Some(version) = &qq.version {
            versions_dir.join(version)
        } else {
            let config_json = qq.install_dir.join("config.json");
            if config_json.exists() {
                let config = std::fs::read_to_string(config_json).context(IoSnafu {
                    op: "read config.json",
                })?;
                let cur_version_line = config
                    .lines()
                    .find(|l| l.contains("\"curVersion\":"))
                    .context(LocateDocumentsDirSnafu)?;
                let version = cur_version_line
                    .split(':')
                    .nth(1)
                    .map(|s| s.trim().trim_matches(&['"', ','][..]))
                    .context(LocateInstalledQQVersionSnafu)?
                    .to_string();
                versions_dir.join(version)
            } else {
                let mut dirs = std::fs::read_dir(&versions_dir)
                    .context(IoSnafu {
                        op: "read versions dir",
                    })?
                    .filter_map(|x| x.ok())
                    .filter(|x| x.file_type().as_ref().is_ok_and(fs::FileType::is_dir));
                let (d1, d2) = (dirs.next(), dirs.next());
                if let Some(dir) = d1
                    && d2.is_none()
                {
                    dir.path()
                } else {
                    return Err(LocateDocumentsDirSnafu.build().into());
                }
            }
        }
    };
    let wrapper_node = version_dir.join("resources/app/wrapper.node");
    let file = fs::File::open(wrapper_node).context(IoSnafu {
        op: "open wrapper.node file",
    })?;
    let data = unsafe {
        // SAFETY: the executable file should not be modified during the mapping lifetime, in practice.
        memmap2::MmapOptions::new().map(&file)
    }
    .context(IoSnafu {
        op: "mmap wrapper.node file",
    })?;
    let obj = object::File::parse(data.as_ref()).map_err(Error::from)?;
    let image_base = obj.relative_address_base();
    const TARGET_PATTERN: &[u8] = b"nt_sqlite3_key_v2: db=%p zDb=%s";
    let rdata = obj
        .section_by_name(".rdata")
        .context(FindTargetFunctionSnafu {
            msg: ".rdata section not found",
        })?;
    let rdata_data = rdata.data().map_err(Error::from)?;
    let target_addr =
        memchr::memmem::find(rdata_data, TARGET_PATTERN).context(FindTargetFunctionSnafu {
            msg: "target pattern not found in .rdata section",
        })? as u64
            + rdata.address();

    let text = obj
        .section_by_name(".text")
        .context(FindTargetFunctionSnafu {
            msg: ".text section not found",
        })?;
    let text_data = text.data().map_err(Error::from)?;
    let text_start_va = text.address();

    let cs = Capstone::new()
        .x86()
        .mode(capstone::arch::x86::ArchMode::Mode64)
        .syntax(capstone::arch::x86::ArchSyntax::Intel)
        .detail(true)
        .build()
        .map_err(Error::from)?;

    let lea_instr_offset = {
        let mut r = None;
        for i in memchr::memchr_iter(0x8d, text_data) {
            // 64位相对寻址。 Prefix(1B)    Opcode(1B)    ModR/M(1B)     Displacement(4B)
            // 所以长度固定为 7 字节
            let (Some(ins_l), ins_r) = (i.checked_sub(1), i.wrapping_add(6)) else {
                continue;
            };
            let code = &text_data[ins_l..ins_r];
            let addr = text_start_va + ins_l as u64;
            let Ok(insn) = cs.disasm_count(code, addr, 1) else {
                continue;
            };
            let Some(insn) = insn.into_iter().next() else {
                continue;
            };
            if insn.id().0 != capstone::arch::x86::X86Insn::X86_INS_LEA as u32 {
                continue;
            }
            let detail = cs.insn_detail(&insn).map_err(Error::from)?;
            let ops = detail.arch_detail().operands();
            let Some(capstone::arch::ArchOperand::X86Operand(op)) = ops.get(1) else {
                continue;
            };
            let capstone::arch::x86::X86OperandType::Mem(mem) = op.op_type else {
                continue;
            };
            if mem.base().0 != capstone::arch::x86::X86Reg::X86_REG_RIP as u16 {
                continue;
            }
            let lea_target_addr = insn
                .address()
                .wrapping_add(insn.len() as u64)
                .wrapping_add(mem.disp() as u64);
            if lea_target_addr == target_addr {
                r = Some(insn.address() - image_base);
                break;
            } else {
                continue;
            }
        }
        r
    };
    let Some(lea_instr_offset) = lea_instr_offset else {
        return Err(FindTargetFunctionSnafu {
            msg: "LEA instruction not found",
        }
        .build()
        .into());
    };
    let object::File::Pe64(pe) = obj else {
        return Err(FindTargetFunctionSnafu {
            msg: "wrapper.node is not PE64 format",
        }
        .build()
        .into());
    };
    let dir =
        pe.data_directory(IMAGE_DIRECTORY_ENTRY_EXCEPTION)
            .context(FindTargetFunctionSnafu {
                msg: "get exception data directory",
            })?;
    let section_table = pe.section_table();
    let data = dir.data(pe.data(), &section_table).map_err(Error::from)?;
    // SAFETY: prechecked PE format and data directory
    let entries: &[ImageRuntimeFunctionEntry] = unsafe {
        std::slice::from_raw_parts(
            data.as_ptr() as *const ImageRuntimeFunctionEntry,
            data.len() / std::mem::size_of::<ImageRuntimeFunctionEntry>(),
        )
    };
    let target_rva = lea_instr_offset as u32;
    let located_entry = entries
        .binary_search_by(|entry| {
            if target_rva < entry.begin_address.get(Default::default()) {
                // 目标 RVA 小于当前函数的起始 -> 当前条目偏大 -> 往左找
                Ordering::Greater
            } else if target_rva >= entry.end_address.get(Default::default()) {
                // 目标 RVA 大于等于当前函数的结束 -> 当前条目偏小 -> 往右找
                Ordering::Less
            } else {
                // begin <= target_rva < end -> 命中！
                Ordering::Equal
            }
        })
        .ok()
        .context(FindTargetFunctionSnafu {
            msg: "located function begin address through exception directory",
        })?;
    let function_offset = entries[located_entry].begin_address.get(Default::default()) as u64;
    Ok(TargetFunction {
        function_offset,
        lea_instr_offset,
    })
}
