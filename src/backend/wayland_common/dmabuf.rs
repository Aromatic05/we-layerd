use std::{
    io,
    os::fd::{AsRawFd, OwnedFd},
};

const FORMAT_TABLE_ENTRY_SIZE: usize = 16;
const MAX_FORMAT_COUNT: usize = 65_536;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DmabufFormatModifier {
    pub(crate) fourcc: u32,
    pub(crate) modifier: u64,
}

#[derive(Debug, Default)]
pub(crate) struct DmabufFeedbackState {
    format_table: Vec<DmabufFormatModifier>,
    pending_formats: Vec<DmabufFormatModifier>,
    surface_formats: Vec<DmabufFormatModifier>,
    legacy_formats: Vec<DmabufFormatModifier>,
    surface_feedback_known: bool,
}

impl DmabufFeedbackState {
    pub(crate) fn add_legacy_modifier(&mut self, fourcc: u32, modifier: u64) {
        push_unique(&mut self.legacy_formats, DmabufFormatModifier { fourcc, modifier });
    }

    pub(crate) fn read_format_table(&mut self, fd: OwnedFd, size: u32) -> io::Result<()> {
        self.format_table.clear();
        self.pending_formats.clear();
        self.surface_feedback_known = false;

        let size = size as usize;
        if size == 0 {
            return Ok(());
        }
        if size % FORMAT_TABLE_ENTRY_SIZE != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("DMA-BUF format table size {size} is not a multiple of 16"),
            ));
        }
        if size / FORMAT_TABLE_ENTRY_SIZE > MAX_FORMAT_COUNT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("DMA-BUF format table contains too many entries: {}", size / 16),
            ));
        }

        let mapped = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ,
                libc::MAP_PRIVATE,
                fd.as_raw_fd(),
                0,
            )
        };
        if mapped == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }

        let bytes = unsafe { std::slice::from_raw_parts(mapped.cast::<u8>(), size) };
        let parsed = parse_format_table(bytes);
        let unmap_result = unsafe { libc::munmap(mapped, size) };
        if unmap_result != 0 {
            return Err(io::Error::last_os_error());
        }
        self.format_table = parsed?;
        Ok(())
    }

    pub(crate) fn add_tranche_indices(&mut self, indices: &[u8]) -> io::Result<()> {
        if indices.len() % 2 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("DMA-BUF tranche index array has odd size {}", indices.len()),
            ));
        }

        for bytes in indices.chunks_exact(2) {
            let index = u16::from_ne_bytes([bytes[0], bytes[1]]) as usize;
            let Some(format) = self.format_table.get(index).copied() else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "DMA-BUF tranche references format index {index}, table has {} entries",
                        self.format_table.len()
                    ),
                ));
            };
            push_unique(&mut self.pending_formats, format);
        }
        Ok(())
    }

    pub(crate) fn finish_surface_feedback(&mut self) -> Vec<DmabufFormatModifier> {
        self.surface_formats = std::mem::take(&mut self.pending_formats);
        self.surface_feedback_known = true;
        self.surface_formats.clone()
    }

    pub(crate) fn formats_for_renderer(
        &self,
        protocol_version: u32,
    ) -> Option<Vec<DmabufFormatModifier>> {
        if protocol_version >= 4 {
            return self.surface_feedback_known.then(|| self.surface_formats.clone());
        }
        if protocol_version >= 3 {
            return Some(self.legacy_formats.clone());
        }
        Some(Vec::new())
    }

    pub(crate) fn advertised_format_count(&self, protocol_version: u32) -> usize {
        if protocol_version >= 4 {
            return self.surface_formats.len();
        }
        if protocol_version >= 3 {
            return self.legacy_formats.len();
        }
        0
    }
}

fn parse_format_table(bytes: &[u8]) -> io::Result<Vec<DmabufFormatModifier>> {
    if bytes.len() % FORMAT_TABLE_ENTRY_SIZE != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "DMA-BUF format table has a partial entry",
        ));
    }

    let mut formats = Vec::with_capacity(bytes.len() / FORMAT_TABLE_ENTRY_SIZE);
    for entry in bytes.chunks_exact(FORMAT_TABLE_ENTRY_SIZE) {
        let fourcc = u32::from_ne_bytes(entry[0..4].try_into().expect("fourcc slice length"));
        let modifier = u64::from_ne_bytes(entry[8..16].try_into().expect("modifier slice length"));
        formats.push(DmabufFormatModifier { fourcc, modifier });
    }
    Ok(formats)
}

fn push_unique(formats: &mut Vec<DmabufFormatModifier>, format: DmabufFormatModifier) {
    if !formats.contains(&format) {
        formats.push(format);
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_format_table, DmabufFeedbackState, DmabufFormatModifier};

    #[test]
    fn parses_native_endian_format_table_entries() {
        let fourcc = u32::from_le_bytes(*b"AB24");
        let modifier = 0x0102_0304_0506_0708_u64;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&fourcc.to_ne_bytes());
        bytes.extend_from_slice(&0_u32.to_ne_bytes());
        bytes.extend_from_slice(&modifier.to_ne_bytes());

        assert_eq!(
            parse_format_table(&bytes).expect("valid table"),
            vec![DmabufFormatModifier { fourcc, modifier }]
        );
    }

    #[test]
    fn tranche_indices_are_deduplicated() {
        let first = DmabufFormatModifier { fourcc: 1, modifier: 2 };
        let second = DmabufFormatModifier { fourcc: 3, modifier: 4 };
        let mut state =
            DmabufFeedbackState { format_table: vec![first, second], ..Default::default() };
        let mut indices = Vec::new();
        indices.extend_from_slice(&0_u16.to_ne_bytes());
        indices.extend_from_slice(&1_u16.to_ne_bytes());
        indices.extend_from_slice(&0_u16.to_ne_bytes());
        state.add_tranche_indices(&indices).expect("valid indices");
        assert_eq!(state.finish_surface_feedback(), vec![first, second]);
    }

    #[test]
    fn version_four_formats_remain_unknown_until_done() {
        let mut state = DmabufFeedbackState::default();
        assert_eq!(state.formats_for_renderer(4), None);
        assert_eq!(state.finish_surface_feedback(), Vec::new());
        assert_eq!(state.formats_for_renderer(4), Some(Vec::new()));
    }

    #[test]
    fn legacy_modifiers_are_deduplicated() {
        let mut state = DmabufFeedbackState::default();
        state.add_legacy_modifier(1, 2);
        state.add_legacy_modifier(1, 2);
        assert_eq!(
            state.formats_for_renderer(3),
            Some(vec![DmabufFormatModifier { fourcc: 1, modifier: 2 }])
        );
    }
}
