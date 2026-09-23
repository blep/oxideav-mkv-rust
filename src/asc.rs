//! AAC `AudioSpecificConfig` channel parsing.
//!
//! Matroska stores an AAC track's `AudioSpecificConfig` (ISO/IEC 14496-3
//! §1.6.2.1) in its `CodecPrivate`. The container `Channels` element is often
//! a placeholder — notably 1 for HE-AACv2, whose parametric stereo decodes a
//! mono core to a stereo output — so the decoder configuration is the
//! authoritative source for the channel count.

/// Output channel count from an `AudioSpecificConfig`.
///
/// Returns `None` for a truncated config or a zero channel configuration
/// (where the layout lives in a program config element).
pub(crate) fn channel_count(asc: &[u8]) -> Option<u16> {
    // ISO/IEC 14496-3 Table 1.19.
    const CHANNELS: [u16; 14] = [0, 1, 2, 3, 4, 5, 6, 8, 0, 0, 0, 7, 8, 24];
    let mut reader = BitReader::new(asc);
    let audio_object_type = reader.read_bits(5)?;
    if audio_object_type == 31 {
        reader.read_bits(6)?;
    }
    if reader.read_bits(4)? == 0xF {
        reader.read_bits(24)?;
    }
    let channel_configuration = reader.read_bits(4)? as usize;
    let mut channels = CHANNELS.get(channel_configuration).copied().unwrap_or(0);
    // HE-AACv2 (SBR + parametric stereo) signals a mono core but decodes to
    // stereo, either via `audioObjectType` 29 or via the SBR/PS sync
    // extension; report the output layout.
    if audio_object_type == 29 && channels < 2 {
        channels = 2;
    }
    if channels == 1 && has_parametric_stereo(asc) {
        channels = 2;
    }
    (channels > 0).then_some(channels)
}

/// Detect the parametric-stereo (PS) sync extension: locate the `0x2B7`
/// sync extension, confirm SBR (`extensionAudioObjectType == 5`,
/// `sbrPresentFlag == 1`), then read the `0x548` PS sync and its flag.
fn has_parametric_stereo(asc: &[u8]) -> bool {
    let total_bits = asc.len() * 8;
    let mut offset = 0usize;
    while offset + 33 <= total_bits {
        if bits_at(asc, offset, 11) == Some(0x2B7)
            && bits_at(asc, offset + 11, 5) == Some(5)
            && bits_at(asc, offset + 16, 1) == Some(1)
            && bits_at(asc, offset + 21, 11) == Some(0x548)
            && bits_at(asc, offset + 32, 1) == Some(1)
        {
            return true;
        }
        offset += 1;
    }
    false
}

/// Read `count` bits at an absolute bit offset (MSB-first).
fn bits_at(buf: &[u8], offset: usize, count: usize) -> Option<u32> {
    let mut value = 0u32;
    for index in offset..offset + count {
        let byte = buf.get(index / 8)?;
        let bit = (byte >> (7 - (index % 8))) & 1;
        value = (value << 1) | u32::from(bit);
    }
    Some(value)
}

/// MSB-first bit reader.
struct BitReader<'a> {
    bytes: &'a [u8],
    bit: usize,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, bit: 0 }
    }

    fn read_bits(&mut self, count: usize) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..count {
            let byte = self.bytes.get(self.bit / 8)?;
            let bit = (byte >> (7 - (self.bit % 8))) & 1;
            value = (value << 1) | u32::from(bit);
            self.bit += 1;
        }
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::channel_count;

    #[test]
    fn reads_channel_configurations() {
        assert_eq!(channel_count(&[0x11, 0xB0]), Some(6));
        assert_eq!(channel_count(&[0x11, 0x90]), Some(2));
        assert_eq!(channel_count(&[0x11, 0x88]), Some(1));
    }

    #[test]
    fn promotes_he_aacv2_to_stereo() {
        // audioObjectType 29 (mono core).
        assert_eq!(channel_count(&[0xE9, 0x88]), Some(2));
        // Parametric-stereo sync extension (real HE-AACv2 CodecPrivate).
        assert_eq!(
            channel_count(&[0x13, 0x88, 0x56, 0xE5, 0xA5, 0x48, 0x80]),
            Some(2)
        );
    }

    #[test]
    fn rejects_truncated_or_zero_configuration() {
        assert_eq!(channel_count(&[0x11]), None);
        assert_eq!(channel_count(&[0x11, 0x80]), None);
    }
}
