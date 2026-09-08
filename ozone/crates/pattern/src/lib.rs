use std::io::{BufReader, Read};
use std::io::Seek;

use thiserror::Error;

fn decode_adrp(instruction: u32) -> usize {
    let immhi = (instruction & 0b0000_0000_1111_1111_1111_1111_1110_0000) >> 3;
    let immlo = (instruction & 0b0110_0000_0000_0000_0000_0000_0000_0000) >> 29;
    let imm = ((immhi | immlo) << 12) as i32 as usize;
    imm
}

/// Decode a LDR Immediate instruction and returns the immediate.
fn decode_ldr_imm(instruction: u32) -> usize {
    let size = (instruction & 0b1100_0000_0000_0000_0000_0000_0000_0000) >> 30;
    let imm = (instruction & 0b0000_0000_0011_1111_1111_1100_0000_0000) >> 10;
    (imm as usize) << size
}

fn decode_add_imm(instruction: u32) -> usize {
    let imm = (instruction & 0b001111111111111000000000) >> 10;
    imm as usize
}

/// Can be used to Branch and Branch-Link instructions
fn decode_branch(instruction: u32, curr_address: usize) -> usize {
    let imm = instruction & 0b0011111111111111111111111111;
    (curr_address + (imm << 2) as usize) & 0b0011111111111111111111111111
}

pub enum PatternCommand {
    FindInstruction { pattern: String },
    DecodeAdrp,
    DecodeLdr,
    DecodeAdd,
    DecodeBranch,
    Offset(u64),
}

pub struct PatternCommandBuffer {
    commands: Vec<PatternCommand>
}

#[derive(Error, Debug)]
pub enum SignatureSearchError {
    #[error("the data for key `{0}` is not available")]
    Redaction(String),
    #[error("no offset could be found for this signature")]
    NotFound
}

pub type PatternSearchResult<T> = Result<T, SignatureSearchError>;

impl PatternCommandBuffer {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub fn find_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.commands.push(PatternCommand::FindInstruction { pattern: pattern.into() });

        self
    }

    pub fn search(self, slice: impl AsRef<[u8]>) -> PatternSearchResult<usize> {
        let mut reader = std::io::Cursor::new(slice.as_ref());
        let mut out_offset = 0;
        let mut buffer = [0u8;4];

        for command in self.commands {
            match command {
                PatternCommand::FindInstruction { pattern } => {
                    out_offset = lazysimd::get_offset_neon(reader.get_ref(), &pattern).unwrap();
                },
                PatternCommand::DecodeAdrp => {
                    reader.read_exact(&mut buffer).unwrap();
                    out_offset = decode_adrp(u32::from_le_bytes(buffer));
                },
                PatternCommand::DecodeLdr => {
                    reader.read_exact(&mut buffer).unwrap();
                    out_offset += decode_ldr_imm(u32::from_le_bytes(buffer));
                },
                PatternCommand::DecodeAdd => {
                    reader.read_exact(&mut buffer).unwrap();
                    out_offset += decode_add_imm(u32::from_le_bytes(buffer));
                },
                PatternCommand::DecodeBranch => {
                    reader.read_exact(&mut buffer).unwrap();
                    reader.seek(std::io::SeekFrom::Start(decode_branch(u32::from_le_bytes(buffer), reader.position() as usize) as u64)).unwrap();
                    out_offset = reader.position() as usize;
                },
                PatternCommand::Offset(offset) => {
                    reader.seek(std::io::SeekFrom::Current(offset as i64)).unwrap();
                },
            }
        }

        Ok(out_offset)
    }
}

impl Default for PatternCommandBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_inst() {
        let memory = &[0x69, 0x69, 0x69, 0x69, 0x69, 0x69, 0x69, 0x69, 0x69, 0x69, 0x42, 0x69, 0x69, 0x69, 0x69, 0x69, 0x69, 0x69, 0x69, 0x69, 0x69, 0x69, 0x69, 0x69];

        let offset = PatternCommandBuffer::new()
            .find_pattern("42 69")
            .search(memory)
            .unwrap();

        assert_eq!(offset, 0xA)
    }

    #[test]
    fn adrp() {
        // adrp x9, #0x43c000
        let memory = [0xe9, 0x21, 0x00, 0x90];
        let offset = decode_adrp(u32::from_le_bytes(memory));

        assert_eq!(offset, 0x43c000)
    }

    #[test]
    fn ldr_imm() {
        // ldr x20, [x20, #0x40]
        let memory = [0x94, 0x22, 0x40, 0xf9];
        let offset = decode_ldr_imm(u32::from_le_bytes(memory));

        assert_eq!(offset, 0x40)
    }

    #[test]
    fn add_imm() {
        // ldr x20, [x20, #0x40]
        let memory = [0x29, 0x81, 0x2f, 0x91];
        let offset = decode_add_imm(u32::from_le_bytes(memory));

        assert_eq!(offset, 0xBE0)
    }

    #[test]
    fn bl() {
        // bl #0xfffffffffffffa1c
        let memory = [0x87, 0xfe, 0xff, 0x97];
        let offset = decode_branch(u32::from_le_bytes(memory), 0x2cb314);

        // Jumping backwards
        assert_eq!(offset, 0x2cad30);
    }
}
